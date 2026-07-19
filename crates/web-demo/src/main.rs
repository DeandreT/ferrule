//! Browser playground for ferrule: a small eframe app around the real
//! `mapping` + `engine` crates, compiled to WebAssembly for the website
//! (and runnable natively for local testing). The browser editor supports
//! project JSON, XML/JSON/CSV/XBRL instance text, validation, and live execution.

mod app;

use app::DemoApp;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    eframe::run_native(
        "ferrule playground",
        eframe::NativeOptions::default(),
        Box::new(|_cc| Ok(Box::new(DemoApp::new()))),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    wasm_bindgen_futures::spawn_local(async {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let Some(element) = document.get_element_by_id("demo_canvas") else {
            return;
        };
        let Ok(canvas) = element.dyn_into::<web_sys::HtmlCanvasElement>() else {
            return;
        };
        let started = eframe::WebRunner::new()
            .start(
                canvas.clone(),
                eframe::WebOptions::default(),
                Box::new(|_cc| Ok(Box::new(DemoApp::new()))),
            )
            .await;
        if started.is_ok() {
            let _ = canvas.set_attribute("data-ferrule-ready", "true");
        }
    });
}
