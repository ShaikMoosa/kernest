//! The separate preview window (SMO-36): Window › Preview Window opens a second
//! window that mirrors the main window's current tab, for example on another
//! display. The main window sends it the text to draw as events.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, Window, WindowEvent};

pub const PREVIEW_LABEL: &str = "preview";

/// Opens the preview window, or brings it forward when it is already open.
/// Async on purpose: building a window from a synchronous command deadlocks on Windows.
#[tauri::command]
pub async fn open_preview_window(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(PREVIEW_LABEL) {
        let _ = w.unminimize();
        w.show().map_err(|e| e.to_string())?;
        w.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }
    WebviewWindowBuilder::new(
        &app,
        PREVIEW_LABEL,
        WebviewUrl::App("index.html?window=preview".into()),
    )
    .title("Kernest Preview")
    .inner_size(960.0, 280.0)
    .min_inner_size(360.0, 140.0)
    .build()
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Closing the main window closes the preview window too, so Kernest quits.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if window.label() == "main" && matches!(event, WindowEvent::Destroyed) {
        if let Some(p) = window.app_handle().get_webview_window(PREVIEW_LABEL) {
            let _ = p.close();
        }
    }
}
