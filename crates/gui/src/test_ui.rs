//! Frame-only tests inspect shapes and interaction state without a renderer.
//! Their texture uploads must be explicitly discarded under egui 0.37.
//! Rendering tests use egui_kittest, which applies the uploads instead.

pub(crate) trait ContextExt {
    fn run_test_ui(&self, input: egui::RawInput, ui: impl FnMut(&mut egui::Ui))
    -> egui::FullOutput;
}

impl ContextExt for egui::Context {
    fn run_test_ui(
        &self,
        input: egui::RawInput,
        ui: impl FnMut(&mut egui::Ui),
    ) -> egui::FullOutput {
        let mut output = self.run_ui(input, ui);
        output.textures_delta.clear();
        output
    }
}
