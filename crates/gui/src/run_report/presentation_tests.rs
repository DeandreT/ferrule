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

#[test]
fn report_labels_are_snapshots_and_keep_node_ids_for_disambiguation() {
    let mut project = crate::new_mapping::blank_project();
    project.graph.nodes.insert(
        7,
        mapping::Node::Position {
            collection: Vec::new(),
        },
    );
    let view = report(vec![output("Primary")]).with_project_labels(&project);
    assert_eq!(
        view.node_label(NodeHistoryKey::Graph(7)),
        "Position (Node 7)"
    );
    project.graph.nodes.remove(&7);
    assert_eq!(
        view.node_label(NodeHistoryKey::Graph(7)),
        "Position (Node 7)"
    );
    assert_eq!(view.node_label(NodeHistoryKey::Graph(8)), "Node 8");
}

#[test]
fn large_event_counts_have_readable_groups() {
    assert_eq!(format_count(0), "0");
    assert_eq!(format_count(999), "999");
    assert_eq!(format_count(50_000), "50,000");
    assert_eq!(format_count(1_000_001), "1,000,001");
}

#[test]
fn pipeline_node_labels_remain_owned_by_their_stage() {
    let mut first = crate::new_mapping::blank_project();
    first.graph.nodes.insert(
        7,
        mapping::Node::Position {
            collection: Vec::new(),
        },
    );
    let mut second = crate::new_mapping::blank_project();
    second.graph.nodes.insert(
        7,
        mapping::Node::Const {
            value: ir::Value::String("other stage".into()),
        },
    );
    let pipeline = mapping::Pipeline {
        main_mapping_path: None,
        stages: [first, second]
            .into_iter()
            .enumerate()
            .map(|(index, project)| mapping::PipelineStage {
                id: format!("stage{index}"),
                mapping_path: None,
                project,
                source: mapping::PipelineInput::Host {
                    name: "input".into(),
                },
                extra_sources: Vec::new(),
            })
            .collect(),
    };
    let mut view = report(vec![output("Primary")]);
    view.report.trace.stages = vec!["stage0".into(), "stage1".into()];
    let mut view = view.with_pipeline_labels(&pipeline);
    view.history_stage = Some(0);
    let first = view.node_label(NodeHistoryKey::Graph(7));
    view.history_stage = Some(1);
    let second = view.node_label(NodeHistoryKey::Graph(7));
    assert!(first.contains("Position"));
    assert!(second.contains("other stage"));
    assert_ne!(first, second);
}

#[test]
fn clicking_a_trace_row_requests_replay_without_a_separate_button() {
    use egui_kittest::{Harness, kittest::Queryable as _};
    let mut harness = Harness::builder().build_ui_state(
        |ui, clicked: &mut bool| {
            *clicked |= replayable_trace_row(ui, 0, "Customer · Node 7 · Ada".into());
        },
        false,
    );
    harness.run();
    harness.get_by_label("Customer · Node 7 · Ada").click();
    harness.run();
    assert!(*harness.state());
}
