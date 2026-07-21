mod app;
mod appearance;
mod appearance_editor;
mod canvas;
mod canvas_keyboard;
mod canvas_layout;
mod diagnostics;
mod document;
mod extra_sources;
mod graph_viewer;
mod icons;
mod layout_store;
mod new_mapping;
mod path_picker;
mod preferences;
mod run_report;
mod schema_tree;
mod scope_editor;
mod theme;
mod value_editor;
mod wire_colors;
mod workspace_layout;
mod x12_tooltips;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "ferrule",
        native_options,
        Box::new(|creation| {
            icons::install(&creation.egui_ctx);
            Ok(Box::new(app::FerruleApp::from_storage(creation.storage)))
        }),
    )
}
