use super::*;
use crate::test_ui::ContextExt as _;

fn output(name: &str) -> RunOutput {
    RunOutput {
        name: name.into(),
        records_written: 1,
        path: PathBuf::from(format!("{name}.json")),
        in_memory: true,
        preview: Some(OutputPreview::from_bytes(b"{}")),
    }
}

fn report(outputs: Vec<RunOutput>) -> RunReportView {
    RunReportView::new(RunReport {
        kind: RunReportKind::Preview,
        duration: Duration::from_millis(1),
        records_written: 1,
        input_path: PathBuf::from("input.json"),
        outputs,
        trace: TraceReport::default(),
    })
}

fn visible_text(shape: &egui::Shape, text: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(value) => text.push(value.galley.job.text.clone()),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                visible_text(shape, text);
            }
        }
        _ => {}
    }
}

fn labels(view: &mut RunReportView) -> Vec<String> {
    let context = egui::Context::default();
    crate::icons::install(&context);
    let result = context.run_test_ui(Default::default(), |ui| show_report(ui, view));
    let mut text = Vec::new();
    for shape in result.shapes {
        visible_text(&shape.shape, &mut text);
    }
    text
}

#[test]
fn single_output_has_one_name_and_one_record_count_without_a_redundant_tab() {
    let text = labels(&mut report(vec![output("Primary")]));
    assert_eq!(text.iter().filter(|text| *text == "Primary").count(), 1);
    assert_eq!(text.iter().filter(|text| *text == "1 record").count(), 1);
    assert!(
        !text
            .iter()
            .any(|text| text == "1 output" || text == "0 trace events")
    );
}

#[test]
fn multiple_outputs_keep_the_selector_and_the_selected_preview() {
    let mut view = report(vec![output("Primary"), output("Audit")]);
    view.selected_output = 1;
    let text = labels(&mut view);
    assert!(text.iter().any(|text| text == "2 outputs"));
    assert!(text.iter().any(|text| text == "Primary"));
    assert!(text.iter().any(|text| text == "Audit.json"));
    assert_eq!(view.selected_output, 1);
}
