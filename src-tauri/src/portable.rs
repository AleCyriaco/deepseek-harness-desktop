//! The portable single-executable build.
//!
//! The harness runtime and a Node interpreter are compressed into the binary
//! at build time (see `build.rs`) and unpacked once, on first run, into a
//! per-user cache directory. After that the app starts from the unpacked copy
//! and needs nothing from the machine — no Node install, no network.
//!
//! The alternative, fetching the harness through `npx` on first run, meant a
//! multi-minute download, a hard dependency on the registry being reachable,
//! and whatever Node happened to be installed. This trades ~100 MB of binary
//! for none of that.

use std::{
    env,
    fs::{self, File},
    io::{self, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use flate2::read::DeflateDecoder;

/// The packed runtime, produced by `build.rs`.
const PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/portable-payload.bin"));

/// Written once the unpack finished, so an interrupted run is never mistaken
/// for a usable runtime.
const MARKER: &str = ".unpacked";

/// Where the unpacked runtime lives, versioned so an upgrade unpacks afresh
/// instead of mixing files from two releases.
fn cache_dir() -> Result<PathBuf, String> {
    let base = if cfg!(windows) {
        env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache"))
    }
    .ok_or_else(|| "could not locate a per-user cache directory".to_string())?;

    Ok(base
        .join("DeepSeek Harness Desktop")
        .join(format!("runtime-{}", env!("CARGO_PKG_VERSION"))))
}

/// Read exactly `len` bytes, or fail.
fn read_exact<R: Read>(source: &mut R, len: usize) -> io::Result<Vec<u8>> {
    let mut buffer = vec![0u8; len];
    source.read_exact(&mut buffer)?;
    Ok(buffer)
}

/// Copy `len` bytes from the archive into `sink`, in chunks — the payload
/// holds a ~100 MB Node binary, which must never be held in memory whole.
fn copy_exact<R: Read, W: Write>(source: &mut R, sink: &mut W, len: u64) -> io::Result<()> {
    let mut remaining = len;
    let mut buffer = vec![0u8; 64 * 1024];
    while remaining > 0 {
        let want = remaining.min(buffer.len() as u64) as usize;
        source.read_exact(&mut buffer[..want])?;
        sink.write_all(&buffer[..want])?;
        remaining -= want as u64;
    }
    Ok(())
}

/// Refuse paths that would escape the destination directory.
fn is_safe(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.contains("..")
        && !Path::new(name).is_absolute()
}

/// Unpack the payload into `target`.
fn unpack(target: &Path, on_progress: &dyn Fn(&str)) -> Result<(), String> {
    let mut archive = DeflateDecoder::new(PAYLOAD);
    let mut files = 0usize;

    loop {
        // Each record is: u32 path length, path, u64 data length, data.
        let mut header = [0u8; 4];
        match archive.read_exact(&mut header) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("reading the bundled runtime failed: {e}")),
        }

        let name_len = u32::from_le_bytes(header) as usize;
        let name = read_exact(&mut archive, name_len)
            .map_err(|e| format!("reading the bundled runtime failed: {e}"))?;
        let name =
            String::from_utf8(name).map_err(|_| "the bundled runtime is corrupt".to_string())?;

        let mut size = [0u8; 8];
        archive
            .read_exact(&mut size)
            .map_err(|e| format!("reading the bundled runtime failed: {e}"))?;
        let size = u64::from_le_bytes(size);

        if !is_safe(&name) {
            return Err(format!(
                "the bundled runtime contains an unsafe path: {name}"
            ));
        }

        let path = target.join(&name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }

        let file =
            File::create(&path).map_err(|e| format!("could not write {}: {e}", path.display()))?;
        let mut sink = BufWriter::new(file);
        copy_exact(&mut archive, &mut sink, size)
            .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        sink.flush()
            .map_err(|e| format!("could not write {}: {e}", path.display()))?;

        #[cfg(unix)]
        if !name.contains('/') {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o755));
        }

        files += 1;
        if files % 250 == 0 {
            on_progress(&format!("unpacked {files} files"));
        }
    }

    on_progress(&format!("unpacked {files} files"));
    Ok(())
}

/// The unpacked runtime directory, unpacking it first if necessary.
pub fn prepare(on_progress: &dyn Fn(&str)) -> Result<PathBuf, String> {
    let dir = cache_dir()?;
    if dir.join(MARKER).is_file() {
        return Ok(dir);
    }

    // A previous run may have died partway through; start clean rather than
    // trusting whatever it left behind.
    if dir.exists() {
        let _ = fs::remove_dir_all(&dir);
    }
    fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;

    on_progress("unpacking the bundled harness runtime");
    unpack(&dir, on_progress)?;

    File::create(dir.join(MARKER))
        .map_err(|e| format!("could not finalise {}: {e}", dir.display()))?;
    Ok(dir)
}

/// The bundled Node interpreter, if the payload carried one.
pub fn node(runtime: &Path) -> Option<PathBuf> {
    let name = if cfg!(windows) { "node.exe" } else { "node" };
    let path = runtime.join(name);
    path.is_file().then_some(path)
}
