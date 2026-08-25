//! DeepSeek Harness Desktop — a native shell (Tauri + Rust) around the
//! `dsh web` browser GUI.
//!
//! The shell starts the real harness server, discovers its loopback URL, and
//! hosts it in a platform webview. No harness logic is reimplemented here.

mod backend;

use std::sync::Mutex;

use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

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

/// Create the main window, pointed at the harness URL the server announced.
fn create_main_window(app: &tauri::App, url: &str) -> tauri::Result<()> {
    let target = url
        .parse::<url::Url>()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    WebviewWindowBuilder::new(app, "main", WebviewUrl::External(target))
        .title("DeepSeek Harness Desktop")
        .inner_size(1280.0, 800.0)
        .min_inner_size(900.0, 600.0)
        .center()
        .build()?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            app.manage(BackendState(Mutex::new(None)));

            let (backend, url) = match backend::spawn_backend(&backend::search_roots(app)) {
                Ok(ready) => ready,
                Err(message) => {
                    eprintln!("dsh-desktop: {message}");
                    app.handle().exit(1);
                    return Ok(());
                }
            };

            app.state::<BackendState>().set(backend);

            if let Err(e) = create_main_window(app, &url) {
                eprintln!("dsh-desktop: failed to create the main window: {e}");
                app.state::<BackendState>().shutdown();
                app.handle().exit(1);
            }

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
