use ir::{Instance, Value};
use std::path::{Path, PathBuf};
fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(n, v)| (n.into(), v))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn text(s: &str) -> Instance {
    Instance::Scalar(Value::String(s.into()))
}
fn wrap(schema: &ir::SchemaNode, value: Instance) -> Instance {
    if schema.repeating {
        Instance::Repeated(vec![value])
    } else {
        value
    }
}
fn source(schema: &ir::SchemaNode) -> Instance {
    let line = schema.child("Rows").unwrap().child("LIN").unwrap();
    group(vec![
        (
            "BEG",
            wrap(
                schema.child("BEG").unwrap(),
                group(vec![("F1", text("ORDER"))]),
            ),
        ),
        (
            "Rows",
            Instance::Repeated(vec![
                group(vec![(
                    "LIN",
                    wrap(line, group(vec![("F1", text("A")), ("F2", text("2.5"))])),
                )]),
                group(vec![(
                    "LIN",
                    wrap(
                        line,
                        group(vec![("F1", text("B")), ("F2", text("9007199254740993.5"))]),
                    ),
                )]),
            ]),
        ),
    ])
}
fn imported() -> mfd::ImportedX12DocumentBodies {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/x12_document_bodies");
    mfd::import_x12_document_bodies_with_options(
        &root.join("mapping.mfd"),
        &mfd::ImportOptions::default().with_package_root(&root),
    )
    .unwrap()
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule-x12-bodies-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/x12_document_bodies");
        for name in [
            "Envelope.Config",
            "850.Config",
            "Defs.Segment",
            "target.json",
            "mapping.mfd",
        ] {
            std::fs::copy(source.join(name), root.join(name)).unwrap();
        }
        Self(root)
    }
    fn change(&self, name: &str, change: impl FnOnce(String) -> String) {
        let path = self.0.join(name);
        std::fs::write(&path, change(std::fs::read_to_string(&path).unwrap())).unwrap();
    }
    fn import(&self) -> Result<mfd::ImportedX12DocumentBodies, mfd::MfdError> {
        mfd::import_x12_document_bodies_with_options(
            &self.0.join("mapping.mfd"),
            &mfd::ImportOptions::default().with_package_root(&self.0),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for name in [
            "Envelope.Config",
            "850.Config",
            "Defs.Segment",
            "target.json",
            "target.xsd",
            "mapping.mfd",
        ] {
            let _ = std::fs::remove_file(self.0.join(name));
        }
        let _ = std::fs::remove_dir(&self.0);
    }
}

fn context() -> engine::ExecutionContext<'static> {
    engine::ExecutionContext::new(Path::new("mapping.mfd")).with_invocation_counter_limits(
        functions::InvocationCounterLimits {
            max_counters: 4,
            max_text_bytes: 1024,
        },
    )
}

fn rust_options() -> codegen_rust::Options {
    codegen_rust::Options {
        package_name: "fixture_generated".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../codegen-runtime")
                .to_string_lossy()
                .into_owned(),
        ),
    }
}
#[test]
fn reads_original_ports_and_keeps_message_bodies_separate_from_filenames() {
    let imported = imported();
    assert_eq!(imported.input_parameter, "Input");
    assert_eq!(
        imported.message_path,
        ["Interchange", "Group", "Message_850"]
    );
    assert_eq!(imported.targets.len(), 1);
    let target = &imported.targets[0];
    assert_eq!(target.driver_path, ["BEG", "F1"]);
    assert!(target.is_default_output);
    let source = source(&target.body.source);
    let context = engine::ExecutionContext::new(Path::new("mapping.mfd"))
        .with_invocation_counter_limits(functions::InvocationCounterLimits {
            max_counters: 4,
            max_text_bytes: 1024,
        });
    let result = engine::run_with_context(&target.body, &source, &context).unwrap();
    let orders = result.field("orders").unwrap().as_repeated().unwrap();
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].field("number"), Some(&text("ORDER")));
    let lines = orders[0].field("lines").unwrap().as_repeated().unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[0].field("line"),
        Some(&Instance::Scalar(Value::decimal(
            ir::Decimal96::parse_exact("1").unwrap()
        )))
    );
    assert_eq!(
        lines[0].field("qty"),
        Some(&Instance::Scalar(Value::decimal(
            ir::Decimal96::parse_exact("3").unwrap()
        )))
    );
    assert_eq!(
        lines[1].field("qty"),
        Some(&Instance::Scalar(Value::decimal(
            ir::Decimal96::parse_exact("9007199254740994").unwrap()
        )))
    );
    let filename = engine::run(&target.filename, &source).unwrap();
    assert_eq!(filename.field("Path"), Some(&text("out-ORDER.json")));
    assert!(codegen::lower(&target.body).is_ok());
}
#[test]
fn configured_required_scalar_fails_only_when_the_body_is_executed() {
    let imported = imported();
    let schema = &imported.targets[0].body.source;
    let line = schema.child("Rows").unwrap().child("LIN").unwrap();
    let source = group(vec![
        (
            "BEG",
            wrap(
                schema.child("BEG").unwrap(),
                group(vec![("F1", text("ORDER"))]),
            ),
        ),
        (
            "Rows",
            Instance::Repeated(vec![group(vec![(
                "LIN",
                wrap(
                    line,
                    group(vec![
                        ("F1", text("A")),
                        ("F2", Instance::Scalar(Value::Null)),
                    ]),
                ),
            )])]),
        ),
    ]);
    assert!(engine::run(&imported.targets[0].filename, &source).is_ok());
    let context = engine::ExecutionContext::new(Path::new("mapping.mfd"))
        .with_invocation_counter_limits(functions::InvocationCounterLimits {
            max_counters: 4,
            max_text_bytes: 1024,
        });
    assert!(engine::run_with_context(&imported.targets[0].body, &source, &context).is_err());
}

#[test]
fn singleton_driver_absence_does_not_borrow_another_row_or_advance_the_counter() {
    let imported = imported();
    let target = &imported.targets[0];
    let input = source(&target.body.source);
    let valid = input.field("Rows").unwrap().as_repeated().unwrap()[0].clone();
    let input = group(vec![
        ("BEG", input.field("BEG").unwrap().clone()),
        (
            "Rows",
            Instance::Repeated(vec![valid.clone(), group(vec![]), valid]),
        ),
    ]);
    let context = context();
    // Interpreter calls are separate invocations. Generated document hosts keep
    // one invocation context while visiting their messages instead.
    for _ in 0..2 {
        let output = engine::run_with_context(&target.body, &input, &context).unwrap();
        let lines = output.field("orders").unwrap().as_repeated().unwrap()[0]
            .field("lines")
            .unwrap()
            .as_repeated()
            .unwrap();
        assert_eq!(lines.len(), 2);
        for (index, line) in lines.iter().enumerate() {
            assert_eq!(
                line.field("line").unwrap().as_scalar().unwrap(),
                &Value::decimal(ir::Decimal96::parse_exact(&(1 + index).to_string()).unwrap())
            );
        }
    }
}

#[test]
fn json_reduction_evaluates_all_matching_dates_but_not_nonmatching_dates() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.0.join("target.json"),
        r#"{"type":"object","properties":{"date":{"type":"string"}}}"#,
    )
    .unwrap();
    fixture.change("mapping.mfd", |xml| {
        let start = xml.find("<component name=\"Target\"").unwrap();
        let end = start + xml[start..].find("</component>").unwrap() + "</component>".len();
        let xml = format!("{}{}{}", &xml[..start], r#"<component name="Target" library="json" kind="31" uid="11"><properties DynamicFilenames="1"/><data><root><entry name="FileInstance" inpkey="40"><entry name="document"><entry name="root"><entry name="object"><entry name="date" type="json-property"><entry name="string" inpkey="48"/></entry></entry></entry></entry></entry></root><json schema="target.json"/></data></component>"#, &xml[end..]);
        let extra = r#"
<component name="constant" library="core" kind="2" uid="12"><targets><datapoint pos="0" key="80"/></targets><data><constant value="[Y0001]-[M01]-[D01]" datatype="string"/><wsdl/></data></component>
<component name="constant" library="core" kind="2" uid="13"><targets><datapoint pos="0" key="81"/></targets><data><constant value="OK" datatype="string"/><wsdl/></data></component>
<component name="format-dateTime" library="core" kind="5" uid="14"><sources><datapoint pos="0" key="91"/><datapoint pos="1" key="92"/><datapoint/></sources><targets><datapoint pos="0" key="90"/></targets></component>
<component name="equal" library="core" kind="5" uid="15"><sources><datapoint pos="0" key="94"/><datapoint pos="1" key="95"/></sources><targets><datapoint pos="0" key="93"/></targets></component>
<component name="if-else" library="core" kind="4" uid="16"><sources><datapoint pos="0" key="97"/><datapoint pos="1" key="98"/><datapoint/></sources><targets><datapoint pos="0" key="96"/></targets></component>
"#;
        let mut xml = xml.replace("</children>", &format!("{extra}</children>"))
            .replace("<edge vertexkey=\"54\"/>", "<edge vertexkey=\"54\"/><edge vertexkey=\"91\"/>")
            .replace("<edge vertexkey=\"46\"/>", "<edge vertexkey=\"94\"/>")
            .replace("</vertices>", r#"<vertex vertexkey="80"><edges><edge vertexkey="92"/></edges></vertex><vertex vertexkey="81"><edges><edge vertexkey="95"/></edges></vertex><vertex vertexkey="90"><edges><edge vertexkey="98"/></edges></vertex><vertex vertexkey="93"><edges><edge vertexkey="97"/></edges></vertex><vertex vertexkey="96"><edges><edge vertexkey="48"/></edges></vertex></vertices>"#);
        for port in [41,42,43,44,45,47] { xml = xml.replace(&format!("<edge vertexkey=\"{port}\"/>"), ""); }
        xml
    });
    let imported = fixture.import().unwrap();
    let target = &imported.targets[0];
    let row = |qualifier: &str, date: &str| {
        group(vec![(
            "LIN",
            group(vec![("F1", text(qualifier)), ("F2", text(date))]),
        )])
    };
    for (second, succeeds) in [("OTHER", true), ("OK", false)] {
        let source = group(vec![(
            "Rows",
            Instance::Repeated(vec![row("OK", "2026-10-23"), row(second, "invalid")]),
        )]);
        let result = engine::run(&target.body, &source);
        if succeeds {
            assert_eq!(result.unwrap().field("date"), Some(&text("2026-10-23")));
        } else {
            assert!(result.is_err());
        }
    }
    assert!(codegen::lower(&target.body).is_ok());
}

#[test]
fn xml_scalar_occurrences_retain_schema_and_native_negative_zero() {
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join("target.xsd"), r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Order"><xs:complexType><xs:sequence><xs:element name="Code" type="xs:string"/><xs:element name="Quantity" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#).unwrap();
    fixture.change("mapping.mfd", |xml| xml.replace("</children>", r#"<component name="XmlTarget" library="xml" kind="14" uid="12"><properties DynamicFilenames="1"/><data><root><entry name="FileInstance" inpkey="100"><entry name="document"><entry name="Order"><entry name="Code" inpkey="101"/><entry name="Quantity" inpkey="102"/></entry></entry></entry></root><document schema="target.xsd" instanceroot="{}Order"/></data></component></children>"#)
        .replace("<edge vertexkey=\"40\"/>", "<edge vertexkey=\"40\"/><edge vertexkey=\"100\"/>")
        .replace("<edge vertexkey=\"46\"/>", "<edge vertexkey=\"46\"/><edge vertexkey=\"101\"/>")
        .replace("<edge vertexkey=\"47\"/>", "<edge vertexkey=\"47\"/><edge vertexkey=\"102\"/>"));
    let imported = fixture.import().unwrap();
    let target = &imported.targets[1];
    assert_eq!(target.format, mfd::X12DocumentBodyFormat::Xml);
    assert!(target.eager_json_project("ResolvedFilename").is_err());
    assert!(!target.body.target.child("Code").unwrap().repeating);
    let row = |code, qty| {
        group(vec![(
            "LIN",
            group(vec![("F1", text(code)), ("F2", text(qty))]),
        )])
    };
    let input = group(vec![(
        "Rows",
        Instance::Repeated(vec![row("A", "-0.1"), row("B", "2.5")]),
    )]);
    let result = engine::run(&target.body, &input).unwrap();
    assert_eq!(
        result.field("Code").unwrap().as_mapped_sequence().unwrap(),
        &[text("A"), text("B")]
    );
    assert_eq!(
        result
            .field("Quantity")
            .unwrap()
            .as_mapped_sequence()
            .unwrap(),
        &[text("-0"), text("3")]
    );
    assert!(codegen::lower(&target.body).is_ok());
    let before = serde_json::to_vec(&target.body).unwrap();
    let document = imported
        .deferred_xml_document(&target.name, "ResolvedFilename", &[vec!["Quantity".into()]])
        .unwrap();
    let program = codegen::lower_document(&document).unwrap();
    assert_eq!(program.program().extra_targets.len(), 3);
    assert_eq!(serde_json::to_vec(&target.body).unwrap(), before);
    let source = codegen::NativeX12Source {
        schema: &imported.source_schema,
        options: &imported.source_options,
        message: &imported.message_path,
        encoding: imported.source_encoding,
        structure: Some(&imported.source_structure),
    };
    assert!(
        codegen::prepare_deferred_x12_xml(
            program.program(),
            "ResolvedFilename",
            &target.driver_path
        )
        .is_ok()
    );
    // This fixture deliberately has a reduced physical envelope. Phasing must
    // not weaken the native source companion's structural admission.
    assert!(matches!(
        codegen_rust::emit_with_native_deferred_x12_xml(
            &program,
            &rust_options(),
            &source,
            "ResolvedFilename",
            &target.driver_path
        ),
        Err(codegen_rust::MapForceSourceEmitError::Boundary(_))
    ));
    assert!(matches!(
        codegen_csharp::emit_with_native_deferred_x12_xml(
            &program,
            &source,
            "ResolvedFilename",
            &target.driver_path
        ),
        Err(codegen_csharp::MapForceSourceEmitError::Boundary(_))
    ));
    assert!(
        target
            .deferred_xml_project("ResolvedFilename", &[vec!["Missing".into()]])
            .is_err()
    );
}

#[test]
fn eager_json_join_preserves_inputs_and_refuses_graph_identity_conflicts() {
    let mut imported = imported();
    let target = &mut imported.targets[0];
    let before = serde_json::to_vec(&target.body).unwrap();
    let before_name = serde_json::to_vec(&target.filename).unwrap();
    let joined = target.eager_json_project("ResolvedFilename").unwrap();
    assert_eq!(joined.extra_targets.len(), 1);
    assert_eq!(serde_json::to_vec(&target.body).unwrap(), before);
    assert_eq!(serde_json::to_vec(&target.filename).unwrap(), before_name);
    let shared = *target
        .body
        .graph
        .nodes
        .iter()
        .find(|(id, node)| {
            matches!(node, mapping::Node::SourceField { .. })
                && target.filename.graph.nodes.contains_key(*id)
        })
        .unwrap()
        .0;
    target.filename.graph.nodes.insert(
        shared,
        mapping::Node::Const {
            value: Value::Bool(false),
        },
    );
    assert!(target.eager_json_project("ResolvedFilename").is_err());
}

#[test]
fn unsupported_metadata_and_port_mutations_are_refused() {
    for (before, after) in [
        ("key=\"53\"", "key=\"52\""),
        ("pos=\"2\" key=\"53\"", "pos=\"1\" key=\"53\""),
        ("vertexkey=\"47\"", "vertexkey=\"999\""),
        ("value=\"#\"", "value=\"0.0\""),
        ("DynamicFilenames=\"1\"", "DynamicFilenames=\"0\""),
        ("cast-in-subtree", "cast-scalar-only"),
        (
            "<entry name=\"lines\"",
            "<entry casttotargettypemode=\"cast-in-subtree\" name=\"lines\"",
        ),
    ] {
        let fixture = Fixture::new();
        fixture.change("mapping.mfd", |xml| {
            assert!(xml.contains(before));
            xml.replace(before, after)
        });
        assert!(fixture.import().is_err(), "admitted {before} -> {after}");
    }
}

#[test]
fn source_encoding_comes_from_the_original_component() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.import().unwrap().source_encoding,
        mapping::X12SourceEncoding::Utf8
    );
    fixture.change("mapping.mfd", |text| {
        text.replace(
            "<text type=\"edi\"",
            "<text encoding=\"1\" byteorder=\"1\" byteordermark=\"0\" type=\"edi\"",
        )
    });
    assert_eq!(
        fixture.import().unwrap().source_encoding,
        mapping::X12SourceEncoding::Latin1
    );
    fixture.change("mapping.mfd", |text| {
        text.replace("encoding=\"1\"", "encoding=\"99999\"")
    });
    assert!(fixture.import().is_err());
}

#[test]
fn native_source_structure_retains_bounds_independently_of_schema_lists() {
    let imported = imported();
    let root = &imported.source_structure;
    root.validate(&imported.source_schema).unwrap();
    assert_eq!(root.name, "Envelope");
    assert_eq!(root.children[0].maximum, None);
    let isa = &root.children[0].children[0];
    assert_eq!(isa.segment.as_deref(), Some("ISA"));
    assert_eq!((isa.minimum, isa.maximum), (0, Some(1)));
    assert!(root.known_segments.iter().any(|name| name == "ISA"));
    assert!(root.children[0].children[1].children[1].selection);
    assert!(
        imported
            .source_schema
            .child("Interchange")
            .unwrap()
            .child("ISA")
            .unwrap()
            .repeating
    );
    let fixture = Fixture::new();
    fixture.change("Envelope.Config", |text| {
        text.replace(
            "ref=\"ISA\" minOccurs=\"0\"",
            "ref=\"ISA\" minOccurs=\"0\" maxOccurs=\"invalid\"",
        )
    });
    assert!(fixture.import().is_err());
}

#[test]
fn unsupported_native_source_encoding_flags_are_refused() {
    for settings in [
        "encoding=\"1\" byteorder=\"2\" byteordermark=\"0\"",
        "encoding=\"1\" byteorder=\"1\" byteordermark=\"1\"",
    ] {
        let fixture = Fixture::new();
        fixture.change("mapping.mfd", |text| {
            text.replace(
                "<text type=\"edi\"",
                &format!("<text {settings} type=\"edi\""),
            )
        });
        assert!(fixture.import().is_err());
    }
    let fixture = Fixture::new();
    fixture.change("mapping.mfd", |text| {
        text.replace("<text type=\"edi\"", "<text encoding=\"1000\" type=\"edi\"")
    });
    assert_eq!(
        fixture.import().unwrap().source_encoding,
        mapping::X12SourceEncoding::Utf8
    );
}

#[test]
fn required_metadata_is_explicit_and_does_not_change_writer_minimums() {
    let fixture = Fixture::new();
    let selected = ["850".to_string()];
    let config = fixture.0.join("Envelope.Config");
    let fields = format_edi::config::import_mapforce_source_minimums_confined(
        &config, &fixture.0, &selected,
    )
    .unwrap();
    assert!(
        fields
            .iter()
            .any(|m| m.path.ends_with(&["LIN".into(), "F2".into()]) && m.minimum == 1)
    );
    let writer =
        format_edi::config::import_mapforce_write_minimums_confined(&config, &fixture.0, &selected)
            .unwrap();
    assert!(
        !writer
            .iter()
            .any(|m| m.path.last().is_some_and(|p| p == "F2"))
    );
    fixture.change("Defs.Segment", |text| {
        text.replace("<Data ref=\"F2\"/>", "<Data ref=\"F2\" minOccurs=\"bad\"/>")
    });
    assert!(fixture.import().is_err());
    assert_eq!(
        format_edi::config::import_mapforce_write_minimums_confined(&config, &fixture.0, &selected)
            .unwrap(),
        writer
    );
}

#[test]
fn connected_hidden_wrappers_and_cyclic_drivers_are_not_silently_dropped() {
    let fixture = Fixture::new();
    fixture.change("mapping.mfd", |xml| {
        xml.replace(
            "name=\"header\" type=\"json-property\"",
            "name=\"header\" inpkey=\"99\" type=\"json-property\"",
        )
        .replace(
            "<edge vertexkey=\"41\"/>",
            "<edge vertexkey=\"41\"/><edge vertexkey=\"99\"/>",
        )
    });
    assert!(fixture.import().is_err());
    let fixture = Fixture::new();
    fixture.change("mapping.mfd", |xml| xml
        .replace("</children>", r#"<component name="Rows" library="core" kind="3" uid="12"><sources><datapoint pos="0" key="91"/><datapoint pos="1" key="92"/></sources><targets><datapoint pos="0" key="90"/><datapoint/></targets></component></children>"#)
        .replace("<edge vertexkey=\"42\"/>", "")
        .replace("<edge vertexkey=\"41\"/>", "<edge vertexkey=\"41\"/><edge vertexkey=\"92\"/>")
        .replace("</vertices>", r#"<vertex vertexkey="90"><edges><edge vertexkey="91"/><edge vertexkey="42"/></edges></vertex></vertices>"#));
    assert!(fixture.import().is_err());
}

#[test]
fn generated_source_companion_binds_the_complete_physical_schema_to_its_message() {
    let fixture = Fixture::new();
    fixture.change("850.Config", |xml| {
        xml.replace(
            "<Segment ref=\"ST\" minOccurs=\"0\"/>",
            "<Segment ref=\"ST\"/>",
        )
        .replace(
            "<Segment ref=\"SE\" minOccurs=\"0\"/>",
            "<Segment ref=\"SE\"/>",
        )
    });
    fixture.change("Defs.Segment", |mut xml| {
        for (name, count) in [
            ("ISA", 16),
            ("GS", 8),
            ("ST", 2),
            ("SE", 2),
            ("GE", 2),
            ("IEA", 2),
        ] {
            let old = format!(
                "<Segment name=\"{name}\"><Data ref=\"{}\"/></Segment>",
                if name == "ST" { "F143" } else { "F1" }
            );
            let data = (1..=count)
                .map(|n| format!("<Data ref=\"F1\" nodeName=\"E{n:02}\"/>"))
                .collect::<String>();
            assert!(xml.contains(&old));
            xml = xml.replace(&old, &format!("<Segment name=\"{name}\">{data}</Segment>"));
        }
        xml
    });
    // The discriminator follows the renamed ST field in the neutral schema.
    fixture.change("Envelope.Config", |xml| xml.replace("ST/F143", "ST/E01"));
    fixture.change("mapping.mfd", |xml| {
        xml.replace(
            "<messages>",
            "<settings interchangecontrolversionnumber=\"00401\"/><messages>",
        )
    });
    let imported = fixture.import().unwrap();
    let program = codegen::lower(&imported.targets[0].body).unwrap();
    let descriptor = codegen::prepare_mapforce_x12_source(
        &program,
        &imported.source_schema,
        &imported.source_options,
        &imported.message_path,
    )
    .unwrap();
    assert!(descriptor.contains("mapforce_interchanges"));
    let csharp = codegen_csharp::emit_with_mapforce_x12_source(
        &program,
        &imported.source_schema,
        &imported.source_options,
        &imported.message_path,
    )
    .unwrap();
    assert!(
        csharp
            .into_files()
            .iter()
            .any(|file| file.path.as_str() == "Runtime/X12/FerruleX12.SourceLexical.cs")
    );
    let strict = codegen_csharp::emit_with_mapforce_x12_json(
        &program,
        &imported.source_schema,
        &imported.source_options,
        &imported.message_path,
    )
    .unwrap()
    .into_files();
    assert!(
        strict
            .iter()
            .any(|file| file.path.as_str() == "Runtime/Json5/FerruleMapForceJson5.cs")
    );
    assert!(
        strict
            .iter()
            .any(|file| file.path.as_str() == "GeneratedMapping.cs"
                && String::from_utf8_lossy(&file.contents).contains("SerializeMapForceJson("))
    );
    let strict = codegen_rust::emit_with_mapforce_x12_json(
        &program,
        &rust_options(),
        &imported.source_schema,
        &imported.source_options,
        &imported.message_path,
    )
    .unwrap()
    .into_files();
    assert!(strict.iter().any(|file| file.path.as_str() == "src/lib.rs"
        && String::from_utf8_lossy(&file.contents).contains("pub fn serialize_mapforce_json(")));
    let joined = imported.targets[0]
        .eager_json_project("ResolvedFilename")
        .unwrap();
    let combined = codegen::lower(&joined).unwrap();
    assert!(
        codegen::prepare_eager_x12_json_message(
            &combined,
            "ResolvedFilename",
            &imported.targets[0].driver_path
        )
        .is_ok()
    );
    assert!(
        codegen::prepare_eager_x12_json_message(
            &combined,
            "WrongName",
            &imported.targets[0].driver_path
        )
        .is_err()
    );
    let files = codegen_csharp::emit_with_eager_x12_json_message(
        &combined,
        &imported.source_schema,
        &imported.source_options,
        &imported.message_path,
        "ResolvedFilename",
        &imported.targets[0].driver_path,
    )
    .unwrap()
    .into_files();
    assert!(
        files
            .iter()
            .any(|file| file.path.as_str() == "GeneratedMapping.cs"
                && String::from_utf8_lossy(&file.contents)
                    .contains("PreparePrimaryX12JsonMessage"))
    );
    let files = codegen_rust::emit_with_eager_x12_json_message(
        &combined,
        &rust_options(),
        &imported.source_schema,
        &imported.source_options,
        &imported.message_path,
        "ResolvedFilename",
        &imported.targets[0].driver_path,
    )
    .unwrap()
    .into_files();
    assert!(files.iter().any(|file| file.path.as_str() == "src/lib.rs"
        && String::from_utf8_lossy(&file.contents).contains("prepare_primary_x12_json_message")));
    let document = imported
        .eager_json_document(&imported.targets[0].name, "ResolvedFilename")
        .unwrap();
    assert_eq!(
        document.global_filename().unwrap(),
        mapping::GlobalFilename::Parameter("Input")
    );
    let document_program = codegen::lower_document(&document).unwrap();
    let files = codegen_rust::emit_with_primary_eager_x12_json(
        &document_program,
        &rust_options(),
        &imported.source_schema,
        &imported.source_options,
        &imported.message_path,
        "ResolvedFilename",
        &imported.targets[0].driver_path,
    )
    .unwrap()
    .into_files();
    assert!(files.iter().any(|file| file.path.as_str() == "src/lib.rs"
        && String::from_utf8_lossy(&file.contents).contains("pub fn acquire_primary_x12")));
    let files = codegen_csharp::emit_with_primary_eager_x12_json(
        &document_program,
        &imported.source_schema,
        &imported.source_options,
        &imported.message_path,
        "ResolvedFilename",
        &imported.targets[0].driver_path,
    )
    .unwrap()
    .into_files();
    assert!(
        files
            .iter()
            .any(|file| file.path.as_str() == "GeneratedMapping.cs"
                && String::from_utf8_lossy(&file.contents).contains("AcquirePrimaryX12("))
    );
    let mut wrong = program.clone();
    wrong.source.name = "UnrelatedMessage".into();
    assert!(
        codegen::prepare_mapforce_x12_source(
            &wrong,
            &imported.source_schema,
            &imported.source_options,
            &imported.message_path
        )
        .is_err()
    );
    assert!(
        codegen::prepare_mapforce_x12_source(
            &program,
            &imported.source_schema,
            &imported.source_options,
            &["missing".into()]
        )
        .is_err()
    );
}
