//! DeepSeek Harness Desktop — a native shell (Tauri + Rust) around the
//! `dsh web` browser GUI.
//!
//! The shell starts the real harness server, discovers its loopback URL, and
//! hosts it in a platform webview. No harness logic is reimplemented here.

mod backend;
mod logging;
#[cfg(feature = "portable")]
mod portable;

use std::{sync::Mutex, thread};

use tauri::{
    menu::{Menu, MenuItem, Submenu},
    AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

use backend::Backend;

/// Managed state holding the live server process for shutdown.
struct BackendState(Mutex<Option<Backend>>);

impl BackendState {
    fn set(&self, backend: Backend) {
        if let Ok(mut guard) = self.0.lock() {
            *guard = Some(backend);
        }
    }

    fn shutdown(&self) {
        if let Ok(mut guard) = self.0.lock() {
            if let Some(mut backend) = guard.take() {
                backend.shutdown();
            }
        }
    }
}

/// Open a path with whatever the desktop uses for it.
fn open_externally(path: &std::path::Path) {
    let mut command = if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else {
        std::process::Command::new("xdg-open")
    };
    let _ = command.arg(path).spawn();
}

/// A menu offering the two things needed to diagnose a problem: the log, and
/// the inspector.
///
/// Neither should require relaunching the app from a terminal. Asking a user
/// to do that is both awkward and misleading — it changes the conditions of
/// the run, and it hides the information until someone knows the trick.
fn build_menu(app: &tauri::App) -> tauri::Result<()> {
    let logs = MenuItem::with_id(app, "open-log", "Open Log File", true, None::<&str>)?;
    let folder = MenuItem::with_id(app, "open-log-dir", "Show Log Folder", true, None::<&str>)?;
    let inspector = MenuItem::with_id(
        app,
        "devtools",
        "Developer Tools",
        true,
        Some("CmdOrCtrl+Shift+I"),
    )?;
    let reload = MenuItem::with_id(app, "reload", "Reload", true, Some("CmdOrCtrl+R"))?;

    let submenu = Submenu::with_items(
        app,
        "Troubleshooting",
        true,
        &[&reload, &inspector, &logs, &folder],
    )?;

    let menu = Menu::default(app.handle())?;
    menu.append(&submenu)?;
    app.set_menu(menu)?;

    app.on_menu_event(move |app, event| match event.id().as_ref() {
        "open-log" => {
            if let Some(path) = logging::path() {
                open_externally(path);
            }
        }
        "open-log-dir" => {
            if let Some(dir) = logging::path().and_then(|p| p.parent()) {
                open_externally(dir);
            }
        }
        "devtools" => {
            if let Some(window) = app.get_webview_window("main") {
                window.open_devtools();
            }
        }
        "reload" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.eval("window.location.reload()");
            }
        }
        _ => {}
    });

    Ok(())
}

/// The window shown while the backend starts.
///
/// It exists so the app is never a blank screen. A locally installed backend
/// is ready in a second or two, but the portable build fetches the harness on
/// first run, which takes minutes — without something on screen that is
/// indistinguishable from a crash.
fn create_splash_window(app: &tauri::App) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, "splash", WebviewUrl::App("index.html".into()))
        .title("DeepSeek Harness Desktop")
        .inner_size(560.0, 360.0)
        .resizable(false)
        .center()
        .build()
}

/// Create the main window, pointed at the harness URL the server announced.
fn create_main_window(app: &AppHandle, url: &str) -> tauri::Result<WebviewWindow> {
    let target = url
        .parse::<url::Url>()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    WebviewWindowBuilder::new(app, "main", WebviewUrl::External(target))
        .title("DeepSeek Harness Desktop")
        .inner_size(1280.0, 800.0)
        .min_inner_size(900.0, 600.0)
        .center()
        .build()
}

/// Stream a line of backend output into the splash window.
fn report_progress(splash: &WebviewWindow, line: &str) {
    if let Ok(payload) = serde_json::to_string(line) {
        let _ = splash.eval(format!(
            "window.dshProgress && window.dshProgress({payload})"
        ));
    }
}

/// Render a startup failure inside the splash window.
///
/// A GUI app has no console, so `eprintln!` alone means the user sees the
/// window vanish with no explanation. The message is passed through
/// `serde_json` so quotes and newlines in a backend error cannot break out of
/// the JavaScript string.
fn report_failure(splash: &WebviewWindow, message: &str) {
    logging::line(&format!("dsh-desktop: {message}"));
    let payload = serde_json::to_string(message)
        .unwrap_or_else(|_| "\"the backend failed to start\"".to_string());
    let _ = splash.eval(format!(
        "window.dshShowError && window.dshShowError({payload})"
    ));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            app.manage(BackendState(Mutex::new(None)));

            let log = app
                .path()
                .app_log_dir()
                .unwrap_or_else(|_| std::env::temp_dir())
                .join("dsh-desktop.log");
            logging::init(log);
            logging::line(&format!(
                "dsh-desktop {} starting",
                env!("CARGO_PKG_VERSION")
            ));

            if let Err(e) = build_menu(app) {
                logging::line(&format!("dsh-desktop: could not build the menu: {e}"));
            }

            let roots = backend::search_roots(app);
            let splash = create_splash_window(app)?;
            let handle = app.handle().clone();

            // Starting the backend must not block `setup`: nothing renders
            // until the setup hook returns, so waiting here would hide the
            // splash window we just created.
            thread::spawn(move || {
                let progress = splash.clone();
                match backend::spawn_backend(&roots, move |line| report_progress(&progress, line)) {
                    Ok((backend, url)) => {
                        handle.state::<BackendState>().set(backend);
                        match create_main_window(&handle, &url) {
                            Ok(_) => {
                                let _ = splash.close();
                            }
                            Err(e) => {
                                report_failure(&splash, &format!("could not open the window: {e}"));
                                handle.state::<BackendState>().shutdown();
                            }
                        }
                    }
                    // The splash window stays open holding the message, so
                    // the user can read it and close the app themselves.
                    Err(message) => report_failure(&splash, &message),
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the DeepSeek Harness Desktop app");

    app.run(|app_handle, event| {
        if let RunEvent::Exit = event {
            if let Some(state) = app_handle.try_state::<BackendState>() {
                state.shutdown();
            }
        }
    });
}
