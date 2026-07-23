use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaKind, Value};
use mapping::ProtobufOptions;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_protobuf_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scalar<'a>(instance: &'a Instance, name: &str) -> Option<&'a Value> {
    instance.field(name).and_then(Instance::as_scalar)
}

fn protobuf_directory() -> Instance {
    let record = |code, label: &str, rank| {
        Instance::Group(vec![
            ("code".into(), Instance::Scalar(Value::Int(code))),
            (
                "label".into(),
                Instance::Scalar(Value::String(label.into())),
            ),
            ("rank".into(), Instance::Scalar(Value::Int(rank))),
            ("notes".into(), Instance::Repeated(Vec::new())),
        ])
    };
    Instance::Group(vec![
        (
            "title".into(),
            Instance::Scalar(Value::String("Imported".into())),
        ),
        (
            "records".into(),
            Instance::Repeated(vec![record(4, "Four", 1), record(8, "Eight", 0)]),
        ),
    ])
}

fn embedded_layout(options: &ProtobufOptions) -> format_protobuf::Layout {
    format_protobuf::Layout::parse_files(
        options.schema_path.as_deref().unwrap_or("root.proto"),
        &options.schema,
        options
            .imports
            .iter()
            .map(|file| (file.path.as_str(), file.source.as_str())),
    )
    .unwrap_or_else(|error| panic!("embedded protobuf graph should parse: {error}"))
}

#[test]
fn multi_file_source_is_embedded_executable_and_roundtrips_without_originals() {
    let temp = TempDir::new();
    for directory in ["api", "shared", "common"] {
        std::fs::create_dir_all(temp.0.join(directory)).unwrap();
    }
    let root_source = r#"
syntax = "proto3";
package app;
import "shared/model.proto";
message Envelope {
  shared.model.Record record = 1;
  shared.types.Status status = 2;
}
"#;
    let model_source = r#"
syntax = "proto3";
package shared.model;
import public "common/status.proto";
message Record {
  string name = 1;
  shared.types.Status status = 2;
}
"#;
    let status_source = r#"
syntax = "proto3";
package shared.types;
enum Status { STATUS_UNSPECIFIED = 0; READY = 1; }
"#;
    std::fs::write(temp.0.join("api/root.proto"), root_source).unwrap();
    std::fs::write(temp.0.join("shared/model.proto"), model_source).unwrap();
    std::fs::write(temp.0.join("common/status.proto"), status_source).unwrap();
    std::fs::write(
        temp.0.join("result.xsd"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Result">
    <xs:complexType><xs:sequence>
      <xs:element name="Name" type="xs:string"/>
      <xs:element name="Status" type="xs:int"/>
    </xs:sequence></xs:complexType>
  </xs:element>
</xs:schema>
"#,
    )
    .unwrap();
    let design = temp.0.join("multi-file.mfd");
    std::fs::write(
        &design,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<mapping version="32">
  <resources/>
  <component name="defaultmap" uid="1">
    <properties SelectedLanguage="builtin"/>
    <structure><children>
      <component name="Envelope" library="binary" uid="2" kind="33">
        <properties/>
        <data><root>
          <entry name="FileInstance" expanded="1">
            <entry name="document" type="doc-protobuf" expanded="1">
              <document schemafile="api/root.proto" root="{app}Envelope"/>
              <entry name="Envelope" expanded="1">
                <entry name="record" expanded="1">
                  <entry name="name" outkey="11"/>
                  <entry name="status"/>
                </entry>
                <entry name="status" outkey="12"/>
              </entry>
            </entry>
          </entry>
        </root><binary inputinstance="envelope.bin"/></data>
      </component>
      <component name="Result" library="xml" uid="3" kind="14">
        <properties XSLTDefaultOutput="1"/>
        <data><root>
          <entry name="FileInstance" expanded="1">
            <entry name="document" expanded="1">
              <entry name="Result" expanded="1">
                <entry name="Name" inpkey="21"/>
                <entry name="Status" inpkey="22"/>
              </entry>
            </entry>
          </entry>
        </root><document schema="result.xsd" outputinstance="result.xml" instanceroot="{}Result"/></data>
      </component>
    </children></structure>
    <connections>
      <edge from="11" to="21"/>
      <edge from="12" to="22"/>
    </connections>
  </component>
</mapping>
"#,
    )
    .unwrap();

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let options = imported.project.source_options.protobuf.as_ref().unwrap();
    assert_eq!(options.schema_path.as_deref(), Some("api/root.proto"));
    assert_eq!(options.imports.len(), 2);
    assert_eq!(
        options
            .imports
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["common/status.proto", "shared/model.proto"]
    );

    for directory in ["api", "shared", "common"] {
        std::fs::remove_dir_all(temp.0.join(directory)).unwrap();
    }
    let layout = embedded_layout(options);
    let input = Instance::Group(vec![
        (
            "record".to_string(),
            Instance::Group(vec![
                (
                    "name".to_string(),
                    Instance::Scalar(Value::String("portable".to_string())),
                ),
                (
                    "status".to_string(),
                    Instance::Scalar(Value::String("READY".to_string())),
                ),
            ]),
        ),
        (
            "status".to_string(),
            Instance::Scalar(Value::String("READY".to_string())),
        ),
    ]);
    let bytes = format_protobuf::to_vec(&layout, &options.root_message, &input).unwrap();
    let decoded = format_protobuf::from_slice(&layout, &options.root_message, &bytes).unwrap();
    let output = engine::run(&imported.project, &decoded).unwrap();
    assert_eq!(
        scalar(&output, "Name"),
        Some(&Value::String("portable".to_string()))
    );
    assert_eq!(scalar(&output, "Status"), Some(&Value::Int(1)));

    let exported_design = temp.0.join("exported/mapping.mfd");
    let warnings = mfd::export(&imported.project, &exported_design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported_base = temp.0.join("exported/mapping-source-protobuf");
    assert_eq!(
        std::fs::read_to_string(exported_base.join("api/root.proto")).unwrap(),
        root_source
    );
    assert_eq!(
        std::fs::read_to_string(exported_base.join("shared/model.proto")).unwrap(),
        model_source
    );
    assert_eq!(
        std::fs::read_to_string(exported_base.join("common/status.proto")).unwrap(),
        status_source
    );
    let exported_xml = std::fs::read_to_string(&exported_design).unwrap();
    assert!(exported_xml.contains("schemafile=\"mapping-source-protobuf/api/root.proto\""));

    let reimported = mfd::import(&exported_design).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(
        reimported.project.source_options.protobuf,
        imported.project.source_options.protobuf
    );
    assert_eq!(engine::run(&reimported.project, &decoded).unwrap(), output);
}

#[test]
fn protobuf_source_imports_exports_reimports_and_executes_equivalently() {
    let imported = mfd::import(&fixture("protobuf-source.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_eq!(
        imported.project.source_path.as_deref(),
        Some("directory.bin")
    );
    assert_eq!(
        imported.project.target_path.as_deref(),
        Some("protobuf-source-output.xml")
    );

    let options = imported.project.source_options.protobuf.as_ref().unwrap();
    assert_eq!(options.root_message, "ferrule.fixture.Directory");
    assert_eq!(
        options.schema,
        std::fs::read_to_string(fixture("protobuf-target.proto")).unwrap()
    );
    let layout = format_protobuf::Layout::parse(&options.schema).unwrap();
    let bytes =
        format_protobuf::to_vec(&layout, &options.root_message, &protobuf_directory()).unwrap();
    let source = format_protobuf::from_slice(&layout, &options.root_message, &bytes).unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    assert_eq!(
        scalar(&output, "Title"),
        Some(&Value::String("Imported".into()))
    );
    let records = output
        .field("Record")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(scalar(&records[0], "Code"), Some(&Value::Int(4)));
    assert_eq!(
        scalar(&records[1], "Label"),
        Some(&Value::String("Eight".into()))
    );

    let temp = TempDir::new();
    let design = temp.0.join("mapping.mfd");
    let warnings = mfd::export(&imported.project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        std::fs::read_to_string(temp.0.join("mapping-source.proto")).unwrap(),
        options.schema
    );
    let exported = std::fs::read_to_string(&design).unwrap();
    let document = roxmltree::Document::parse(&exported).unwrap();
    let component = document
        .descendants()
        .find(|node| {
            node.has_tag_name("component")
                && node.attribute("library") == Some("binary")
                && node.attribute("kind") == Some("33")
        })
        .unwrap();
    let binary = component
        .descendants()
        .find(|node| node.has_tag_name("binary"))
        .unwrap();
    assert_eq!(binary.attribute("inputinstance"), Some("directory.bin"));
    assert_eq!(binary.attribute("outputinstance"), None);
    assert!(component.descendants().any(|node| {
        node.has_tag_name("entry")
            && node.attribute("name") == Some("title")
            && node.attribute("outkey").is_some()
            && node.attribute("inpkey").is_none()
    }));

    let reimported = mfd::import(&design).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_eq!(reimported.project.source, imported.project.source);
    assert_eq!(
        reimported.project.source_options,
        imported.project.source_options
    );
    assert_eq!(reimported.project.source_path, imported.project.source_path);
    assert_eq!(engine::run(&reimported.project, &source).unwrap(), output);
}

#[test]
fn unsupported_protobuf_source_options_do_not_replace_existing_artifacts() {
    let mut imported = mfd::import(&fixture("protobuf-source.mfd")).unwrap();
    imported.project.source_options.delimiter = Some(';');
    let temp = TempDir::new();
    let design = temp.0.join("mapping.mfd");
    let schema = temp.0.join("mapping-source.proto");
    std::fs::write(&design, "keep this design").unwrap();
    std::fs::write(&schema, "keep this schema").unwrap();

    let result = mfd::export(&imported.project, &design);
    assert!(
        matches!(result, Err(mfd::MfdError::Unsupported(message)) if message.contains("a protobuf source cannot combine"))
    );
    assert_eq!(std::fs::read_to_string(design).unwrap(), "keep this design");
    assert_eq!(std::fs::read_to_string(schema).unwrap(), "keep this schema");
}

#[test]
fn imports_executes_and_encodes_proto2_target() {
    let imported = mfd::import(&fixture("protobuf-target.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_eq!(
        imported.project.target_path.as_deref(),
        Some("directory.bin")
    );

    let options = imported.project.target_options.protobuf.as_ref().unwrap();
    assert_eq!(options.root_message, "ferrule.fixture.Directory");
    assert_eq!(
        options.schema,
        std::fs::read_to_string(fixture("protobuf-target.proto")).unwrap()
    );
    let records = imported.project.target.child("records").unwrap();
    assert!(records.repeating);
    assert_eq!(
        records.child("code").map(|field| &field.kind),
        Some(&SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    );

    let source = format_xml::read(
        &fixture("protobuf-target-source.xml"),
        &imported.project.source,
    )
    .unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    assert_eq!(
        scalar(&target, "title"),
        Some(&Value::String("Demo".into()))
    );
    let rows = target
        .field("records")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(scalar(&rows[0], "code"), Some(&Value::Int(7)));
    assert_eq!(
        scalar(&rows[1], "label"),
        Some(&Value::String("Two".into()))
    );
    let notes = rows[0]
        .field("notes")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(scalar(&notes[0], "text"), Some(&Value::String("A".into())));

    let layout = format_protobuf::Layout::parse(&options.schema).unwrap();
    let bytes = format_protobuf::to_vec(&layout, &options.root_message, &target).unwrap();
    assert_eq!(
        bytes,
        vec![
            0x0a, 0x04, b'D', b'e', b'm', b'o', 0x12, 0x0e, 0x08, 0x07, 0x12, 0x03, b'O', b'n',
            b'e', 0x18, 0x01, 0x22, 0x03, 0x0a, 0x01, b'A', 0x12, 0x0e, 0x08, 0x09, 0x12, 0x03,
            b'T', b'w', b'o', 0x18, 0x00, 0x22, 0x03, 0x0a, 0x01, b'B',
        ]
    );
}

#[test]
fn protobuf_target_exports_reimports_and_preserves_encoded_output() {
    let imported = mfd::import(&fixture("protobuf-target.mfd")).unwrap();
    let source = format_xml::read(
        &fixture("protobuf-target-source.xml"),
        &imported.project.source,
    )
    .unwrap();
    let original_target = engine::run(&imported.project, &source).unwrap();
    let original_options = imported.project.target_options.protobuf.as_ref().unwrap();
    let original_layout = format_protobuf::Layout::parse(&original_options.schema).unwrap();
    let original_bytes = format_protobuf::to_vec(
        &original_layout,
        &original_options.root_message,
        &original_target,
    )
    .unwrap();

    let temp = TempDir::new();
    let design = temp.0.join("mapping.mfd");
    let warnings = mfd::export(&imported.project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        std::fs::read_to_string(temp.0.join("mapping-target.proto")).unwrap(),
        original_options.schema
    );
    let exported = std::fs::read_to_string(&design).unwrap();
    assert_eq!(exported.matches("library=\"binary\"").count(), 1);
    assert!(exported.contains("kind=\"33\""));
    assert!(exported.contains("type=\"doc-protobuf\""));

    let reimported = mfd::import(&design).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_eq!(reimported.project.target, imported.project.target);
    assert_eq!(
        reimported.project.target_options,
        imported.project.target_options
    );
    assert_eq!(reimported.project.target_path, imported.project.target_path);

    let reimported_target = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(reimported_target, original_target);
    let options = reimported.project.target_options.protobuf.as_ref().unwrap();
    let layout = format_protobuf::Layout::parse(&options.schema).unwrap();
    let bytes =
        format_protobuf::to_vec(&layout, &options.root_message, &reimported_target).unwrap();
    assert_eq!(bytes, original_bytes);
}

#[test]
fn invalid_protobuf_metadata_does_not_replace_existing_artifacts() {
    let mut imported = mfd::import(&fixture("protobuf-target.mfd")).unwrap();
    imported
        .project
        .target_options
        .protobuf
        .as_mut()
        .unwrap()
        .schema = "not a proto schema".to_string();
    let temp = TempDir::new();
    let design = temp.0.join("mapping.mfd");
    let schema = temp.0.join("mapping-target.proto");
    std::fs::write(&design, "keep this design").unwrap();
    std::fs::write(&schema, "keep this schema").unwrap();

    let result = mfd::export(&imported.project, &design);
    assert!(
        matches!(result, Err(mfd::MfdError::Unsupported(message)) if message.contains("embedded protobuf schema is invalid"))
    );
    assert_eq!(std::fs::read_to_string(design).unwrap(), "keep this design");
    assert_eq!(std::fs::read_to_string(schema).unwrap(), "keep this schema");
}
