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
mod preferences;
mod preview;
mod quality;
mod script;
mod settings;
mod spacing;
mod tools;
mod updates;

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
        .manage(settings::SettingsState::default())
        .manage(script::ScriptState::default())
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
            commands::fix_interpolation,
            commands::doc_status,
            commands::glyph_layer,
            commands::update_layer,
            commands::set_metric,
            commands::set_glyph_hints,
            preview::pixel_preview,
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
            fontinfo::set_alignment_zones,
            fontinfo::set_stems,
            fontinfo::set_control_instructions,
            fontinfo::set_ps_hinting,
            fontinfo::set_grid,
            fontinfo::set_master_guides,
            fontinfo::set_vertical_metrics,
            fontinfo::set_use_typo_metrics,
            fontinfo::set_custom_parameters,
            fontinfo::set_palettes,
            fontinfo::set_master_palettes,
            fontinfo::set_axes,
            fontinfo::add_master,
            fontinfo::preview_foreign_masters,
            fontinfo::add_master_from,
            fontinfo::compare_fonts,
            fontinfo::generate_instances,
            fontinfo::update_master,
            fontinfo::remove_master,
            fontinfo::set_instances,
            fontinfo::set_variable_fonts,
            fontinfo::set_export_presets,
            fontinfo::set_glyph_order,
            fontinfo::set_font_names,
            fontinfo::set_notes,
            fontinfo::compatibility,
            features::features,
            features::set_features,
            features::set_automatic_features,
            features::preview_feature_import,
            features::import_features,
            features::shape_text,
            export::export_font,
            export::export_preset,
            quality::check_fonts,
            quality::save_quality_report,
            export::export_ufo,
            import::import_font,
            import::save_glyphs,
            import::import_svg_text,
            import::import_svg_files,
            import::read_metrics_source,
            import::import_metrics,
            glyphs::add_glyphs,
            glyphs::language_coverage,
            glyphs::missing_glyphs,
            glyphs::add_missing,
            glyphs::remove_glyphs,
            glyphs::set_glyph_export,
            glyphs::set_glyph_tags,
            glyphs::set_glyph_unicodes,
            glyphs::set_production_name,
            glyphs::set_filters,
            glyphs::evaluate_filter,
            glyphs::rename_glyphs,
            commands::path_operation,
            commands::path_edit,
            commands::add_pixel_glyph,
            commands::set_pixels,
            commands::add_path_part,
            commands::set_path_part,
            commands::decompose_parts,
            commands::set_path_stroke,
            commands::set_pen_width,
            commands::expand_strokes,
            commands::set_path_locked,
            commands::set_glyph_locked,
            commands::run_filter,
            commands::filter_preview,
            commands::set_metrics_key,
            commands::update_metrics,
            commands::batch_path_operation,
            commands::update_metrics_for,
            commands::copy_glyphs,
            commands::paste_special,
            layers::glyph_layers,
            layers::add_brace_layer,
            layers::add_bracket_layers,
            layers::update_layer_position,
            layers::remove_layer,
            layers::add_backup_layer,
            layers::use_backup_layer,
            layers::add_color_layer,
            layers::set_color_paint,
            layers::set_master_paint,
            layers::master_color_paint,
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
            settings::get_settings,
            settings::set_appearance,
            settings::set_onboarding,
            settings::record_milestone,
            settings::set_update_check,
            updates::check_updates,
            settings::set_plugin_enabled,
            settings::set_shortcut,
            settings::reset_shortcuts,
            settings::set_python_path,
            script::list_scripts,
            script::list_plugins,
            script::run_script,
            script::cancel_script,
            script::run_filter_plugin,
            script::reporter_draw,
            script::open_scripts_folder,
            script::open_plugins_folder,
            script::python_info,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Kernest");
}
