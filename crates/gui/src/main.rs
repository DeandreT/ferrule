mod app;
mod canvas;
mod canvas_keyboard;
mod diagnostics;
mod document;
mod extra_sources;
mod graph_viewer;
mod layout_store;
mod new_mapping;
mod path_picker;
mod schema_tree;
mod scope_editor;
mod value_editor;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "ferrule",
        native_options,
        Box::new(|_cx| Ok(Box::new(app::FerruleApp::default()))),
    )
}
