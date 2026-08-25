//! Launches the DeepSeek Harness `dsh web` server and discovers its URL.
//!
//! The desktop shell does not reimplement the harness: it starts the real
//! `dsh` CLI (the same command that serves the browser GUI) as a child
//! process, waits for it to announce its loopback URL on stdout, and then
//! points a native webview at that URL. On shutdown it tears the whole
//! process group down so no orphaned server survives the window.

use std::{
    env,
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

/// Look a command up on `PATH`, honouring Windows executable extensions.
fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    #[cfg(windows)]
    let exts: &[&str] = &[".exe", ".cmd", ".bat", ""];
    #[cfg(not(windows))]
    let exts: &[&str] = &[""];

    for dir in env::split_paths(&path) {
        for ext in exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Append the `web --no-open --port <port>` arguments to a command.
fn add_web_args(cmd: &mut Command) {
    let port = env::var("DSH_DESKTOP_PORT")
        .ok()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| "0".to_string());

    cmd.arg("web").arg("--no-open").arg("--port").arg(port);
}

/// Resolve the backend command to run, in order of preference:
///
/// 1. `DSH_DESKTOP_BACKEND` — explicit path to a `dsh` executable or to a
///    `.js`/`.mjs`/`.cjs` entry run through `node`.
/// 2. A bundled backend under `<root>/backend/node_modules/@deepseek-ai/dsh`.
/// 3. `dsh` on `PATH`.
/// 4. `npx --yes @deepseek-ai/dsh@latest`.
fn build_command(search_roots: &[PathBuf]) -> Result<Command, String> {
    if let Ok(backend) = env::var("DSH_DESKTOP_BACKEND") {
        let backend = backend.trim().to_string();
        if !backend.is_empty() {
            let mut cmd = if backend.ends_with(".js")
                || backend.ends_with(".mjs")
                || backend.ends_with(".cjs")
            {
                let mut c = Command::new("node");
                c.arg(&backend);
                c
            } else {
                Command::new(&backend)
            };
            add_web_args(&mut cmd);
            return Ok(cmd);
        }
    }

    for root in search_roots {
        let bin = root
            .join("backend")
            .join("node_modules")
            .join("@deepseek-ai")
            .join("dsh")
            .join("lib")
            .join("bin.js");
        if bin.is_file() {
            let mut cmd = Command::new("node");
            cmd.arg(bin);
            add_web_args(&mut cmd);
            return Ok(cmd);
        }
    }

    if let Some(dsh) = find_on_path("dsh") {
        let mut cmd = Command::new(dsh);
        add_web_args(&mut cmd);
        return Ok(cmd);
    }

    if let Some(npx) = find_on_path("npx") {
        let mut cmd = Command::new(npx);
        cmd.args(["--yes", "@deepseek-ai/dsh@latest"]);
        add_web_args(&mut cmd);
        return Ok(cmd);
    }

    Err(
        "no DeepSeek Harness backend found: set DSH_DESKTOP_BACKEND, run `npm run backend:install`, \
         or install Node.js and `dsh`"
            .to_string(),
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

/// Start the backend and block until it announces its URL.
pub fn spawn_backend(search_roots: &[PathBuf]) -> Result<(Backend, String), String> {
    let mut cmd = build_command(search_roots)?;

    // Run the server as its own process-group leader so `terminate_group` can
    // reap the bash/pwsh tool subprocesses it spawns later.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to start the DeepSeek Harness backend: {e}"))?;
    let pid = child.id();

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Drain both streams on a background thread. The first URL line is
    // forwarded; everything after keeps being drained so a full pipe can never
    // stall the server.
    let (tx, rx) = mpsc::channel::<String>();
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
                if announced {
                    // Keep draining; log in debug builds only.
                    #[cfg(debug_assertions)]
                    eprintln!("[dsh web] {line}");
                }
            }
        }
        if let Some(stderr) = stderr {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                #[cfg(debug_assertions)]
                eprintln!("[dsh web] {line}");
            }
        }
    });

    let url = match rx.recv_timeout(READY_TIMEOUT) {
        Ok(url) => url,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            terminate_group(pid);
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "backend did not become ready within {}s",
                READY_TIMEOUT.as_secs()
            ));
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            terminate_group(pid);
            let _ = child.kill();
            let _ = child.wait();
            return Err("backend exited before announcing its URL".to_string());
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

    roots
}

#[cfg(test)]
mod tests {
    use super::extract_url;

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
}
