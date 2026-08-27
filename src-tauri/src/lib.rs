//! DeepSeek Harness Desktop — a native shell (Tauri + Rust) around the
//! `dsh web` browser GUI.
//!
//! The shell starts the real harness server, discovers its loopback URL, and
//! hosts it in a platform webview. No harness logic is reimplemented here.

mod backend;
mod logging;
#[cfg(feature = "portable")]
mod portable;
mod usage;

use std::{sync::Mutex, thread};

use tauri::{
    menu::{Menu, MenuItem, Submenu},
    webview::WebviewBuilder,
    window::WindowBuilder,
    AppHandle, LogicalPosition, LogicalSize, Manager, RunEvent, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window, WindowEvent,
};

/// How wide the status panel is, in logical pixels.
const PANEL_WIDTH: f64 = 320.0;

/// How the status panel is displayed.
///
/// Three states because they answer different needs: `Pinned` gives the panel
/// its own column and shrinks the harness to fit, `Floating` overlays it for a
/// glance without disturbing the layout, and `Hidden` gets it out of the way.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum PanelMode {
    #[default]
    Pinned,
    Floating,
    Hidden,
}

impl PanelMode {
    fn from_id(id: &str) -> Option<Self> {
        match id {
            "panel-pin" => Some(Self::Pinned),
            "panel-float" => Some(Self::Floating),
            "panel-hide" => Some(Self::Hidden),
            _ => None,
        }
    }
}

/// The current panel mode, so a window resize can re-apply the same layout.
struct PanelState(Mutex<PanelMode>);

/// Lay the two webviews out for the given mode.
///
/// Called on every resize as well as on every mode change: the webviews are
/// native views positioned by hand, so nothing repositions them for us.
fn layout(window: &Window, mode: PanelMode) {
    let Ok(size) = window.inner_size() else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let width = size.width as f64 / scale;
    let height = size.height as f64 / scale;

    // Never let the panel crowd out the harness on a narrow window.
    let panel_width = PANEL_WIDTH.min(width * 0.45);
    let harness_width = match mode {
        PanelMode::Pinned => (width - panel_width).max(1.0),
        _ => width,
    };

    if let Some(harness) = window.get_webview("harness") {
        let _ = harness.set_position(LogicalPosition::new(0.0, 0.0));
        let _ = harness.set_size(LogicalSize::new(harness_width, height));
    }

    if let Some(panel) = window.get_webview(usage::PANEL_LABEL) {
        match mode {
            PanelMode::Hidden => {
                let _ = panel.hide();
            }
            _ => {
                let _ = panel.set_position(LogicalPosition::new(width - panel_width, 0.0));
                let _ = panel.set_size(LogicalSize::new(panel_width, height));
                let _ = panel.show();
            }
        }
    }
}

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
    command.arg(path);

    // Same reason as the backend: `cmd.exe` is a console program, and letting
    // it allocate a window would flash a black box every time someone opens
    // the log.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }

    let _ = command.spawn();
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

    let pin = MenuItem::with_id(
        app,
        "panel-pin",
        "Pin Status Panel",
        true,
        Some("CmdOrCtrl+1"),
    )?;
    let float = MenuItem::with_id(
        app,
        "panel-float",
        "Float Status Panel",
        true,
        Some("CmdOrCtrl+2"),
    )?;
    let hide = MenuItem::with_id(
        app,
        "panel-hide",
        "Hide Status Panel",
        true,
        Some("CmdOrCtrl+0"),
    )?;
    let view = Submenu::with_items(app, "View", true, &[&pin, &float, &hide])?;

    let submenu = Submenu::with_items(
        app,
        "Troubleshooting",
        true,
        &[&reload, &inspector, &logs, &folder],
    )?;

    let menu = Menu::default(app.handle())?;
    menu.append(&view)?;
    menu.append(&submenu)?;
    app.set_menu(menu)?;

    app.on_menu_event(move |app, event| match event.id().as_ref() {
        id if PanelMode::from_id(id).is_some() => {
            let mode = PanelMode::from_id(id).expect("checked by the guard");
            if let Some(state) = app.try_state::<PanelState>() {
                if let Ok(mut current) = state.0.lock() {
                    *current = mode;
                }
            }
            if let Some(window) = app.get_window("main") {
                layout(&window, mode);
            }
        }
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
            if let Some(window) = app.get_window("main") {
                if let Some(harness) = window.get_webview("harness") {
                    harness.open_devtools();
                }
            }
        }
        "reload" => {
            if let Some(window) = app.get_window("main") {
                if let Some(harness) = window.get_webview("harness") {
                    let _ = harness.eval("window.location.reload()");
                }
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

/// Create the main window: the harness on the left, the status panel on the
/// right, as two native webviews.
///
/// Two webviews rather than one, because the panel must not be injected into
/// the harness's page. Injecting would mean our markup living inside a UI we
/// do not control, breaking whenever it changes — and this shell's whole point
/// is that it leaves the harness alone.
fn create_main_window(app: &AppHandle, url: &str, mode: PanelMode) -> tauri::Result<Window> {
    let target = url
        .parse::<url::Url>()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    let window = WindowBuilder::new(app, "main")
        .title("DeepSeek Harness Desktop")
        .inner_size(1280.0, 800.0)
        .min_inner_size(900.0, 600.0)
        .center()
        .build()?;

    window.add_child(
        WebviewBuilder::new("harness", WebviewUrl::External(target)),
        LogicalPosition::new(0.0, 0.0),
        LogicalSize::new(1280.0, 800.0),
    )?;

    window.add_child(
        WebviewBuilder::new(usage::PANEL_LABEL, WebviewUrl::App("panel.html".into())),
        LogicalPosition::new(960.0, 0.0),
        LogicalSize::new(PANEL_WIDTH, 800.0),
    )?;

    layout(&window, mode);
    Ok(window)
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
            app.manage(PanelState(Mutex::new(PanelMode::default())));
            backend::install_signal_handlers();

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
                        let mode = handle
                            .try_state::<PanelState>()
                            .and_then(|s| s.0.lock().ok().map(|m| *m))
                            .unwrap_or_default();
                        match create_main_window(&handle, &url, mode) {
                            Ok(window) => {
                                // The webviews are positioned by hand, so
                                // nothing else keeps them in step with the
                                // window as it is resized.
                                let resized = window.clone();
                                window.on_window_event(move |event| {
                                    if let WindowEvent::Resized(_) = event {
                                        let mode = resized
                                            .try_state::<PanelState>()
                                            .and_then(|s| s.0.lock().ok().map(|m| *m))
                                            .unwrap_or_default();
                                        layout(&resized, mode);
                                    }
                                });
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
        .invoke_handler(tauri::generate_handler![usage::usage_snapshot])
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
