//! Reproducible captures of the actual run and result views.
use super::*;
use egui_kittest::{Harness, kittest::Queryable as _};

#[derive(Clone, Copy, Debug)]
enum View {
    Preview,
    Results,
    Documents,
    Diagnostics,
    Pipeline,
    Stage,
    Trace,
    History,
    Replay,
    DebugRun,
}

fn app_for(view: View) -> FerruleApp {
    let project = serde_json::from_str(include_str!("../../../../examples/codegen/project.json"))
        .expect("self-authored codegen example");
    let mut app = FerruleApp {
        main_canvas: CanvasDocumentState::main(&project),
        project,
        input_path: "/demo/input.xml".into(),
        output_path: "/demo/output.xml".into(),
        ..Default::default()
    };
    match view {
        View::Preview => {
            app.preview_draft = Some(crate::preview::PreviewDraft::new(
                crate::preview::PreviewTarget::Primary,
                "input.xml".into(),
                "output.xml".into(),
            ));
            app.preview_draft.as_mut().unwrap().input_text =
                "<Orders><Order><Customer>Ada</Customer><Amount>30</Amount><Currency>EUR</Currency></Order></Orders>".into();
        }
        View::Results | View::Trace | View::History | View::Replay => {
            let report = crate::run_report::RunReport::from_payload_with_trace(
                cli::PayloadRunOutcome {
                    records_written: 1,
                    artifacts: vec![cli::PayloadArtifact {
                        target: "Primary".into(),
                        records_written: 1,
                        path: "output.xml".into(),
                        bytes: b"<Invoices><Invoice><Customer>Ada</Customer><Amount>30</Amount></Invoice></Invoices>".to_vec(),
                    }],
                },
                "input.xml".into(),
                std::time::Duration::from_millis(25),
                Default::default(),
            );
            let mut report = report;
            if !matches!(view, View::Results) {
                report.trace.events = vec![cli::TraceEvent::NodeValue {
                    node: *app.project.graph.nodes.keys().next().unwrap(),
                    positions: Vec::new(),
                    value: cli::TraceValue {
                        value_type: "string",
                        preview: "Ada".into(),
                        truncated: false,
                    },
                }];
            }
            app.run_report = Some(crate::run_report::RunReportView::new(report)
                .with_project_labels(&app.project));
            app.show_run_report = true;
        }
        View::Documents => {
            app.primary_document = Some(super::super::primary_document::PrimaryDocumentSession::new(
                mapping::PrimarySourcePlan::new(0),
                std::path::Path::new("/demo/project.json"),
            ));
            app.show_run_setup = true;
        }
        View::Diagnostics => app.diagnostics.error(
            "Preview failed",
            "The customer reference is missing. Open the source mapping and connect the required customer reference before running this mapping again.",
        ),
        View::Pipeline | View::Stage => {
            let unused = std::env::temp_dir().join(format!("capture-pipeline-{}.json", std::process::id()));
            let mut document = crate::pipeline_edit::PipelineEditorDocument::create(&unused).unwrap();
            document.path = "/demo/pipeline.json".into();
            document.pipeline.stages.push(mapping::PipelineStage {
                id: "Map orders".into(),
                mapping_path: Some("mapping.json".into()),
                project: app.project.clone(),
                source: mapping::PipelineInput::Host { name: "orders".into() },
                extra_sources: Vec::new(),
            });
            app.pipeline_editor = Some(super::super::pipeline_editor_ui::PipelineEditorUi::new(document));
            if matches!(view, View::Stage) {
                app.begin_pipeline_stage_canvas(0);
            }
        }
        View::DebugRun => {
            let (_, receiver) = std::sync::mpsc::channel();
            let (commands, _) = std::sync::mpsc::channel();
            app.pending_file_run = Some(super::super::run_ui::PendingFileRun::from_test_core(
                super::super::run_ui::FileRunCore {
                    receiver,
                    worker: None,
                    completed: None,
                    commands,
                    cancelled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                    started: std::time::Instant::now(),
                    debug: true,
                    phase: super::super::run_ui::FileRunPhase::PausedNode(Box::new(engine::PendingNodeValue {
                        node: 0,
                        value: engine::TraceValue { value_type: "string", preview: "Ada".into(), truncated: false },
                        positions: Vec::new(),
                        omitted_outer_positions: 0,
                        position_paths_truncated: false,
                        source: engine::DebugSourceContext { frames: Vec::new(), omitted_outer_frames: 0, primary_root_origin: None },
                    })),
                    extraction: None,
                    shutdown: Default::default(),
                },
            ));
        }
    }
    app
}

#[test]
#[ignore = "renders run-view screenshots; set FERRULE_RUN_VIEW_SHOTS to a new directory"]
fn capture_run_views() {
    let output = std::path::PathBuf::from(
        std::env::var_os("FERRULE_RUN_VIEW_SHOTS").expect("FERRULE_RUN_VIEW_SHOTS is required"),
    );
    std::fs::create_dir_all(&output).unwrap();
    for (width, height) in [(1600.0, 1000.0), (1280.0, 800.0)] {
        for view in [
            View::Preview,
            View::Results,
            View::Documents,
            View::Diagnostics,
            View::Pipeline,
            View::Stage,
            View::Trace,
            View::History,
            View::Replay,
            View::DebugRun,
        ] {
            if let Ok(filter) = std::env::var("FERRULE_RUN_VIEW_FILTER")
                && filter != format!("{view:?}")
            {
                continue;
            }
            let mut initialized = false;
            let mut harness = Harness::builder()
                .with_accessibility_check(true)
                .with_size((width, height))
                .with_max_steps(120)
                .build_ui_state(
                    move |ui, app: &mut FerruleApp| {
                        if !initialized {
                            crate::fonts::install(ui.ctx());
                            crate::icons::install(ui.ctx());
                            app.theme.apply(ui.ctx());
                            ui.ctx().all_styles_mut(|style| style.animation_time = 0.0);
                            initialized = true;
                        }
                        if matches!(view, View::Documents) {
                            app.show_main_canvas(ui, true);
                            app.show_run_settings(ui.ctx(), true);
                        } else {
                            if matches!(view, View::Diagnostics) {
                                egui::Panel::bottom("capture_diagnostics")
                                    .default_size(160.0)
                                    .show(ui, |ui| {
                                        app.diagnostics.show(ui);
                                    });
                            }
                            if !matches!(view, View::Stage) {
                                app.show_main_canvas(ui, true);
                            }
                            match view {
                                View::Preview => app.show_preview_setup(ui.ctx()),
                                View::Results | View::Trace | View::History | View::Replay => {
                                    crate::run_report::show(
                                        ui.ctx(),
                                        &mut app.show_run_report,
                                        app.run_report.as_mut().unwrap(),
                                    )
                                }
                                View::Diagnostics => {}
                                View::Pipeline => app.show_pipeline_editor(ui.ctx()),
                                View::Stage => app.show_pipeline_stage_canvas(ui),
                                View::DebugRun => app.show_file_run_progress(ui.ctx()),
                                View::Documents => unreachable!(),
                            }
                        }
                    },
                    app_for(view),
                );
            harness.run();
            let page = match view {
                View::Trace => Some("Trace"),
                View::History => Some("History"),
                View::Replay => Some("Replay"),
                _ => None,
            };
            if let Some(page) = page {
                harness.get_by_label(page).click();
                harness.run();
            }
            let image = harness.render().expect("run-view render");
            let file =
                std::fs::File::create_new(output.join(format!("{view:?}-{width}x{height}.png")))
                    .unwrap();
            image
                .write_to(&mut std::io::BufWriter::new(file), image::ImageFormat::Png)
                .unwrap();
        }
    }
}
