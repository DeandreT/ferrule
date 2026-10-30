//! Opt-in, deterministic renders of the actual dialog widgets.
use super::*;
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};

static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn flat_schema() -> ir::SchemaNode {
    ir::SchemaNode::group(
        "Order",
        vec![
            ir::SchemaNode::scalar("Name", ir::ScalarType::String),
            ir::SchemaNode::scalar("Quantity", ir::ScalarType::Int),
        ],
    )
}

fn app_for(view: &str) -> FerruleApp {
    let mut app = FerruleApp::default();
    app.project.source = flat_schema();
    app.project.target = flat_schema();
    app.main_canvas = CanvasDocumentState::main(&app.project);
    match view {
        "wizard-empty" => app.begin_new_mapping(),
        "wizard-schema" | "wizard-csv" | "wizard-csv-long" | "wizard-fixed-width"
        | "wizard-xlsx" | "wizard-csv-source" => {
            app.begin_new_mapping();
            for side in [SchemaSide::Source, SchemaSide::Target] {
                let boundary =
                    MappingBoundary::Schema(Box::new(crate::new_mapping::ImportedSchema {
                        path: PathBuf::from(
                            "/demo/order-processing/schemas/customer-orders.schema.json",
                        ),
                        schema: flat_schema(),
                    }));
                let setup = app.new_mapping_setup.as_mut().unwrap();
                match side {
                    SchemaSide::Source => setup.source = Some(boundary),
                    SchemaSide::Target => setup.target = Some(boundary),
                }
                if view == "wizard-fixed-width" {
                    app.configure_mapping_fixed_width(side);
                }
                if view == "wizard-xlsx" {
                    app.configure_mapping_xlsx(side);
                }
            }
            if view == "wizard-csv" || view == "wizard-csv-long" {
                let mut csv = CsvBoundaryDraft::target();
                csv.columns = vec![
                    CsvColumnDraft {
                        name: "Name".into(),
                        ty: ir::ScalarType::String,
                    },
                    CsvColumnDraft {
                        name: "Quantity".into(),
                        ty: ir::ScalarType::Int,
                    },
                ];
                if view == "wizard-csv-long" {
                    csv.path = "/demo/output.csv".into();
                    csv.columns = (1..=40)
                        .map(|index| CsvColumnDraft {
                            name: format!("Column{index}"),
                            ty: ir::ScalarType::String,
                        })
                        .collect();
                }
                app.new_mapping_setup.as_mut().unwrap().target = Some(MappingBoundary::Csv(csv));
            }
            if view == "wizard-csv-source" {
                let path = std::env::temp_dir().join(format!(
                    "ferrule-dialog-{}-{}.csv",
                    std::process::id(),
                    NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ));
                std::fs::write(&path, "Name,Quantity\nSample order,2\nSecond order,4\n").unwrap();
                let mut csv = CsvBoundaryDraft::source(path.clone()).unwrap();
                csv.path = "/demo/input/orders.csv".into();
                app.new_mapping_setup.as_mut().unwrap().source = Some(MappingBoundary::Csv(csv));
                std::fs::remove_file(path).unwrap();
            }
        }
        "wizard-protobuf" | "wizard-flextext" => {
            app.begin_new_mapping();
            let protobuf = view == "wizard-protobuf";
            let path = std::env::temp_dir().join(format!(
                "ferrule-dialog-{}-{}-layout.{}",
                std::process::id(),
                NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                if protobuf { "proto" } else { "mft" }
            ));
            std::fs::write(&path, if protobuf {
                "syntax = \"proto3\"; package demo; message Order { string name = 1; int32 quantity = 2; } message Invoice { string name = 1; }"
            } else {
                r#"<FlexText><Commands><Project FileName="orders.txt" LineEnding="LF" ByteOrderMark="0"><RootName Value="document"/><Connections><Connection><CSV><RecordName Value="rows"/><FieldSeparator Value=";"/><RecordSeparator Value="%0A"/><Fields><Field Type="string"><Name Value="Name"/></Field><Field Type="integer"><Name Value="Quantity"/></Field></Fields></CSV></Connection></Connections></Project></Commands></FlexText>"#
            }).unwrap();
            for side in [SchemaSide::Source, SchemaSide::Target] {
                app.stage_mapping_schema(side, path.clone());
                let setup = app.new_mapping_setup.as_mut().unwrap();
                let slot = match side {
                    SchemaSide::Source => &mut setup.source,
                    SchemaSide::Target => &mut setup.target,
                };
                match slot.as_mut().unwrap() {
                    MappingBoundary::Protobuf(draft) => {
                        draft.schema_path = "/demo/schemas/orders.proto".into();
                        draft.set_root_message("demo.Order".into());
                    }
                    MappingBoundary::FlexText(draft) => {
                        draft.configuration_path = "/demo/layouts/orders.mft".into();
                    }
                    _ => unreachable!(),
                }
            }
            std::fs::remove_file(path).unwrap();
        }
        "add-source" => app.begin_extra_source(),
        "add-target" => app.begin_extra_target(),
        "source-fixed-width" | "source-csv" => {
            app.begin_extra_source();
            let draft = app.extra_source_draft.as_mut().unwrap();
            draft.name = "Supplement".into();
            draft.instance_path = if view == "source-csv" {
                "/demo/supplement.csv"
            } else {
                "/demo/supplement.dat"
            }
            .into();
            draft.set_schema(flat_schema());
            if view == "source-csv" {
                draft.begin_csv_dialect().unwrap();
            } else {
                draft.begin_fixed_width().unwrap();
            }
        }
        "target-fixed-width" | "target-csv" | "edit-target" => {
            app.begin_extra_target();
            let draft = app.extra_target_draft.as_mut().unwrap();
            draft.name = "Invoice".into();
            draft.output_path = if view == "target-csv" {
                "/demo/invoice.csv"
            } else {
                "/demo/invoice.dat"
            }
            .into();
            draft.schema = Some(flat_schema());
            if view == "target-csv" {
                draft.begin_csv_dialect().unwrap();
            }
            if view == "target-fixed-width" {
                draft.begin_fixed_width().unwrap();
            }
            if view == "edit-target" {
                app.project.extra_targets.push(mapping::NamedTarget {
                    name: "Invoice".into(),
                    path: Some("/demo/invoice.json".into()),
                    schema: flat_schema(),
                    options: Default::default(),
                    root: Default::default(),
                });
                app.extra_target_draft = Some(crate::extra_targets::ExtraTargetDraft::from_target(
                    0,
                    &app.project.extra_targets[0],
                ));
            }
        }
        "remove-source" => {
            app.project.extra_sources.push(mapping::NamedSource {
                name: "Supplement".into(),
                path: "/demo/supplement.json".into(),
                schema: flat_schema(),
                options: Default::default(),
                dynamic_path: None,
            });
            app.pending_extra_source_removal = Some(0);
        }
        "remove-target" => {
            app.project.extra_targets.push(mapping::NamedTarget {
                name: "Invoice".into(),
                path: Some("/demo/invoice.json".into()),
                schema: flat_schema(),
                options: Default::default(),
                root: Default::default(),
            });
            app.pending_extra_target_removal = Some(0);
        }
        "appearance-theme" | "appearance-wires" | "appearance-canvas" => {
            app.show_appearance_editor = true;
            app.theme.preference = crate::theme::ThemePreference::Custom;
            app.appearance_tab = match view {
                "appearance-wires" => AppearanceTab::Wires,
                "appearance-canvas" => AppearanceTab::Canvas,
                _ => AppearanceTab::Theme,
            };
        }
        "functions" | "tabs" => {
            let id = mapping::FunctionId::new(1);
            app.project.user_functions.insert(
                id,
                mapping::UserFunction {
                    library: "local".into(),
                    name: "formatAmount".into(),
                    description: Some("Format an amount for an invoice".into()),
                    parameters: vec![],
                    output_name: "result".into(),
                    output_type: ir::ScalarType::String,
                    body: mapping::Graph {
                        nodes: [(
                            0,
                            mapping::Node::Const {
                                value: ir::Value::Null,
                            },
                        )]
                        .into(),
                    },
                    output: 0,
                },
            );
            app.open_function_tab(id);
            app.show_function_navigator = view == "functions";
        }
        "new-function" | "new-function-input" => app.new_function_draft = Some(Default::default()),
        "auto-connect" | "auto-connect-empty" => {
            if view == "auto-connect-empty" {
                app.project.source = ir::SchemaNode::group("Source", vec![]);
            }
            app.project.target = ir::SchemaNode::group(
                "Order",
                vec![
                    ir::SchemaNode::scalar("Name", ir::ScalarType::String),
                    ir::SchemaNode::scalar("Missing", ir::ScalarType::String),
                ],
            );
            app.begin_auto_connect();
        }
        "library" => app.begin_library_generation(),
        _ => panic!("unknown dialog scenario {view}"),
    }
    app
}

fn harness(view: &'static str, size: (f32, f32)) -> Harness<'static, FerruleApp> {
    let mut initialized = false;
    Harness::builder()
        .with_size(size)
        // Legacy widgets outside this lane do not all have accessible names.
        .with_accessibility_check(false)
        .with_os(egui::os::OperatingSystem::Windows)
        .with_max_steps(40)
        .build_ui_state(
            move |ui, app: &mut FerruleApp| {
                if !initialized {
                    crate::fonts::install(ui.ctx());
                    crate::icons::install(ui.ctx());
                    app.theme.apply(ui.ctx());
                    ui.ctx().all_styles_mut(|style| {
                        style.visuals.text_cursor.blink = false;
                        style.scroll_animation = egui::style::ScrollAnimation::none();
                        style.animation_time = 0.0;
                    });
                    initialized = true;
                }
                egui::Panel::top("dialog_test_shell").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("ferrule");
                        ui.label("Order mapping");
                    });
                    app.show_mapping_tabs(ui, true);
                });
                if !app.diagnostics.is_empty() {
                    egui::Panel::bottom("dialog_test_diagnostics").show(ui, |ui| {
                        app.diagnostics.show(ui);
                    });
                }
                let ctx = ui.ctx().clone();
                crate::ui_kit::set_workspace_rect(&ctx, ui.available_rect_before_wrap());
                app.show_main_canvas(ui, true);
                match view {
                    view if view.starts_with("wizard-") => app.show_new_mapping_setup(&ctx),
                    "add-source" | "source-fixed-width" | "source-csv" => {
                        app.show_extra_source_setup(&ctx)
                    }
                    "add-target" | "target-fixed-width" | "target-csv" | "edit-target" => {
                        app.show_extra_target_setup(&ctx)
                    }
                    "remove-source" => app.show_extra_source_removal_confirmation(&ctx),
                    "remove-target" => app.show_extra_target_removal_confirmation(&ctx),
                    view if view.starts_with("appearance-") => crate::appearance_editor::show(
                        &ctx,
                        &mut app.show_appearance_editor,
                        &mut app.appearance_tab,
                        &mut app.theme,
                        &mut app.appearance,
                        app.palette,
                    ),
                    "functions" => app.show_function_navigator(&ctx, true),
                    "new-function" | "new-function-input" => {
                        app.show_new_function_dialog(&ctx, true)
                    }
                    "auto-connect" | "auto-connect-empty" => {
                        app.show_auto_connect_confirmation(&ctx)
                    }
                    "library" => app.show_library_generation(&ctx),
                    "tabs" => (),
                    _ => unreachable!(),
                }
            },
            app_for(view),
        )
}

#[test]
#[ignore = "opt-in dialog images: FERRULE_DIALOG_SHOTS must name a new directory"]
fn dialog_visual_evidence() {
    let directory =
        PathBuf::from(std::env::var_os("FERRULE_DIALOG_SHOTS").expect("explicit image directory"));
    std::fs::create_dir(&directory).unwrap();
    for size in [(1600.0, 1000.0), (1280.0, 800.0)] {
        for view in [
            "wizard-empty",
            "wizard-schema",
            "wizard-csv",
            "wizard-csv-long",
            "wizard-csv-source",
            "wizard-fixed-width",
            "wizard-xlsx",
            "wizard-protobuf",
            "wizard-flextext",
            "add-source",
            "add-target",
            "source-fixed-width",
            "target-fixed-width",
            "source-csv",
            "target-csv",
            "edit-target",
            "remove-source",
            "remove-target",
            "appearance-theme",
            "appearance-wires",
            "appearance-canvas",
            "functions",
            "new-function",
            "new-function-input",
            "tabs",
            "auto-connect",
            "auto-connect-empty",
            "library",
        ] {
            let mut harness = harness(view, size);
            harness.run();
            if view == "new-function-input" {
                harness.get_by_label("Add input").click();
                harness.run();
            }
            if view == "appearance-theme" {
                // Compare the same expanded color section on both revisions.
                if harness.query_by_label("Background").is_none() {
                    harness.get_by_label("Surfaces").click();
                    harness.run();
                }
                for (section, field) in [
                    ("Text and actions", "Muted text"),
                    ("Data and status", "Source"),
                ] {
                    if harness.query_by_label(field).is_some() {
                        harness.get_by_label(section).click();
                        harness.run();
                    }
                }
            }
            harness.hover_at(egui::pos2(size.0 - 1.0, 1.0));
            harness.run();
            let image = harness.render().expect("dialog render");
            image
                .save(directory.join(format!("{view}-{}x{}.png", size.0 as u32, size.1 as u32)))
                .unwrap();
        }
    }
}

#[test]
fn dialog_footers_stay_visible_and_empty_forms_cannot_submit() {
    for size in [(1600.0, 1000.0), (1280.0, 800.0)] {
        for (view, label) in [
            ("wizard-csv-long", "Create mapping"),
            ("new-function", "Create"),
            ("add-source", "Add source"),
            ("add-target", "Add target"),
            ("auto-connect-empty", "Apply connections"),
        ] {
            let mut harness = harness(view, size);
            harness.run();
            let primary = harness.get_by_role_and_label(egui::accesskit::Role::Button, label);
            assert!(
                primary.rect().bottom() < size.1 - 10.0,
                "{view}: footer leaves viewport"
            );
            assert!(primary.rect().top() > 10.0);
            if view != "wizard-csv-long" {
                assert!(
                    primary.accesskit_node().is_disabled(),
                    "{view}: invalid form enabled"
                );
            }
            harness.get_by_label("Cancel").click();
            harness.run();
            assert!(
                harness.query_by_label(label).is_none(),
                "{view}: cancel did not close"
            );
        }
    }
}

#[test]
fn function_form_keeps_width_and_create_uses_validated_typed_edit() {
    let mut harness = harness("new-function", (1280.0, 800.0));
    harness.run();
    assert!(
        harness
            .get_by_label("Create")
            .accesskit_node()
            .is_disabled()
    );
    let before = harness.get_by_label("Function name").rect();
    harness.get_by_label("Function name").click();
    harness.run();
    harness
        .get_by_label("Function name")
        .type_text("formatAmount");
    harness.run();
    assert!(
        !harness
            .get_by_label("Create")
            .accesskit_node()
            .is_disabled()
    );
    harness.get_by_label("Add input").click();
    harness.run();
    let after = harness.get_by_label("Function name").rect();
    assert!((before.width() - after.width()).abs() < 1.0);
    let project_before = harness.state().project.clone();
    harness.get_by_label("Create").click();
    harness.run();
    assert!(harness.state().new_function_draft.is_none());
    assert_eq!(
        harness.state().project.user_functions.len(),
        project_before.user_functions.len() + 1
    );
    let function = harness
        .state()
        .project
        .user_functions
        .values()
        .next()
        .unwrap();
    assert_eq!(function.name, "formatAmount");
    assert_eq!(
        function.parameters[0].id,
        mapping::FunctionParameterId::new(1)
    );
}

#[test]
fn function_tabs_close_inline_without_changing_the_project() {
    let mut harness = harness("tabs", (1280.0, 800.0));
    harness.run();
    let original = serde_json::to_value(&harness.state().project).unwrap();
    harness.get_by_label("Main mapping");
    harness.get_by_label("formatAmount");
    harness.get_by_label("Close tab").click();
    harness.run();
    assert_eq!(
        harness.state().mapping_workspace.active,
        MappingDocument::Main
    );
    assert_eq!(
        serde_json::to_value(&harness.state().project).unwrap(),
        original
    );
    harness.get_by_label("Functions").click();
    harness.run();
    harness.get_by_label("Browse functions…").click();
    harness.run();
    assert!(harness.state().show_function_navigator);
}

#[test]
fn embedded_stage_tabs_hide_global_functions_but_keep_function_navigation() {
    let mut harness = harness("tabs", (1280.0, 800.0));
    harness.state_mut().embedded_stage_namespace = Some(egui::Id::unique("stage_function_test"));
    harness.run();
    assert!(harness.query_by_label("Functions").is_none());
    harness.get_by_label("Main mapping");
    harness.get_by_label("formatAmount");
    harness.get_by_label("Close tab");
    let before = serde_json::to_value(&harness.state().project).unwrap();
    harness.get_by_label("Main mapping").click();
    harness.run();
    assert_eq!(
        harness.state().mapping_workspace.active,
        MappingDocument::Main
    );
    harness.get_by_label("formatAmount").click();
    harness.run();
    assert!(matches!(
        harness.state().mapping_workspace.active,
        MappingDocument::Function(_)
    ));
    assert_eq!(
        serde_json::to_value(&harness.state().project).unwrap(),
        before
    );
    harness.state_mut().embedded_stage_namespace = None;
    harness.run();
    harness.get_by_label("Functions").click();
    harness.run();
    harness.get_by_label("Browse functions…").click();
    harness.run();
    assert!(harness.state().show_function_navigator);
}

fn replace_text(harness: &mut Harness<'_, FerruleApp>, label: &str, text: &str) {
    harness.get_by_label(label).click();
    harness.run();
    harness.key_press_modifiers(
        egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
        egui::Key::A,
    );
    harness.run();
    harness.get_by_label(label).type_text(text);
    harness.run();
}

fn csv_draft_snapshot(draft: &CsvBoundaryDraft) -> serde_json::Value {
    // Exhaustive destructuring makes a new draft field require coverage here.
    let CsvBoundaryDraft {
        path,
        delimiter,
        quote_mode,
        custom_quote,
        has_header_row,
        preserve_empty_strings,
        utf8_bom,
        columns,
        preview_rows,
        sample_error,
    } = draft;
    serde_json::json!({
        "path": path,
        "delimiter": delimiter.to_string(),
        "quote_mode": format!("{quote_mode:?}"),
        "custom_quote": custom_quote,
        "has_header_row": has_header_row,
        "preserve_empty_strings": preserve_empty_strings,
        "utf8_bom": utf8_bom,
        "columns": columns.iter().map(|column| (&column.name, column.ty)).collect::<Vec<_>>(),
        "preview_rows": preview_rows,
        "sample_error": sample_error,
    })
}

fn choose_csv_target(harness: &mut Harness<'_, FerruleApp>) {
    harness
        .get_all_by_label("Choose format…")
        .nth(1)
        .expect("target format menu")
        .click();
    harness.run();
    harness.get_by_label("CSV / delimited text…").click();
    harness.run();
}

#[test]
fn csv_target_format_reselection_preserves_every_edited_draft_field() {
    for size in [(1600.0, 1000.0), (1280.0, 800.0)] {
        for sample_error in [None, Some("Retained draft validation error".to_owned())] {
            let mut harness = harness("wizard-csv", size);
            let Some(MappingBoundary::Csv(draft)) = harness
                .state_mut()
                .new_mapping_setup
                .as_mut()
                .unwrap()
                .target
                .as_mut()
            else {
                panic!("CSV target");
            };
            draft.path = "/demo/custom-output.csv".into();
            draft.delimiter = ';';
            draft.quote_mode = CsvQuoteMode::Custom;
            draft.custom_quote = "~".into();
            draft.has_header_row = false;
            draft.preserve_empty_strings = true;
            draft.utf8_bom = true;
            draft.columns = [
                ("Customer", ir::ScalarType::String),
                ("Quantity", ir::ScalarType::Int),
                ("Amount", ir::ScalarType::Float),
                ("Active", ir::ScalarType::Bool),
            ]
            .into_iter()
            .map(|(name, ty)| CsvColumnDraft {
                name: name.into(),
                ty,
            })
            .collect();
            draft.preview_rows = vec![vec!["Retained preview".into(), "7".into()]];
            draft.sample_error = sample_error;
            let before = csv_draft_snapshot(draft);
            let project_before = serde_json::to_value(&harness.state().project).unwrap();
            harness.run();
            for _ in 0..2 {
                choose_csv_target(&mut harness);
                let setup = harness.state().new_mapping_setup.as_ref().unwrap();
                let Some(MappingBoundary::Csv(draft)) = setup.target.as_ref() else {
                    panic!("CSV target retained");
                };
                assert_eq!(csv_draft_snapshot(draft), before);
                assert!(matches!(
                    setup.source.as_ref(),
                    Some(MappingBoundary::Schema(_))
                ));
                assert!(harness.state().pending_dialog.is_none());
                assert_eq!(
                    serde_json::to_value(&harness.state().project).unwrap(),
                    project_before
                );
            }
        }
    }
}

#[test]
fn csv_target_format_selection_initializes_a_new_or_non_csv_target() {
    for view in ["wizard-empty", "wizard-schema"] {
        let mut harness = harness(view, (1280.0, 800.0));
        harness.run();
        choose_csv_target(&mut harness);
        let Some(MappingBoundary::Csv(draft)) = harness
            .state()
            .new_mapping_setup
            .as_ref()
            .unwrap()
            .target
            .as_ref()
        else {
            panic!("new CSV target");
        };
        assert_eq!(
            csv_draft_snapshot(draft),
            csv_draft_snapshot(&CsvBoundaryDraft::target())
        );
    }
}

#[test]
fn schema_import_failure_diagnostic_shows_the_cause() {
    let directory = std::env::temp_dir().join(format!(
        "ferrule-dialog-{}-{}-schema",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("contentless.json");
    std::fs::write(&path, "[1,2,3]").unwrap();
    let mut harness = harness("wizard-empty", (1280.0, 800.0));
    harness.run();
    harness
        .state_mut()
        .stage_mapping_schema(SchemaSide::Source, path.clone());
    std::fs::remove_dir_all(&directory).unwrap();
    harness.run();

    let app = harness.state();
    assert!(app.new_mapping_setup.as_ref().unwrap().source.is_none());
    let [diagnostic] = app.diagnostics.items() else {
        panic!("one diagnostic: {:?}", app.diagnostics.items());
    };
    // The outer context names the file; the cause says why it was refused.
    let context = format!("importing json schema {}", path.display());
    let cause = "describes no JSON value: a schema must be an object or a boolean, \
                 but this one is an array";
    assert!(
        diagnostic.message.starts_with(&context) && diagnostic.message.contains(cause),
        "{}",
        diagnostic.message
    );
    harness.get_by_label(&diagnostic.message);
}

#[test]
fn advanced_wizard_paths_are_editable_and_keep_embedded_formats() {
    for (view, label) in [
        ("wizard-fixed-width", "Source fixed-width path"),
        ("wizard-xlsx", "Source workbook path"),
        ("wizard-protobuf", "Source protobuf path"),
        ("wizard-flextext", "Source FlexText path"),
    ] {
        let mut harness = harness(view, (1280.0, 800.0));
        harness.run();
        assert!(harness.get_by_label(label).rect().width() >= 180.0);
        replace_text(&mut harness, label, "/demo/input.dat");
        harness.get_by_label("Create mapping").click();
        harness.run();
        assert!(
            harness.state().new_mapping_setup.is_none(),
            "{view} did not create"
        );
        assert_eq!(
            harness.state().project.source_path.as_deref(),
            Some("/demo/input.dat")
        );
        let options = &harness.state().project.source_options;
        match view {
            "wizard-fixed-width" => assert!(options.fixed_width.is_some()),
            "wizard-xlsx" => assert_eq!(
                options.tabular_kind,
                Some(mapping::TabularBoundaryKind::Xlsx)
            ),
            "wizard-protobuf" => assert!(options.protobuf.is_some()),
            "wizard-flextext" => assert!(options.flextext.is_some()),
            _ => unreachable!(),
        }
    }
}

#[test]
fn fixed_width_validation_and_csv_column_actions_are_live() {
    let mut harness = harness("wizard-fixed-width", (1280.0, 800.0));
    harness.run();
    replace_text(&mut harness, "Source Name width", "0");
    assert!(
        harness
            .get_by_label("Create mapping")
            .accesskit_node()
            .is_disabled()
    );
    replace_text(&mut harness, "Source Name width", "12");
    assert!(
        !harness
            .get_by_label("Create mapping")
            .accesskit_node()
            .is_disabled()
    );

    let mut harness = self::harness("wizard-csv", (1280.0, 800.0));
    harness.run();
    harness.get_by_label("Add column").click();
    harness.run();
    assert_eq!(harness.get_all_by_label("Remove column").count(), 3);
    harness
        .get_all_by_label("Remove column")
        .last()
        .unwrap()
        .click();
    harness.run();
    assert_eq!(harness.get_all_by_label("Remove column").count(), 2);
}

#[test]
fn appearance_hex_edits_apply_only_valid_colors() {
    let mut harness = harness("appearance-theme", (1280.0, 800.0));
    harness.run();
    let original = harness.state().theme.custom.background;
    replace_text(&mut harness, "Background hex", "#zzzzzz");
    assert_eq!(harness.state().theme.custom.background, original);
    replace_text(&mut harness, "Background hex", "#123456");
    assert_eq!(
        harness.state().theme.custom.background.color32(),
        egui::Color32::from_rgb(0x12, 0x34, 0x56)
    );
    harness.get_by_label("Start from…").click();
    harness.run();
    harness.get_by_label("Light palette").click();
    harness.run();
    assert!(!harness.state().theme.custom.dark_mode);
}

#[test]
fn workbook_transposed_rows_validate_and_protobuf_root_selection_updates() {
    let mut harness = harness("wizard-xlsx", (1280.0, 800.0));
    harness.run();
    harness
        .get_by_role_and_label(egui::accesskit::Role::ComboBox, "Read layout")
        .click();
    harness.run();
    harness
        .get_by_label("Columns as records (transposed)")
        .click();
    harness.run();
    replace_text(&mut harness, "Worksheet row for Name", "0");
    assert!(
        harness
            .get_by_label("Create mapping")
            .accesskit_node()
            .is_disabled()
    );
    replace_text(&mut harness, "Worksheet row for Name", "3");
    harness.get_by_label("Create mapping").click();
    harness.run();
    assert_eq!(harness.state().project.source_options.xlsx_rows, [3, 2]);

    let mut harness = self::harness("wizard-protobuf", (1280.0, 800.0));
    harness.run();
    harness
        .get_all_by_role(egui::accesskit::Role::ComboBox)
        .next()
        .unwrap()
        .click();
    harness.run();
    harness.get_by_label("demo.Invoice").click();
    harness.run();
    harness.get_by_label("Create mapping").click();
    harness.run();
    assert_eq!(
        harness
            .state()
            .project
            .source_options
            .protobuf
            .as_ref()
            .unwrap()
            .root_message,
        "demo.Invoice"
    );
    assert_eq!(
        harness
            .state()
            .project
            .target_options
            .protobuf
            .as_ref()
            .unwrap()
            .root_message,
        "demo.Order"
    );
}

#[test]
fn named_boundary_forms_keep_values_and_removal_cancel_keeps_project() {
    for view in ["source-fixed-width", "target-fixed-width"] {
        let mut harness = harness(view, (1280.0, 800.0));
        harness.run();
        let source = view.starts_with("source");
        replace_text(
            &mut harness,
            if source { "Source name" } else { "Target name" },
            "Orders",
        );
        replace_text(
            &mut harness,
            if source {
                "Source input file"
            } else {
                "Target output file"
            },
            "/demo/orders.dat",
        );
        harness
            .get_by_role_and_label(
                egui::accesskit::Role::Button,
                if source { "Add source" } else { "Add target" },
            )
            .click();
        harness.run();
        if source {
            let saved = &harness.state().project.extra_sources[0];
            assert_eq!(saved.name, "Orders");
            assert_eq!(saved.path, "/demo/orders.dat");
            assert!(saved.options.fixed_width.is_some());
        } else {
            let saved = &harness.state().project.extra_targets[0];
            assert_eq!(saved.name, "Orders");
            assert_eq!(saved.path.as_deref(), Some("/demo/orders.dat"));
            assert!(saved.options.fixed_width.is_some());
        }
    }
    for view in ["remove-source", "remove-target"] {
        let mut harness = harness(view, (1280.0, 800.0));
        harness.run();
        let original = serde_json::to_value(&harness.state().project).unwrap();
        harness.get_by_label("Cancel").click();
        harness.run();
        assert_eq!(
            serde_json::to_value(&harness.state().project).unwrap(),
            original
        );
        assert!(harness.state().pending_extra_source_removal.is_none());
        assert!(harness.state().pending_extra_target_removal.is_none());
    }
}
