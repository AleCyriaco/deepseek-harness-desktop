//! Launches the DeepSeek Harness `dsh web` server and discovers its URL.
//!
//! The desktop shell does not reimplement the harness: it starts the real
//! `dsh` CLI (the same command that serves the browser GUI) as a child
//! process, waits for it to announce its loopback URL on stdout, and then
//! points a native webview at that URL. On shutdown it tears the whole
//! process group down so no orphaned server survives the window.

use std::{
    cmp::Reverse,
    env,
    ffi::OsStr,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use tauri::Manager;

/// How long to wait for the server to print its URL before giving up.
const READY_TIMEOUT: Duration = Duration::from_secs(45);

/// The exact prefix the web app prints when it is listening.
const URL_PREFIX: &str = "http://127.0.0.1:";

/// Executable extensions to try when looking a command up by name.
#[cfg(windows)]
const EXE_EXTS: &[&str] = &[".exe", ".cmd", ".bat", ""];
#[cfg(not(windows))]
const EXE_EXTS: &[&str] = &[""];

/// A live `dsh web` process plus enough state to kill its whole tree later.
pub struct Backend {
    child: Child,
    /// Process id; on unix this doubles as the process-group id because the
    /// child is started as its own group leader.
    pid: u32,
}

impl Backend {
    /// Gracefully stop the server (SIGTERM to the group), then force-kill.
    pub fn shutdown(&mut self) {
        terminate_group(self.pid);
        // Give the Cordis host a beat to flush, then hard-kill what remains.
        thread::sleep(Duration::from_millis(600));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        // Backstop: if the app exits without an explicit `shutdown`, make sure
        // the child server does not outlive us.
        terminate_group(self.pid);
        let _ = self.child.kill();
    }
}

/// Extract `http://127.0.0.1:<port>` from a stdout line such as
/// `dsh web: http://127.0.0.1:54321`.
fn extract_url(line: &str) -> Option<String> {
    let pos = line.find(URL_PREFIX)?;
    let rest = &line[pos + URL_PREFIX.len()..];
    let port: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if port.is_empty() {
        None
    } else {
        Some(format!("{URL_PREFIX}{port}"))
    }
}

/// Parse a Node version directory name (`v20.11.0`, `20.11.0`) into a sortable
/// triple. Anything unparseable sorts last as `(0, 0, 0)`.
fn parse_version(name: &str) -> (u32, u32, u32) {
    let mut parts = name.trim_start_matches('v').split('.');
    let mut next = || {
        parts
            .next()
            .and_then(|p| p.split(|c: char| !c.is_ascii_digit()).next())
            .and_then(|p| p.parse::<u32>().ok())
            .unwrap_or(0)
    };
    (next(), next(), next())
}

/// `bin` directories of Node installs managed by nvm or fnm, newest first.
///
/// Version managers install outside every system directory, so a packaged app
/// would never find them without looking here explicitly.
fn version_manager_bin_dirs(home: &Path) -> Vec<PathBuf> {
    let roots = [
        home.join(".nvm").join("versions").join("node"),
        home.join(".local")
            .join("share")
            .join("fnm")
            .join("node-versions"),
        home.join("Library")
            .join("Application Support")
            .join("fnm")
            .join("node-versions"),
    ];

    let mut found: Vec<((u32, u32, u32), PathBuf)> = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let version = parse_version(&entry.file_name().to_string_lossy());
            let dir = entry.path();
            // nvm keeps `bin` at the top; fnm nests it under `installation`.
            found.push((version, dir.join("bin")));
            found.push((version, dir.join("installation").join("bin")));
        }
    }

    found.sort_by_key(|(version, _)| Reverse(*version));
    found.into_iter().map(|(_, dir)| dir).collect()
}

/// Directories to search when a tool is not on `PATH`.
///
/// This list exists because of one specific failure: a macOS `.app` launched
/// from Finder inherits launchd's minimal `PATH` (`/usr/bin:/bin:/usr/sbin:
/// /sbin`), which contains no Node installation at all. Without these
/// fallbacks a packaged app would fail to start for every user who installed
/// Node with Homebrew, MacPorts, nvm, fnm, Volta or asdf — which is nearly
/// everyone. The same reasoning applies to Linux desktop launchers.
fn extra_bin_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/opt/local/bin")); // MacPorts
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/usr/bin"));
        dirs.push(PathBuf::from("/snap/bin"));
    }
    #[cfg(windows)]
    {
        if let Some(program_files) = env::var_os("ProgramFiles") {
            dirs.push(Path::new(&program_files).join("nodejs"));
        }
        if let Some(local) = env::var_os("LOCALAPPDATA") {
            dirs.push(Path::new(&local).join("Programs").join("nodejs"));
            dirs.push(Path::new(&local).join("Volta").join("bin"));
        }
        if let Some(appdata) = env::var_os("APPDATA") {
            dirs.push(Path::new(&appdata).join("npm"));
        }
    }

    if let Some(home) = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        dirs.push(home.join(".volta").join("bin"));
        dirs.push(home.join(".asdf").join("shims"));
        dirs.push(home.join(".local").join("bin"));
        dirs.extend(version_manager_bin_dirs(&home));
    }

    dirs
}

/// Strip Windows' extended-length (`\\?\`) prefix from a path.
///
/// `resource_dir()` hands back a verbatim path on Windows. Every Win32 API
/// accepts it, so `is_file()` and the rest of our resolution succeed — but
/// Node does not: given `\\?\C:\...\bin.js` it fails with
/// `EISDIR: lstat 'C:'` and exits before announcing its URL. Anything handed
/// to a child process therefore has to be a plain path.
fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        // `\\?\UNC\server\share` is really `\\server\share`.
        if let Some(unc) = rest.strip_prefix("UNC\\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        // `\\?\C:\...` is really `C:\...`.
        let bytes = rest.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}

/// Look a command up on `PATH`, honouring Windows executable extensions.
fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        if let Some(found) = probe_dir(&dir, name) {
            return Some(found);
        }
    }
    None
}

/// Return `<dir>/<name><ext>` for the first extension that is a real file.
fn probe_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    for ext in EXE_EXTS {
        let candidate = dir.join(format!("{name}{ext}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Resolve a command by name: `PATH` first, then the well-known install
/// directories a GUI launcher does not put on `PATH`.
fn find_tool(name: &str) -> Option<PathBuf> {
    find_on_path(name).or_else(|| extra_bin_dirs().iter().find_map(|dir| probe_dir(dir, name)))
}

/// Locate the Node interpreter, honouring `DSH_DESKTOP_NODE`.
fn find_node() -> Option<PathBuf> {
    if let Some(explicit) = non_empty_env("DSH_DESKTOP_NODE") {
        return Some(PathBuf::from(explicit));
    }
    find_tool("node")
}

/// Read an environment variable, treating blank values as unset.
fn non_empty_env(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Does this path look like a JavaScript entry point rather than a binary?
fn is_js_entry(path: &str) -> bool {
    [".js", ".mjs", ".cjs"]
        .iter()
        .any(|ext| path.ends_with(ext))
}

/// Prepend a directory to the child's `PATH`.
///
/// The harness spawns its own tool subprocesses (`npm`, `npx`, language
/// servers) which expect to find Node the same way we did. Handing them the
/// resolved directory keeps them working under a Finder-launched app.
fn prepend_path(cmd: &mut Command, dir: &Path) {
    let mut dirs = vec![dir.to_path_buf()];
    if let Some(existing) = env::var_os("PATH") {
        dirs.extend(env::split_paths(&existing));
    }
    if let Ok(joined) = env::join_paths(dirs) {
        cmd.env("PATH", joined);
    }
}

/// Build a `Command` for an executable, exposing its own directory to the
/// child so sibling tools remain reachable.
fn command_at(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    if let Some(dir) = program.parent() {
        if !dir.as_os_str().is_empty() {
            prepend_path(&mut cmd, dir);
        }
    }
    cmd
}

/// Append the `web --no-open --port <port>` arguments to a command.
fn add_web_args(cmd: &mut Command) {
    let port = non_empty_env("DSH_DESKTOP_PORT").unwrap_or_else(|| "0".to_string());
    cmd.arg("web").arg("--no-open").arg("--port").arg(port);
}

/// Resolve the backend command to run, in order of preference:
///
/// 1. `DSH_DESKTOP_BACKEND` — explicit path to a `dsh` executable or to a
///    `.js`/`.mjs`/`.cjs` entry run through `node`.
/// 2. A bundled backend under `<root>/backend/node_modules/@deepseek-ai/dsh`.
/// 3. `dsh` on `PATH` (or in a well-known install directory).
/// 4. `npx --yes @deepseek-ai/dsh@latest`.
fn build_command(search_roots: &[PathBuf]) -> Result<Command, String> {
    let node = find_node();

    if let Some(backend) = non_empty_env("DSH_DESKTOP_BACKEND") {
        let mut cmd = if is_js_entry(&backend) {
            let node = node.ok_or_else(|| missing_node_message("DSH_DESKTOP_BACKEND"))?;
            let mut cmd = command_at(&node);
            cmd.arg(&backend);
            cmd
        } else {
            command_at(Path::new(&backend))
        };
        add_web_args(&mut cmd);
        return Ok(cmd);
    }

    if let Some(node) = node.as_ref() {
        for root in search_roots {
            let bin = root
                .join("backend")
                .join("node_modules")
                .join("@deepseek-ai")
                .join("dsh")
                .join("lib")
                .join("bin.js");
            if bin.is_file() {
                let mut cmd = command_at(node);
                cmd.arg(bin);
                add_web_args(&mut cmd);
                return Ok(cmd);
            }
        }
    }

    if let Some(dsh) = find_tool("dsh") {
        let mut cmd = command_at(&dsh);
        add_web_args(&mut cmd);
        return Ok(cmd);
    }

    if let Some(npx) = find_tool("npx") {
        let mut cmd = command_at(&npx);
        cmd.args(["--yes", "@deepseek-ai/dsh@latest"]);
        add_web_args(&mut cmd);
        return Ok(cmd);
    }

    if node.is_none() {
        return Err(missing_node_message("the bundled backend"));
    }

    Err(
        "no DeepSeek Harness backend found: set DSH_DESKTOP_BACKEND, run `npm run backend:install`, \
         or install Node.js and `dsh`"
            .to_string(),
    )
}

/// The diagnostic shown when Node cannot be located anywhere.
fn missing_node_message(needed_by: &str) -> String {
    format!(
        "Node.js not found, which {needed_by} requires. Install Node.js 20 or newer, \
         or set DSH_DESKTOP_NODE to the full path of the `node` binary."
    )
}

/// Kill a process group (unix) or a process tree (Windows).
#[cfg(unix)]
fn terminate_group(pid: u32) {
    unsafe {
        libc::kill(-(pid as i32), libc::SIGTERM);
    }
}

#[cfg(windows)]
fn terminate_group(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output();
}

/// Render a command as a readable string for diagnostics.
fn describe(cmd: &Command) -> String {
    let args: Vec<String> = cmd
        .get_args()
        .map(OsStr::to_string_lossy)
        .map(|arg| arg.into_owned())
        .collect();
    format!("{} {}", cmd.get_program().to_string_lossy(), args.join(" "))
}

/// Start the backend and block until it announces its URL.
pub fn spawn_backend(search_roots: &[PathBuf]) -> Result<(Backend, String), String> {
    let mut cmd = build_command(search_roots)?;
    let description = describe(&cmd);

    // Run the server as its own process-group leader so `terminate_group` can
    // reap the bash/pwsh tool subprocesses it spawns later.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        format!("failed to start the DeepSeek Harness backend ({description}): {e}")
    })?;
    let pid = child.id();

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Drain both streams on a background thread. The first URL line is
    // forwarded; everything after keeps being drained so a full pipe can never
    // stall the server. Recent stderr is kept so a failed start can explain
    // itself instead of reporting a bare timeout.
    let (tx, rx) = mpsc::channel::<String>();
    let (err_tx, err_rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let mut announced = false;
        if let Some(stdout) = stdout {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                if !announced {
                    if let Some(url) = extract_url(&line) {
                        announced = true;
                        let _ = tx.send(url);
                    }
                }
                #[cfg(debug_assertions)]
                eprintln!("[dsh web] {line}");
            }
        }
        if let Some(stderr) = stderr {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                #[cfg(debug_assertions)]
                eprintln!("[dsh web] {line}");
                let _ = err_tx.send(line);
            }
        }
    });

    let fail = |pid: u32, child: &mut Child, reason: String| -> String {
        terminate_group(pid);
        let _ = child.kill();
        let _ = child.wait();
        let tail: Vec<String> = err_rx.try_iter().collect();
        if tail.is_empty() {
            reason
        } else {
            let tail = tail[tail.len().saturating_sub(10)..].join("\n  ");
            format!("{reason}\n  {tail}")
        }
    };

    let url = match rx.recv_timeout(READY_TIMEOUT) {
        Ok(url) => url,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let reason = format!(
                "backend did not become ready within {}s ({description})",
                READY_TIMEOUT.as_secs()
            );
            return Err(fail(pid, &mut child, reason));
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            let reason = format!("backend exited before announcing its URL ({description})");
            return Err(fail(pid, &mut child, reason));
        }
    };

    Ok((Backend { child, pid }, url))
}

/// Search roots where a bundled backend may live: the packaged resource dir,
/// the development project root (found by walking up from the executable),
/// the current working directory, and the cargo manifest dir.
pub fn search_roots(app: &tauri::App) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();

    // Packaged builds: the resource dir holds the bundled backend.
    if let Ok(res) = app.path().resource_dir() {
        roots.push(res);
    }

    // Development: walk up from the executable to find the project root.
    if let Ok(exe) = env::current_exe() {
        let mut dir = exe.parent().map(Path::to_path_buf);
        for _ in 0..6 {
            match dir {
                Some(d) => {
                    roots.push(d.clone());
                    dir = d.parent().map(Path::to_path_buf);
                }
                None => break,
            }
        }
    }

    // Running from the project root.
    if let Ok(cwd) = env::current_dir() {
        roots.push(cwd);
    }

    // Cargo dev builds: the manifest dir's parent is the project root.
    if let Ok(dir) = env::var("CARGO_MANIFEST_DIR") {
        roots.push(Path::new(&dir).join(".."));
    }

    // Normalise here rather than at the point of use, so every path derived
    // from a root is already safe to hand to Node.
    roots
        .iter()
        .map(|root| strip_verbatim_prefix(root))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{extract_url, is_js_entry, parse_version, strip_verbatim_prefix};
    use std::path::{Path, PathBuf};

    #[test]
    fn extracts_announced_loopback_url() {
        assert_eq!(
            extract_url("dsh web: http://127.0.0.1:54321"),
            Some("http://127.0.0.1:54321".to_string())
        );
    }

    #[test]
    fn extracts_url_with_lan_suffix() {
        assert_eq!(
            extract_url("dsh web: http://127.0.0.1:50340 (LAN: http://192.168.1.5:50340)"),
            Some("http://127.0.0.1:50340".to_string())
        );
    }

    #[test]
    fn ignores_unrelated_lines() {
        assert_eq!(extract_url("listening on 127.0.0.1"), None);
        assert_eq!(extract_url(""), None);
    }

    #[test]
    fn parses_node_version_directory_names() {
        assert_eq!(parse_version("v20.11.0"), (20, 11, 0));
        assert_eq!(parse_version("22.3.1"), (22, 3, 1));
        assert_eq!(parse_version("v18"), (18, 0, 0));
        assert_eq!(parse_version("not-a-version"), (0, 0, 0));
    }

    #[test]
    fn orders_node_versions_numerically_not_lexically() {
        // The bug this guards: "v9" sorts after "v20" as a string.
        let mut names = ["v9.0.0", "v20.11.0", "v18.19.1"];
        names.sort_by_key(|n| std::cmp::Reverse(parse_version(n)));
        assert_eq!(names, ["v20.11.0", "v18.19.1", "v9.0.0"]);
    }

    #[test]
    fn strips_the_windows_verbatim_prefix() {
        // The bug this guards: Node fails with `EISDIR: lstat 'C:'` when it is
        // handed a `\\?\` path, so a packaged Windows build never started.
        assert_eq!(
            strip_verbatim_prefix(Path::new(r"\\?\C:\Users\max\app\bin.js")),
            PathBuf::from(r"C:\Users\max\app\bin.js")
        );
        assert_eq!(
            strip_verbatim_prefix(Path::new(r"\\?\UNC\server\share\app")),
            PathBuf::from(r"\\server\share\app")
        );
    }

    #[test]
    fn leaves_ordinary_paths_untouched() {
        assert_eq!(
            strip_verbatim_prefix(Path::new("/usr/local/bin/node")),
            PathBuf::from("/usr/local/bin/node")
        );
        assert_eq!(
            strip_verbatim_prefix(Path::new(r"C:\Program Files\nodejs")),
            PathBuf::from(r"C:\Program Files\nodejs")
        );
    }

    #[test]
    fn recognises_javascript_entry_points() {
        assert!(is_js_entry("/path/lib/bin.js"));
        assert!(is_js_entry("/path/lib/bin.mjs"));
        assert!(is_js_entry("/path/lib/bin.cjs"));
        assert!(!is_js_entry("/usr/local/bin/dsh"));
    }
}
