use super::*;
use ir::SchemaNode;
use mapping::{FormatOptions, ProtobufOptions};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> anyhow::Result<Self> {
        let directory = std::env::temp_dir().join(format!(
            "ferrule-gui-named-source-format-{}-{}",
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

fn protobuf_boundary() -> anyhow::Result<(SchemaNode, FormatOptions)> {
    let proto = r#"syntax = "proto3"; package demo;
message Person { string name = 1; int32 id = 2; }"#;
    let layout = format_protobuf::Layout::parse(proto)?;
    let schema = format_protobuf::to_ir_schema(&layout, "demo.Person")?;
    Ok((
        schema,
        FormatOptions {
            protobuf: Some(ProtobufOptions {
                schema: proto.to_owned(),
                root_message: "demo.Person".into(),
                schema_path: None,
                imports: Vec::new(),
            }),
            ..FormatOptions::default()
        },
    ))
}

#[test]
fn explicit_input_formats_override_sqlite_controls_and_render_without_mutation()
-> anyhow::Result<()> {
    let (schema, protobuf) = protobuf_boundary()?;
    let variants = [
        protobuf,
        FormatOptions {
            csv_text_repair_dependency: Some(mapping::CsvTextRepairDependency::new(
                mapping::CsvTextRepairCause::Encoding,
            )),
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
    ];
    for options in variants {
        for suffix in ["db", "sqlite", "sqlite3"] {
            let mut app = FerruleApp::default();
            app.begin_extra_source();
            let draft = app.extra_source_draft.as_mut().unwrap();
            draft.name = "contacts".into();
            draft.instance_path = format!("contacts.{suffix}");
            draft.schema = Some(schema.clone());
            draft.options = options.clone();
            assert!(!source_uses_sqlite(draft));
            let context = egui::Context::default();
            let _ = context.run_ui(egui::RawInput::default(), |ui| {
                app.show_extra_source_setup(ui.ctx())
            });
            let draft = app.extra_source_draft.as_ref().unwrap();
            assert_eq!(draft.options, options);
            assert_eq!(draft.schema.as_ref(), Some(&schema));
            app.stage_extra_source_sqlite_table();
            let draft = app.extra_source_draft.as_ref().unwrap();
            assert_eq!(draft.options, options);
            assert_eq!(draft.schema.as_ref(), Some(&schema));
            assert_eq!(app.status, "SQLite table import blocked");
        }
    }
    let mut path_driven = ExtraSourceDraft::default();
    path_driven.instance_path = "contacts.sqlite".into();
    assert!(source_uses_sqlite(&path_driven));
    Ok(())
}

#[test]
fn workbook_fallback_does_not_hide_sqlite_controls_for_db_paths() {
    let options = FormatOptions {
        tabular_kind: Some(mapping::TabularBoundaryKind::Xlsx),
        xlsx_sheet: Some("Data".into()),
        ..FormatOptions::default()
    };
    let mut draft = ExtraSourceDraft::default();
    draft.set_instance_path("contacts.db".into());
    draft.options = options.clone();
    let mut app = FerruleApp {
        extra_source_draft: Some(draft),
        ..FerruleApp::default()
    };
    assert!(source_uses_sqlite(app.extra_source_draft.as_ref().unwrap()));
    let csv_fallback = FormatOptions {
        tabular_kind: Some(mapping::TabularBoundaryKind::Csv),
        delimiter: Some('|'),
        ..FormatOptions::default()
    };
    let mut csv_path_draft = ExtraSourceDraft::default();
    csv_path_draft.set_instance_path("contacts.db".into());
    csv_path_draft.options = csv_fallback;
    assert!(source_uses_sqlite(&csv_path_draft));
    assert!(crate::new_mapping::uses_path_format(
        &options,
        "contacts.csv"
    ));
    assert!(!crate::new_mapping::uses_path_format(
        &options,
        "contacts.xlsx"
    ));
    assert!(!crate::new_mapping::uses_path_format(
        &options,
        "contacts.capture"
    ));
    let context = egui::Context::default();
    let _ = context.run_ui(egui::RawInput::default(), |ui| {
        app.show_extra_source_setup(ui.ctx());
    });
    assert_eq!(app.extra_source_draft.as_ref().unwrap().options, options);
}

#[test]
fn matching_source_schema_keeps_binary_metadata_and_mismatches_preserve_the_draft()
-> anyhow::Result<()> {
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
<xs:element name="changed" type="xs:boolean"/>
</xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    let (schema, options) = protobuf_boundary()?;
    let mut app = FerruleApp::default();
    app.begin_extra_source();
    let draft = app.extra_source_draft.as_mut().unwrap();
    draft.name = "contacts".into();
    draft.instance_path = "contacts.db".into();
    draft.set_schema(schema.clone());
    draft.options = options.clone();
    app.stage_extra_source_schema(matching_path);
    let draft = app.extra_source_draft.as_ref().unwrap();
    assert_eq!(draft.options, options);
    assert_eq!(draft.schema.as_ref(), Some(&schema));
    assert!(app.status.contains("kept Protocol Buffers"));
    app.stage_extra_source_schema(mismatched_path.clone());
    let draft = app.extra_source_draft.as_ref().unwrap();
    assert_eq!(draft.options, options);
    assert_eq!(draft.schema.as_ref(), Some(&schema));
    assert_eq!(app.status, "failed to load extra source schema");
    app.extra_source_draft.as_mut().unwrap().options = FormatOptions {
        xml_document: true,
        ..FormatOptions::default()
    };
    app.stage_extra_source_schema(mismatched_path);
    let draft = app.extra_source_draft.as_ref().unwrap();
    assert!(draft.options.protobuf.is_none());
    assert!(draft.options.xml_document);
    assert!(draft.schema.as_ref().unwrap().child("changed").is_some());
    Ok(())
}
