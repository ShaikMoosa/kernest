// Hide the console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::glyph_sets,
            commands::new_font,
            commands::open_sample,
            commands::open_font,
            commands::save_font,
            commands::close_font,
            commands::font_view,
            commands::doc_status,
            commands::glyph_layer,
            commands::update_layer,
            commands::set_metric,
            commands::undo,
            commands::redo,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Kernest");
}
