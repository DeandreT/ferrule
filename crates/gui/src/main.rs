mod app;
mod appearance;
mod appearance_editor;
mod auto_connect;
mod canvas;
#[path = "canvas/endpoints.rs"]
mod canvas_endpoints;
#[path = "canvas/keyboard.rs"]
mod canvas_keyboard;
#[path = "canvas/layout.rs"]
mod canvas_layout;
#[path = "canvas/minimap.rs"]
mod canvas_minimap;
#[path = "canvas/search.rs"]
mod canvas_search;
#[path = "canvas/tools.rs"]
mod canvas_tools;
mod diagnostics;
mod document;
mod document_file;
mod extra_sources;
mod extra_targets;
mod filter_map_editor;
mod fonts;
mod graph_viewer;
mod icons;
mod layout_store;
mod new_mapping;
mod path_picker;
mod pipeline_edit;
mod pipeline_run;
mod preferences;
mod preview;
mod primary_root_authoring;
mod project_state;
mod run_report;
mod schema_scalar;
mod schema_tree;
mod scope_editor;
mod target_xml_type;
#[cfg(test)]
mod test_ui;
mod theme;
mod value_editor;
#[cfg(test)]
#[path = "canvas/vendor_hook_tests.rs"]
mod vendor_hook_tests;
mod wire_colors;
mod workspace_layout;
mod x12_tooltips;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };
    let mut saved_file_owner = app::SavedFileRunTeardownService::new();
    let saved_file_port = saved_file_owner.port();
    let runner_result = eframe::run_native(
        "ferrule",
        native_options,
        Box::new(move |creation| {
            fonts::install(&creation.egui_ctx);
            icons::install(&creation.egui_ctx);
            Ok(Box::new(
                app::FerruleApp::from_storage_with_saved_file_owner(
                    creation.storage,
                    saved_file_port,
                ),
            ))
        }),
    );
    let (runner_result, terminal) = saved_file_owner.finish_after_runner(runner_result);
    if let Some(terminal) = terminal {
        terminal.report();
    }
    runner_result
}
