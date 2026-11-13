// Hide the console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod export;
mod fontinfo;
mod import;
mod spacing;

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
            spacing::text_layout,
            spacing::set_glyph_kerning,
            spacing::kerning_list,
            spacing::set_kerning_pair,
            spacing::set_glyph_groups,
            fontinfo::font_info,
            fontinfo::set_font_info,
            fontinfo::set_axes,
            fontinfo::add_master,
            fontinfo::update_master,
            fontinfo::remove_master,
            fontinfo::set_instances,
            fontinfo::compatibility,
            export::export_font,
            import::import_font,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Kernest");
}
