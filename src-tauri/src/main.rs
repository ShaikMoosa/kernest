// Hide the console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod anchors;
mod commands;
mod export;
mod features;
mod fontinfo;
mod glyphs;
mod import;
mod launch;
mod layers;
mod spacing;
mod tools;

fn main() {
    let launch_paths = launch::paths_from_args(std::env::args());
    tauri::Builder::default()
        // Must be first: a second launch hands its paths to the running window.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            launch::on_second_instance(app, argv)
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(commands::AppState::default())
        .manage(launch::LaunchPaths(std::sync::Mutex::new(launch_paths)))
        .invoke_handler(tauri::generate_handler![
            commands::glyph_sets,
            launch::take_launch_paths,
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
            fontinfo::set_hinting,
            fontinfo::set_grid,
            fontinfo::set_master_guides,
            fontinfo::set_palettes,
            fontinfo::set_axes,
            fontinfo::add_master,
            fontinfo::update_master,
            fontinfo::remove_master,
            fontinfo::set_instances,
            fontinfo::set_font_names,
            fontinfo::set_notes,
            fontinfo::compatibility,
            features::features,
            features::set_features,
            features::set_automatic_features,
            features::shape_text,
            export::export_font,
            import::import_font,
            glyphs::add_glyphs,
            glyphs::remove_glyphs,
            glyphs::set_glyph_export,
            commands::path_operation,
            commands::set_metrics_key,
            commands::update_metrics,
            layers::glyph_layers,
            layers::add_brace_layer,
            layers::add_bracket_layers,
            layers::update_layer_position,
            layers::remove_layer,
            layers::add_color_layer,
            layers::set_color_paint,
            layers::move_color_layer,
            layers::set_smart_axes,
            layers::add_smart_pole,
            commands::set_component_smart,
            anchors::add_anchor,
            anchors::move_anchors,
            anchors::rename_anchor,
            anchors::remove_anchors,
            anchors::set_default_anchors,
            anchors::set_component_align,
            anchors::create_composites,
            tools::set_guides,
            tools::set_guide_scope,
            tools::transform,
            tools::background_edit,
            tools::update_background,
            tools::measure,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Kernest");
}
