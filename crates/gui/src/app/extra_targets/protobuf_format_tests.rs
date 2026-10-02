use super::*;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Node, ProtobufOptions, Scope};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> anyhow::Result<Self> {
        let directory = std::env::temp_dir().join(format!(
            "ferrule-gui-named-target-format-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory)?;
        Ok(Self(directory))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn protobuf_boundary() -> anyhow::Result<(SchemaNode, FormatOptions, format_protobuf::Layout)> {
    let proto = r#"syntax = "proto3"; package demo;
message Person { string name = 1; int32 id = 2; }"#;
    let layout = format_protobuf::Layout::parse(proto)?;
    let schema = format_protobuf::to_ir_schema(&layout, "demo.Person")?;
    let options = FormatOptions {
        protobuf: Some(ProtobufOptions {
            schema: proto.to_owned(),
            root_message: "demo.Person".to_owned(),
            schema_path: None,
            imports: Vec::new(),
        }),
        ..FormatOptions::default()
    };
    Ok((schema, options, layout))
}

fn render_options(path: &str, options: &mut FormatOptions) {
    let context = egui::Context::default();
    let _ = context.run_ui(egui::RawInput::default(), |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            show_target_format_options(ui, path, options);
        });
    });
}

#[test]
fn rendering_preserves_default_csv_explicit_documents_and_configured_layouts() -> anyhow::Result<()>
{
    let (_, protobuf, _) = protobuf_boundary()?;
    let fixed = mapping::FixedWidthLayout::new(
        vec![mapping::FixedFieldWidth::new(10).unwrap()],
        ' ',
        true,
        true,
    )?;
    let flex = mapping::FlexTextLayout::new(
        "root",
        mapping::FlexCommand::Store {
            name: "text".into(),
            ty: ScalarType::String,
            trim: None,
        },
        mapping::FlexLineEnding::Lf,
        false,
    )?;
    let variants = vec![
        FormatOptions::default(),
        FormatOptions {
            csv_text_repair_dependency: Some(mapping::CsvTextRepairDependency::new(
                mapping::CsvTextRepairCause::ByteOrderMark,
            )),
            ..FormatOptions::default()
        },
        FormatOptions {
            has_header_row: Some(false),
            csv_utf8_bom: true,
            ..FormatOptions::default()
        },
        FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        },
        FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        FormatOptions {
            json_lines: true,
            ..FormatOptions::default()
        },
        FormatOptions {
            json5: true,
            ..FormatOptions::default()
        },
        protobuf,
        FormatOptions {
            fixed_width: Some(fixed),
            ..FormatOptions::default()
        },
        FormatOptions {
            flextext: Some(flex),
            ..FormatOptions::default()
        },
        FormatOptions {
            edi_kind: Some(mapping::EdiBoundaryKind::X12),
            ..FormatOptions::default()
        },
        FormatOptions {
            xlsx_sheet: Some("Data".into()),
            xlsx_start_row: Some(7),
            ..FormatOptions::default()
        },
    ];
    for before in variants {
        for path in [
            "output.csv",
            "output.txt",
            "output.xlsx",
            "output.db",
            "output.json",
        ] {
            let mut options = before.clone();
            render_options(path, &mut options);
            assert_eq!(
                options, before,
                "rendering {path} changed the saved options"
            );
        }
    }
    Ok(())
}

#[test]
fn workbook_fallback_follows_path_and_update_requires_flat_layout() {
    let workbook = FormatOptions {
        tabular_kind: Some(mapping::TabularBoundaryKind::Xlsx),
        xlsx_sheet: Some("Data".into()),
        ..FormatOptions::default()
    };
    for path in ["output.csv", "output.db", "output.json"] {
        assert_eq!(document_kind(&workbook, path), DocumentKind::Automatic);
        assert!(crate::new_mapping::uses_path_format(&workbook, path));
        assert!(!crate::new_mapping::can_update_existing_workbook(
            &workbook, path
        ));
    }
    for path in ["output.xlsx", "output.capture"] {
        assert_eq!(document_kind(&workbook, path), DocumentKind::Configured);
        assert!(crate::new_mapping::can_update_existing_workbook(
            &workbook, path
        ));
    }
    let csv_fallback = FormatOptions {
        tabular_kind: Some(mapping::TabularBoundaryKind::Csv),
        delimiter: Some('|'),
        ..FormatOptions::default()
    };
    for path in ["output.csv", "output.db", "output.xlsx"] {
        assert_eq!(document_kind(&csv_fallback, path), DocumentKind::Automatic);
        assert!(crate::new_mapping::uses_path_format(&csv_fallback, path));
    }
    assert!(crate::new_mapping::can_update_existing_workbook(
        &csv_fallback,
        "output.xlsx"
    ));
    assert!(!crate::new_mapping::can_update_existing_workbook(
        &csv_fallback,
        "output.db"
    ));
    assert!(crate::new_mapping::uses_csv_format(
        &csv_fallback,
        "output.capture"
    ));
    assert!(crate::new_mapping::uses_csv_format(
        &csv_fallback,
        "output.csv"
    ));
    assert!(!crate::new_mapping::uses_csv_format(
        &csv_fallback,
        "output.xlsx"
    ));
    assert!(!crate::new_mapping::uses_csv_format(
        &csv_fallback,
        "output.db"
    ));
    let before = csv_fallback.clone();
    let mut rendered = csv_fallback;
    render_options("output.capture", &mut rendered);
    assert_eq!(rendered, before);
    let transposed = FormatOptions {
        tabular_kind: Some(mapping::TabularBoundaryKind::Xlsx),
        xlsx_rows: vec![1],
        ..FormatOptions::default()
    };
    assert!(!crate::new_mapping::can_update_existing_workbook(
        &transposed,
        "output.xlsx"
    ));
    let hierarchical = FormatOptions {
        xlsx_hierarchical: Some(mapping::XlsxHierarchicalLayout {
            worksheets_path: vec!["Sheets".into()],
            worksheet_name_path: vec!["Name".into()],
            ranges: vec![],
        }),
        ..FormatOptions::default()
    };
    assert!(!crate::new_mapping::can_update_existing_workbook(
        &hierarchical,
        "output.xlsx"
    ));
}

#[test]
fn deliberate_document_switches_clear_previous_binary_csv_and_bom_settings() -> anyhow::Result<()> {
    let (_, protobuf, _) = protobuf_boundary()?;
    let csv = FormatOptions {
        tabular_kind: Some(mapping::TabularBoundaryKind::Csv),
        delimiter: Some('|'),
        csv_quote: Some('\''),
        csv_utf8_bom: true,
        csv_preserve_empty_strings: true,
        has_header_row: Some(false),
        ..FormatOptions::default()
    };
    let repair = FormatOptions {
        csv_text_repair_dependency: Some(mapping::CsvTextRepairDependency::new(
            mapping::CsvTextRepairCause::Encoding,
        )),
        ..csv.clone()
    };
    for previous in [csv, protobuf, repair] {
        for kind in [
            DocumentKind::Xml,
            DocumentKind::Json,
            DocumentKind::JsonLines,
            DocumentKind::Automatic,
        ] {
            let mut options = previous.clone();
            set_document_kind(&mut options, kind);
            let expected = FormatOptions {
                xml_document: kind == DocumentKind::Xml,
                json_document: kind == DocumentKind::Json,
                json_lines: kind == DocumentKind::JsonLines,
                ..FormatOptions::default()
            };
            assert_eq!(options, expected);
            let before = options.clone();
            render_options("output.csv", &mut options);
            assert_eq!(options, before);
        }
    }
    Ok(())
}

#[test]
fn schema_replacement_preserves_matching_binary_codec_and_rejects_mismatches() -> anyhow::Result<()>
{
    let directory = TestDirectory::new()?;
    let matching_path = directory.0.join("matching.xsd");
    let mismatched_path = directory.0.join("mismatched.xsd");
    std::fs::write(
        &matching_path,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:element name="Person"><xs:complexType><xs:sequence>
<xs:element name="name" type="xs:string"/><xs:element name="id" type="xs:int"/>
</xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    std::fs::write(
        &mismatched_path,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:element name="Person"><xs:complexType><xs:sequence>
<xs:element name="other" type="xs:string"/>
</xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    let (schema, options, _) = protobuf_boundary()?;
    let mut app = FerruleApp {
        extra_target_draft: Some(ExtraTargetDraft {
            name: "binary".into(),
            output_path: "target.csv".into(),
            schema: Some(schema.clone()),
            options: options.clone(),
            ..ExtraTargetDraft::default()
        }),
        ..FerruleApp::default()
    };
    app.stage_extra_target_schema(matching_path);
    let draft = app.extra_target_draft.as_ref().unwrap();
    assert_eq!(draft.schema.as_ref(), Some(&schema));
    assert_eq!(draft.options, options);
    assert!(app.status.contains("kept Protocol Buffers"));
    app.stage_extra_target_schema(mismatched_path.clone());
    let draft = app.extra_target_draft.as_ref().unwrap();
    assert_eq!(draft.schema.as_ref(), Some(&schema));
    assert_eq!(draft.options, options);
    assert_eq!(app.status, "failed to load target schema");
    set_document_kind(
        &mut app.extra_target_draft.as_mut().unwrap().options,
        DocumentKind::Xml,
    );
    app.stage_extra_target_schema(mismatched_path);
    let draft = app.extra_target_draft.as_ref().unwrap();
    assert!(draft.schema.as_ref().unwrap().child("other").is_some());
    assert!(draft.options.protobuf.is_none());
    assert!(draft.options.xml_document);
    Ok(())
}

#[test]
fn named_binary_source_and_target_keep_exact_file_and_payload_bytes_after_editing()
-> anyhow::Result<()> {
    let directory = TestDirectory::new()?;
    let (schema, options, layout) = protobuf_boundary()?;
    let source_value = Instance::Group(
        (vec![
            ("name".into(), Instance::Scalar(Value::String("Zoë".into()))),
            ("id".into(), Instance::Scalar(Value::Int(7))),
        ])
        .into(),
    );
    let binary_bytes = format_protobuf::to_vec(&layout, "demo.Person", &source_value)?;
    let named_source_path = directory.0.join("contacts.sqlite");
    let primary_source_path = directory.0.join("driver.json");
    let primary_output_path = directory.0.join("main.xml");
    let project_path = directory.0.join("mapping.json");
    std::fs::write(&named_source_path, &binary_bytes)?;
    let primary_bytes = br#"{"name":"driver"}"#;
    std::fs::write(&primary_source_path, primary_bytes)?;
    let mut app = FerruleApp::default();
    app.project.source = SchemaNode::group(
        "Driver",
        vec![SchemaNode::scalar("name", ScalarType::String)],
    );
    app.project.target =
        SchemaNode::group("Main", vec![SchemaNode::scalar("name", ScalarType::String)]);
    app.project.source_path = Some(primary_source_path.to_str().unwrap().to_owned());
    app.project.target_path = Some(primary_output_path.to_str().unwrap().to_owned());
    app.project.target_options.xml_document = true;
    app.project.extra_sources.push(mapping::NamedSource {
        name: "contacts".into(),
        path: named_source_path.to_str().unwrap().to_owned(),
        schema: schema.clone(),
        options: options.clone(),
        dynamic_path: None,
    });
    for (id, path) in [
        (0, vec!["name".into()]),
        (1, vec!["contacts".into(), "name".into()]),
        (2, vec!["contacts".into(), "id".into()]),
    ] {
        app.project
            .graph
            .nodes
            .insert(id, Node::SourceField { path, frame: None });
    }
    app.project.root.bindings = vec![Binding {
        target_field: "name".into(),
        node: 0,
    }];
    app.project.extra_targets.push(mapping::NamedTarget {
        name: "binary".into(),
        path: Some(directory.0.join("target.csv").to_str().unwrap().to_owned()),
        schema,
        options: options.clone(),
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "name".into(),
                    node: 1,
                },
                Binding {
                    target_field: "id".into(),
                    node: 2,
                },
            ],
            ..Scope::default()
        },
    });
    for suffix in ["csv", "txt", "xlsx", "db", "json"] {
        let target_path = directory.0.join(format!("target.{suffix}"));
        app.edit_extra_target(0);
        app.extra_target_draft.as_mut().unwrap().output_path =
            target_path.to_str().unwrap().to_owned();
        let context = egui::Context::default();
        let _ = context.run_ui(egui::RawInput::default(), |ui| {
            app.show_extra_target_setup(ui.ctx())
        });
        assert_eq!(app.extra_target_draft.as_ref().unwrap().options, options);
        app.finish_extra_target();
        app.save_document_to(&project_path)?;
        app.load_project_from(&project_path);
        assert!(cli::validate(&app.project).is_empty());
        cli::run_project_with_paths(&project_path, None, None)?;
        assert_eq!(std::fs::read(&target_path)?, binary_bytes);
        let extra_inputs = [cli::NamedPayloadInput::new(
            "contacts",
            cli::PayloadDocument::new(&named_source_path, &binary_bytes)?,
        )?];
        let payload = cli::run_project_value_payloads(
            &app.project,
            &project_path,
            &cli::PayloadRunOptions::new(cli::PayloadDocument::new(
                &primary_source_path,
                primary_bytes,
            )?)
            .with_extra_sources(&extra_inputs),
        )?;
        let artifact = payload
            .artifacts
            .iter()
            .find(|artifact| artifact.target == "binary")
            .unwrap();
        assert_eq!(artifact.path, target_path);
        assert_eq!(artifact.bytes, binary_bytes);
    }
    Ok(())
}

#[test]
fn explicit_document_outputs_keep_file_and_payload_bytes_with_csv_filenames() -> anyhow::Result<()>
{
    let directory = TestDirectory::new()?;
    let source_path = directory.0.join("input.json");
    let project_path = directory.0.join("mapping.json");
    let input = r#"{"name":"Zoë"}"#.as_bytes();
    std::fs::write(&source_path, input)?;
    for kind in [
        DocumentKind::Xml,
        DocumentKind::Json,
        DocumentKind::JsonLines,
    ] {
        let mut project = crate::new_mapping::blank_project();
        project.source =
            SchemaNode::group("Row", vec![SchemaNode::scalar("name", ScalarType::String)]);
        project.target = project.source.clone();
        project.source_path = Some(source_path.to_str().unwrap().to_owned());
        let output_path = directory.0.join(format!("output-{kind:?}.csv"));
        project.target_path = Some(output_path.to_str().unwrap().to_owned());
        project.graph.nodes.insert(
            0,
            Node::SourceField {
                path: vec!["name".into()],
                frame: None,
            },
        );
        project.root.bindings.push(Binding {
            target_field: "name".into(),
            node: 0,
        });
        project.target_options.csv_utf8_bom = true;
        project.target_options.delimiter = Some('|');
        project.target_options.has_header_row = Some(false);
        set_document_kind(&mut project.target_options, kind);
        render_options(output_path.to_str().unwrap(), &mut project.target_options);
        std::fs::write(
            &project_path,
            mapping::project_file::encode_pretty(&project)?,
        )?;
        cli::run_project_with_paths(&project_path, None, None)?;
        let expected = std::fs::read(&output_path)?;
        assert!(!expected.starts_with(&[0xef, 0xbb, 0xbf]));
        let payload = cli::run_project_value_payloads(
            &project,
            &project_path,
            &cli::PayloadRunOptions::new(cli::PayloadDocument::new(&source_path, input)?),
        )?;
        assert_eq!(payload.artifacts[0].bytes, expected);
        match kind {
            DocumentKind::Xml => assert!(String::from_utf8(expected)?.contains("<name>Zoë</name>")),
            DocumentKind::Json | DocumentKind::JsonLines => assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&expected)?["name"],
                "Zoë"
            ),
            _ => unreachable!(),
        }
    }
    Ok(())
}
