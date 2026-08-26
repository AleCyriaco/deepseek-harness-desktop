//! A log file, written for every run.
//!
//! The shell is a GUI app with no console, so anything it or the backend
//! prints is invisible by default. Diagnosing a problem then means asking the
//! user to relaunch the app from a terminal, which is a poor thing to ask and
//! changes the conditions of the run. Everything goes to a file instead, and
//! the window's menu opens it.

use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

static LOG: OnceLock<Mutex<Option<File>>> = OnceLock::new();
static PATH: OnceLock<PathBuf> = OnceLock::new();

/// Start a fresh log at `path`.
///
/// The previous run's log is replaced rather than appended to: a stale tail
/// from an earlier session is worse than no history when someone is trying to
/// work out what just happened.
pub fn init(path: PathBuf) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let file = File::create(&path).ok();
    let _ = PATH.set(path);
    let _ = LOG.set(Mutex::new(file));
}

/// Where the log lives, once `init` has run.
pub fn path() -> Option<&'static PathBuf> {
    PATH.get()
}

/// Append one line. Never fails the caller — a broken log must not take the
/// app down with it.
pub fn line(text: &str) {
    #[cfg(debug_assertions)]
    eprintln!("{text}");

    if let Some(lock) = LOG.get() {
        if let Ok(mut guard) = lock.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = writeln!(file, "{text}");
                let _ = file.flush();
            }
        }
    }
}
