use ir::{Instance, Value};
use std::path::PathBuf;

#[path = "grouped_json_edi_documents/compiled.rs"]
mod compiled;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "grouped-json-edi-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/json_edi_documents");
        for name in [
            "mapping.mfd",
            "source.json",
            "Envelope.Config",
            "850.Config",
            "Defs.Segment",
        ] {
            std::fs::copy(fixture.join(name), root.join(name)).unwrap();
        }
        let text = std::fs::read_to_string(root.join("mapping.mfd")).unwrap()
            .replace("</children>",r#"<component name="group-by" library="core" kind="5" uid="30"><sources><datapoint pos="0" key="101"/><datapoint pos="1" key="102"/></sources><targets><datapoint/><datapoint pos="1" key="103"/></targets></component><component name="concat" library="core" kind="5" uid="31"><sources><datapoint pos="0" key="104"/><datapoint pos="1" key="105"/></sources><targets><datapoint pos="0" key="106"/></targets></component></children>"#)
            .replace(r#"<vertex vertexkey="2"><edges><edge vertexkey="80"/></edges></vertex>"#,r#"<vertex vertexkey="2"><edges><edge vertexkey="104"/></edges></vertex><vertex vertexkey="103"><edges><edge vertexkey="105"/></edges></vertex><vertex vertexkey="106"><edges><edge vertexkey="80"/><edge vertexkey="81"/><edge vertexkey="82"/></edges></vertex><vertex vertexkey="20"><edges><edge vertexkey="101"/></edges></vertex><vertex vertexkey="21"><edges><edge vertexkey="102"/></edges></vertex>"#)
            .replace(r#"<vertex vertexkey="11"><edges><edge vertexkey="81"/><edge vertexkey="82"/></edges></vertex>"#, "");
        std::fs::write(root.join("mapping.mfd"), text).unwrap();
        Self(root)
    }
    fn change(&self, from: &str, to: &str) {
        let path = self.0.join("mapping.mfd");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(from));
        std::fs::write(path, text.replace(from, to)).unwrap();
    }
    fn import(&self) -> Result<mfd::ImportedGroupedJsonEdiDocument, mfd::MfdError> {
        mfd::import_grouped_json_edi_document_with_options(
            &self.0.join("mapping.mfd"),
            &mfd::ImportOptions::default().with_package_root(&self.0),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for name in [
            "mapping.mfd",
            "source.json",
            "Envelope.Config",
            "850.Config",
            "Defs.Segment",
        ] {
            std::fs::remove_file(self.0.join(name)).unwrap();
        }
        let variables = self.0.join("variables.json");
        if variables.exists() {
            std::fs::remove_file(variables).unwrap();
        }
        std::fs::remove_dir(&self.0).unwrap();
    }
}

fn invoked_fixture() -> Fixture {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.0.join("variables.json"),
        r#"{"type":"object","properties":{"projectCode":{"type":"string"}}}"#,
    )
    .unwrap();
    fixture.change("</children>", r#"<component name="Variables" library="core" uid="40" kind="6"><sources><datapoint/></sources><targets><datapoint pos="0" key="140"/></targets><data><input datatype="string"/><parameter usageKind="input" name="Variables"/></data></component>
<component name="Parameter" library="json" uid="41" kind="31"><properties XSLTTargetEncoding="UTF-8"/><data><root><entry name="FileInstance" inpkey="141"><entry name="document"><entry name="root"><entry name="object" outkey="142"><entry name="projectCode" type="json-property"><entry name="string" outkey="143"/></entry></entry></entry></entry></entry></root><parameter usageKind="stringparse"/><json schema="variables.json"/></data></component>
<component name="group-by" library="core" uid="42" kind="5"><sources><datapoint pos="0" key="144"/><datapoint pos="1" key="145"/></sources><targets><datapoint/><datapoint pos="1" key="146"/></targets></component>
<component name="concat" library="core" uid="43" kind="5"><sources><datapoint pos="0" key="147"/><datapoint pos="1" key="148"/></sources><targets><datapoint pos="0" key="149"/></targets></component></children>"#);
    fixture.change(r#"<vertex vertexkey="106"><edges><edge vertexkey="80"/><edge vertexkey="81"/><edge vertexkey="82"/></edges></vertex>"#, r#"<vertex vertexkey="106"><edges><edge vertexkey="148"/></edges></vertex><vertex vertexkey="149"><edges><edge vertexkey="80"/><edge vertexkey="81"/><edge vertexkey="82"/></edges></vertex>"#);
    fixture.change("</vertices>", r#"<vertex vertexkey="140"><edges><edge vertexkey="141"/></edges></vertex><vertex vertexkey="142"><edges><edge vertexkey="144"/></edges></vertex><vertex vertexkey="143"><edges><edge vertexkey="145"/></edges></vertex><vertex vertexkey="146"><edges><edge vertexkey="147"/></edges></vertex></vertices>"#);
    fixture
}

#[test]
fn explicit_invoked_import_keeps_parameter_and_physical_groups_separate() {
    let fixture = invoked_fixture();
    let original = std::fs::read(fixture.0.join("mapping.mfd")).unwrap();
    let imported = mfd::import_invoked_grouped_json_edi_document_with_options(
        &fixture.0.join("mapping.mfd"),
        &mfd::ImportOptions::default().with_package_root(&fixture.0),
    )
    .unwrap();
    assert_eq!(imported.invocation.parameter, "Variables");
    assert_eq!(imported.invocation.key_member, "projectCode");
    assert_eq!(
        imported.invocation.iteration,
        mapping::JsonParameterIteration::DistinctRootStringKeys
    );
    assert_eq!(imported.parameter_source.input_port, 141);
    assert_eq!(imported.source.filename_input_port, 10);
    let lowered = codegen::lower_document(&imported.document).unwrap();
    assert!(codegen::prepare_json_parameter_invocation(&lowered, &imported.invocation).is_ok());
    let mut wrong_binding = imported.invocation.clone();
    wrong_binding.key_parameter = "DifferentInput".into();
    assert!(codegen::prepare_json_parameter_invocation(&lowered, &wrong_binding).is_err());
    let mut body_parameter = imported.document.project().clone();
    let key_node = body_parameter.graph.nodes.iter().find_map(|(id, node)| {
        matches!(node, mapping::Node::RuntimeParameter { name, .. } if name == &imported.invocation.key_parameter).then_some(*id)
    }).unwrap();
    let filter_node = body_parameter.graph.nodes.keys().max().unwrap() + 1;
    body_parameter.graph.nodes.insert(
        filter_node,
        mapping::Node::Call {
            function: "exists".into(),
            args: vec![key_node],
        },
    );
    body_parameter.root.filter = Some(filter_node);
    let guarded =
        mapping::DocumentProject::new(body_parameter, imported.document.primary_source().clone())
            .with_json_grouping_preparation(
                imported.source.shape.clone(),
                imported
                    .document
                    .json_grouping_preparation()
                    .unwrap()
                    .clone(),
            )
            .unwrap();
    let unsafe_program = codegen::lower_document(&guarded).unwrap();
    assert!(
        codegen::prepare_json_parameter_invocation(&unsafe_program, &imported.invocation).is_err()
    );
    let plan = imported.invocation;
    let grouped = mfd::ImportedGroupedJsonEdiDocument {
        document: imported.document,
        source: imported.source,
        writer_minimums: imported.writer_minimums,
    };
    let prepared = codegen_runtime::PreparedJsonParameterInvocations::prepare(
        &plan,
        &Value::String(
            r#"{"projectCode":"P","projectCode":4,"projectCode":"P","projectCode":"Q"}"#.into(),
        ),
        Default::default(),
        Default::default(),
    )
    .unwrap();
    let mut paths = Vec::new();
    prepared
        .visit::<engine::EngineError>(|invocation| {
            let mut parameters = engine::RuntimeParameters::new();
            let (name, value) = invocation.key_binding();
            parameters.insert(name, value).unwrap();
            let context = engine::ExecutionContext::new(&fixture.0).with_parameters(&parameters);
            let source = input(
                &grouped,
                &[r#"{"rows":[{"name":"a","qty":1},{"name":"b","qty":2}]}"#],
            );
            let ir::Instance::DocumentSet(documents) =
                engine::run_with_context(grouped.document.project(), &source, &context)?
            else {
                panic!("document set");
            };
            paths.extend(documents.iter().map(|document| document.path().to_owned()));
            Ok(())
        })
        .unwrap();
    assert_eq!(paths, ["Pout.edia", "Pout.edib", "Qout.edia", "Qout.edib"]);
    assert!(fixture.import().is_err());
    assert_eq!(
        std::fs::read(fixture.0.join("mapping.mfd")).unwrap(),
        original
    );
}

#[test]
fn invoked_import_refuses_unqualified_parameter_consumers_and_types() {
    for (from, to) in [
        (
            r#"<entry name="string" outkey="143"/>"#,
            r#"<entry name="number" outkey="143"/>"#,
        ),
        (
            r#"<vertex vertexkey="143"><edges><edge vertexkey="145"/></edges></vertex>"#,
            r#"<vertex vertexkey="143"><edges><edge vertexkey="145"/><edge vertexkey="85"/></edges></vertex>"#,
        ),
        (
            r#"<vertex vertexkey="21"><edges><edge vertexkey="85"/></edges></vertex>"#,
            r#"<vertex vertexkey="146"><edges><edge vertexkey="85"/></edges></vertex>"#,
        ),
        (
            r#"name="Variables"/></data>"#,
            r#"name="__ferrule_parameter_group_146"/></data>"#,
        ),
    ] {
        let fixture = invoked_fixture();
        fixture.change(from, to);
        assert!(
            mfd::import_invoked_grouped_json_edi_document_with_options(
                &fixture.0.join("mapping.mfd"),
                &mfd::ImportOptions::default().with_package_root(&fixture.0)
            )
            .is_err(),
            "accepted {to}"
        );
    }
}

#[test]
fn invoked_import_does_not_drop_additional_document_targets() {
    for target in [
        r#"<component name="ExtraTarget" library="json" kind="31" uid="94"><data><root><entry name="FileInstance" inpkey="194"><entry name="document"><entry name="root"><entry name="object" inpkey="195"/></entry></entry></entry></root></data></component>"#,
        r#"<component name="ExtraOutput" library="core" kind="7" uid="94"><sources><datapoint pos="0" key="194"/></sources><data><output datatype="string"/><parameter usageKind="output" name="ExtraOutput"/></data></component>"#,
    ] {
        let fixture = invoked_fixture();
        fixture.change("</children>", &format!("{target}</children>"));
        assert!(
            mfd::import_invoked_grouped_json_edi_document_with_options(
                &fixture.0.join("mapping.mfd"),
                &mfd::ImportOptions::default().with_package_root(&fixture.0),
            )
            .is_err()
        );
    }
}

#[test]
fn disconnected_embedded_palette_instances_do_not_admit_connected_calls() {
    let fixture = invoked_fixture();
    fixture.change("</children>", r#"<component name="dormant" library="unknown" uid="99" kind="19"><data><root><entry name="argument" componentid="1"/></root><root><entry name="result" componentid="2"/></root></data></component></children>"#);
    let import = || {
        mfd::import_invoked_grouped_json_edi_document_with_options(
            &fixture.0.join("mapping.mfd"),
            &mfd::ImportOptions::default().with_package_root(&fixture.0),
        )
    };
    assert!(import().is_ok());
    fixture.change(
        r#"name="argument" componentid="1""#,
        r#"name="argument" inpkey="999" componentid="1""#,
    );
    assert!(import().is_err());
}

#[test]
fn invoked_array_groups_retain_all_items_for_each_distinct_item_key() {
    let fixture = invoked_fixture();
    fixture.change(
        r#"<entry name="array">"#,
        r#"<entry name="array" outkey="201">"#,
    );
    fixture.change(
        r#"<vertex vertexkey="20"><edges><edge vertexkey="101"/></edges></vertex>"#,
        r#"<vertex vertexkey="201"><edges><edge vertexkey="101"/></edges></vertex>"#,
    );
    let imported = mfd::import_invoked_grouped_json_edi_document_with_options(
        &fixture.0.join("mapping.mfd"),
        &mfd::ImportOptions::default().with_package_root(&fixture.0),
    )
    .unwrap();
    let mut parameters = engine::RuntimeParameters::new();
    parameters
        .insert(
            &imported.invocation.key_parameter,
            Value::String("P".into()),
        )
        .unwrap();
    let context = engine::ExecutionContext::new(&fixture.0).with_parameters(&parameters);
    let grouped = mfd::ImportedGroupedJsonEdiDocument {
        document: imported.document,
        source: imported.source,
        writer_minimums: imported.writer_minimums,
    };
    let source = input(
        &grouped,
        &[r#"{"rows":[{"name":"a","qty":1},{"name":"b","qty":2}]}"#],
    );
    let ir::Instance::DocumentSet(documents) =
        engine::run_with_context(grouped.document.project(), &source, &context).unwrap()
    else {
        panic!("document set");
    };
    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].path(), "Pout.edia");
    assert_eq!(documents[1].path(), "Pout.edib");
    assert_eq!(documents[0].value(), documents[1].value());
    assert!(fixture.import().is_err());
}

#[test]
fn invoked_direct_scalar_selection_keeps_writer_first_nonempty() {
    let fixture = invoked_fixture();
    let imported = mfd::import_invoked_grouped_json_edi_document_with_options(
        &fixture.0.join("mapping.mfd"),
        &mfd::ImportOptions::default().with_package_root(&fixture.0),
    )
    .unwrap();
    let mut parameters = engine::RuntimeParameters::new();
    parameters
        .insert(
            &imported.invocation.key_parameter,
            Value::String("P".into()),
        )
        .unwrap();
    let context = engine::ExecutionContext::new(&fixture.0).with_parameters(&parameters);
    let grouped = mfd::ImportedGroupedJsonEdiDocument {
        document: imported.document,
        source: imported.source,
        writer_minimums: imported.writer_minimums,
    };
    for (text, expected) in [
        (r#"{"rows":[{"name":"a","name":"b","qty":1}]}"#, "a"),
        (r#"{"rows":[{"name":"a","name":"","qty":1}]}"#, "a"),
    ] {
        let source = input(&grouped, &[text]);
        let ir::Instance::DocumentSet(documents) =
            engine::run_with_context(grouped.document.project(), &source, &context).unwrap()
        else {
            panic!("document set");
        };
        assert_eq!(documents.len(), 2);
        for document in documents {
            let interchange = &document
                .value()
                .field("Interchange")
                .unwrap()
                .as_repeated()
                .unwrap()[0];
            let group = &interchange.field("Group").unwrap().as_repeated().unwrap()[0];
            let message = group.field("Message_850").unwrap();
            let message = message.as_repeated().map_or(message, |items| &items[0]);
            let line = &message.field("Lines").unwrap().as_repeated().unwrap()[0];
            assert_eq!(
                line.field("LIN").unwrap().field("F1"),
                Some(&ir::Instance::Scalar(Value::String(expected.into())))
            );
        }
    }
}

fn invoked_sibling_fixture() -> Fixture {
    let fixture = invoked_fixture();
    let schema = fixture.0.join("source.json");
    let contents = std::fs::read_to_string(&schema)
        .unwrap()
        .replace("\"number\"", "\"string\"");
    std::fs::write(schema, contents).unwrap();
    fixture.change(
        r#"<entry name="number" outkey="22"/>"#,
        r#"<entry name="string" outkey="22"/>"#,
    );
    fixture.change("</children>", r#"<component name="constant" library="core" kind="2" uid="80"><targets><datapoint pos="0" key="180"/></targets><data><constant value="a" datatype="string"/><wsdl/></data></component>
<component name="equal" library="core" kind="5" uid="82"><sources><datapoint pos="0" key="182"/><datapoint pos="1" key="183"/></sources><targets><datapoint pos="0" key="184"/></targets></component>
<component name="if-else" library="core" kind="4" uid="83"><sources><datapoint pos="0" key="185"/><datapoint pos="1" key="186"/><datapoint/></sources><targets><datapoint pos="0" key="188"/></targets></component></children>"#);
    fixture.change(r#"<entry name="LIN" inpkey="84">"#, r#"<entry name="LIN">"#);
    fixture.change(r#"<entry name="F2" inpkey="86"/>"#, r#"<entry name="F2"/>"#);
    fixture.change(r#"<vertex vertexkey="20"><edges><edge vertexkey="83"/><edge vertexkey="84"/></edges></vertex>"#, "");
    fixture.change(
        r#"<vertex vertexkey="21"><edges><edge vertexkey="85"/></edges></vertex>"#,
        r#"<vertex vertexkey="21"><edges><edge vertexkey="182"/></edges></vertex>"#,
    );
    fixture.change(
        r#"<vertex vertexkey="22"><edges><edge vertexkey="86"/></edges></vertex>"#,
        r#"<vertex vertexkey="22"><edges><edge vertexkey="186"/></edges></vertex>"#,
    );
    fixture.change("</vertices>", r#"<vertex vertexkey="180"><edges><edge vertexkey="183"/></edges></vertex><vertex vertexkey="184"><edges><edge vertexkey="185"/></edges></vertex><vertex vertexkey="188"><edges><edge vertexkey="83"/><edge vertexkey="85"/></edges></vertex></vertices>"#);
    fixture
}

#[test]
fn invoked_pure_conditional_keeps_predicate_occurrences_and_last_sibling_value() {
    let fixture = invoked_sibling_fixture();
    let imported = mfd::import_invoked_grouped_json_edi_document_with_options(
        &fixture.0.join("mapping.mfd"),
        &mfd::ImportOptions::default().with_package_root(&fixture.0),
    )
    .unwrap();
    let mut parameters = engine::RuntimeParameters::new();
    parameters
        .insert(
            &imported.invocation.key_parameter,
            Value::String("P".into()),
        )
        .unwrap();
    let context = engine::ExecutionContext::new(&fixture.0).with_parameters(&parameters);
    let grouped = mfd::ImportedGroupedJsonEdiDocument {
        document: imported.document,
        source: imported.source,
        writer_minimums: imported.writer_minimums,
    };
    for (text, count, expected) in [
        (
            r#"{"rows":[{"name":"a","name":"a","qty":"first","qty":"last"}]}"#,
            2,
            "last",
        ),
        (
            r#"{"rows":[{"name":"b","name":"a","qty":"last"}]}"#,
            1,
            "last",
        ),
        (r#"{"rows":[{"name":"a","qty":""}]}"#, 1, ""),
        (r#"{"rows":[{"name":"a"}]}"#, 0, ""),
        (r#"{"rows":[{"name":"a","qty":null}]}"#, 0, ""),
    ] {
        let source = input(&grouped, &[text]);
        let ir::Instance::DocumentSet(documents) =
            engine::run_with_context(grouped.document.project(), &source, &context).unwrap()
        else {
            panic!("documents");
        };
        assert!(!documents.is_empty());
        for document in documents {
            let interchange = &document
                .value()
                .field("Interchange")
                .unwrap()
                .as_repeated()
                .unwrap()[0];
            let group = &interchange.field("Group").unwrap().as_repeated().unwrap()[0];
            let message = group.field("Message_850").unwrap();
            let message = message.as_repeated().map_or(message, |items| &items[0]);
            let lines = message.field("Lines").unwrap().as_repeated().unwrap();
            assert_eq!(lines.len(), count, "{text}");
            for line in lines {
                assert_eq!(
                    line.field("LIN").unwrap().field("F1"),
                    Some(&ir::Instance::Scalar(Value::String(expected.into())))
                );
            }
        }
    }
    let source = input(
        &grouped,
        &[r#"{"rows":[{"name":"a","qty":"first"},{"name":"a","qty":"second"}]}"#],
    );
    let ir::Instance::DocumentSet(documents) =
        engine::run_with_context(grouped.document.project(), &source, &context).unwrap()
    else {
        panic!("documents");
    };
    assert_eq!(documents.len(), 1);
    let interchange = &documents[0]
        .value()
        .field("Interchange")
        .unwrap()
        .as_repeated()
        .unwrap()[0];
    let group = &interchange.field("Group").unwrap().as_repeated().unwrap()[0];
    let message = group.field("Message_850").unwrap();
    let message = message.as_repeated().map_or(message, |items| &items[0]);
    let actual = message
        .field("Lines")
        .unwrap()
        .as_repeated()
        .unwrap()
        .iter()
        .map(|line| line.field("LIN").unwrap().field("F1").unwrap().clone())
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            ir::Instance::Scalar(Value::String("first".into())),
            ir::Instance::Scalar(Value::String("second".into()))
        ]
    );
}

#[test]
fn invoked_sibling_conditional_does_not_admit_other_predicates_or_else_branches() {
    for (from, to) in [
        (
            r#"name="equal" library="core" kind="5" uid="82""#,
            r#"name="not-equal" library="core" kind="5" uid="82""#,
        ),
        (
            r#"<datapoint pos="1" key="186"/><datapoint/>"#,
            r#"<datapoint pos="1" key="186"/><datapoint pos="2" key="187"/>"#,
        ),
    ] {
        let fixture = invoked_sibling_fixture();
        fixture.change(from, to);
        if to.contains("187") {
            fixture.change(
                r#"<edge vertexkey="182"/>"#,
                r#"<edge vertexkey="182"/><edge vertexkey="187"/>"#,
            );
        }
        assert!(
            mfd::import_invoked_grouped_json_edi_document_with_options(
                &fixture.0.join("mapping.mfd"),
                &mfd::ImportOptions::default().with_package_root(&fixture.0),
            )
            .is_err()
        );
    }
}

#[test]
fn nested_nonrepeating_conditional_group_is_refused_instead_of_losing_its_predicate() {
    let fixture = invoked_sibling_fixture();
    fixture.change(r#"<entry name="LIN">"#, r#"<entry name="LIN" inpkey="84">"#);
    fixture.change("</children>", r#"<component name="constant" library="core" kind="2" uid="95"><targets><datapoint pos="0" key="195"/></targets><data><constant value="b" datatype="string"/><wsdl/></data></component>
<component name="equal" library="core" kind="5" uid="96"><sources><datapoint pos="0" key="196"/><datapoint pos="1" key="197"/></sources><targets><datapoint pos="0" key="198"/></targets></component>
<component name="if-else" library="core" kind="4" uid="97"><sources><datapoint pos="0" key="199"/><datapoint pos="1" key="200"/><datapoint/></sources><targets><datapoint pos="0" key="201"/></targets></component></children>"#);
    fixture.change(
        r#"<edge vertexkey="182"/>"#,
        r#"<edge vertexkey="182"/><edge vertexkey="196"/>"#,
    );
    fixture.change(
        r#"<edge vertexkey="186"/>"#,
        r#"<edge vertexkey="186"/><edge vertexkey="200"/>"#,
    );
    fixture.change("</vertices>", r#"<vertex vertexkey="195"><edges><edge vertexkey="197"/></edges></vertex><vertex vertexkey="198"><edges><edge vertexkey="199"/></edges></vertex><vertex vertexkey="201"><edges><edge vertexkey="84"/></edges></vertex></vertices>"#);
    let result = mfd::import_invoked_grouped_json_edi_document_with_options(
        &fixture.0.join("mapping.mfd"),
        &mfd::ImportOptions::default().with_package_root(&fixture.0),
    );
    let error = result.err().expect("nonrepeating conditional was admitted");
    assert!(
        error
            .to_string()
            .contains("same-frame conditional group requires a repeating target")
    );
}

fn invoked_nested_fixture() -> Fixture {
    let fixture = invoked_sibling_fixture();
    let schema = fixture.0.join("source.json");
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&schema).unwrap()).unwrap();
    value["properties"]["rows"]["items"]["properties"]["fallback"] =
        serde_json::json!({"type":"string"});
    std::fs::write(schema, serde_json::to_string(&value).unwrap()).unwrap();
    fixture.change(r#"<entry name="qty" type="json-property"><entry name="string" outkey="22"/></entry>"#,
        r#"<entry name="qty" type="json-property"><entry name="string" outkey="22"/></entry><entry name="fallback" type="json-property"><entry name="string" outkey="190"/></entry>"#);
    fixture.change(
        r#"<datapoint pos="1" key="186"/><datapoint/>"#,
        r#"<datapoint pos="1" key="186"/><datapoint pos="2" key="187"/>"#,
    );
    fixture.change(r#"<vertex vertexkey="22"><edges><edge vertexkey="186"/></edges></vertex>"#,
        r#"<vertex vertexkey="22"><edges><edge vertexkey="191"/><edge vertexkey="195"/></edges></vertex>"#);
    fixture.change("</children>", r#"<component name="empty" library="lang" uid="91" kind="5"><sources><datapoint pos="0" key="191"/></sources><targets><datapoint pos="0" key="192"/></targets></component>
<component name="if-else" library="core" uid="92" kind="4"><sources><datapoint pos="0" key="193"/><datapoint pos="1" key="194"/><datapoint pos="2" key="195"/></sources><targets><datapoint pos="0" key="198"/></targets></component></children>"#);
    fixture.change("</vertices>", r#"<vertex vertexkey="192"><edges><edge vertexkey="193"/></edges></vertex><vertex vertexkey="190"><edges><edge vertexkey="194"/></edges></vertex><vertex vertexkey="198"><edges><edge vertexkey="186"/><edge vertexkey="187"/></edges></vertex></vertices>"#);
    fixture
}

#[test]
fn invoked_same_branch_gate_keeps_typed_occurrences_and_nested_empty_fallback() {
    let fixture = invoked_nested_fixture();
    let imported = mfd::import_invoked_grouped_json_edi_document_with_options(
        &fixture.0.join("mapping.mfd"),
        &mfd::ImportOptions::default().with_package_root(&fixture.0),
    )
    .unwrap();
    let mut parameters = engine::RuntimeParameters::new();
    parameters
        .insert(
            &imported.invocation.key_parameter,
            Value::String("P".into()),
        )
        .unwrap();
    let context = engine::ExecutionContext::new(&fixture.0).with_parameters(&parameters);
    let grouped = mfd::ImportedGroupedJsonEdiDocument {
        document: imported.document,
        source: imported.source,
        writer_minimums: imported.writer_minimums,
    };
    for (text, count, expected) in [
        (r#"{"rows":[{"name":"b","qty":"value"}]}"#, 1, "value"),
        (
            r#"{"rows":[{"name":"a","name":"b","qty":"","fallback":"one","fallback":"two"}]}"#,
            2,
            "two",
        ),
        (
            r#"{"rows":[{"name":"a","qty":"value","qty":"","fallback":"replacement"}]}"#,
            1,
            "replacement",
        ),
        (r#"{"rows":[{"name":"a","qty":""}]}"#, 0, ""),
        (r#"{"rows":[{"name":"a","fallback":"unused"}]}"#, 0, ""),
    ] {
        let source = input(&grouped, &[text]);
        let ir::Instance::DocumentSet(documents) =
            engine::run_with_context(grouped.document.project(), &source, &context).unwrap()
        else {
            panic!("documents");
        };
        assert!(!documents.is_empty());
        for document in documents {
            let interchange = &document
                .value()
                .field("Interchange")
                .unwrap()
                .as_repeated()
                .unwrap()[0];
            let group = &interchange.field("Group").unwrap().as_repeated().unwrap()[0];
            let message = group.field("Message_850").unwrap();
            let message = message.as_repeated().map_or(message, |items| &items[0]);
            let lines = message.field("Lines").unwrap().as_repeated().unwrap();
            assert_eq!(lines.len(), count, "{text}");
            for line in lines {
                assert_eq!(
                    line.field("LIN").unwrap().field("F1"),
                    Some(&ir::Instance::Scalar(Value::String(expected.into())))
                );
            }
        }
    }
}

#[test]
fn invoked_conditional_field_reduces_inside_each_object_before_writer_selection() {
    let fixture = invoked_sibling_fixture();
    fixture.change(
        r#"<entry name="Lines" inpkey="83">"#,
        r#"<entry name="Lines">"#,
    );
    fixture.change(r#"<edge vertexkey="83"/>"#, "");
    let imported = mfd::import_invoked_grouped_json_edi_document_with_options(
        &fixture.0.join("mapping.mfd"),
        &mfd::ImportOptions::default().with_package_root(&fixture.0),
    )
    .unwrap();
    let mut parameters = engine::RuntimeParameters::new();
    parameters
        .insert(
            &imported.invocation.key_parameter,
            Value::String("P".into()),
        )
        .unwrap();
    let context = engine::ExecutionContext::new(&fixture.0).with_parameters(&parameters);
    let grouped = mfd::ImportedGroupedJsonEdiDocument {
        document: imported.document,
        source: imported.source,
        writer_minimums: imported.writer_minimums,
    };
    for (text, expected) in [
        (
            r#"{"rows":[{"name":"a","qty":"first"},{"name":"a","qty":"second"}]}"#,
            "first",
        ),
        (
            r#"{"rows":[{"name":"a","qty":""},{"name":"a","qty":"second"}]}"#,
            "second",
        ),
    ] {
        let source = input(&grouped, &[text]);
        let ir::Instance::DocumentSet(documents) =
            engine::run_with_context(grouped.document.project(), &source, &context).unwrap()
        else {
            panic!("documents");
        };
        assert_eq!(documents.len(), 1);
        let interchange = &documents[0]
            .value()
            .field("Interchange")
            .unwrap()
            .as_repeated()
            .unwrap()[0];
        let group = &interchange.field("Group").unwrap().as_repeated().unwrap()[0];
        let message = group.field("Message_850").unwrap();
        let message = message.as_repeated().map_or(message, |items| &items[0]);
        let lines = message.field("Lines").unwrap().as_repeated().unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].field("LIN").unwrap().field("F1"),
            Some(&ir::Instance::Scalar(Value::String(expected.into())))
        );
    }
}

#[test]
fn pure_conditional_matrix_preserves_typed_presence_duplicate_guards_and_last_values() {
    conditional_matrix(invoked_sibling_fixture(), false);
}

#[test]
fn nested_conditional_matrix_preserves_typed_presence_and_empty_fallbacks() {
    conditional_matrix(invoked_nested_fixture(), true);
}

fn conditional_matrix(fixture: Fixture, nested: bool) {
    use serde_json::Value as Json;
    let imported = mfd::import_invoked_grouped_json_edi_document_with_options(
        &fixture.0.join("mapping.mfd"),
        &mfd::ImportOptions::default().with_package_root(&fixture.0),
    )
    .unwrap();
    let mut parameters = engine::RuntimeParameters::new();
    parameters
        .insert(
            &imported.invocation.key_parameter,
            Value::String("P".into()),
        )
        .unwrap();
    let context = engine::ExecutionContext::new(&fixture.0).with_parameters(&parameters);
    let grouped = mfd::ImportedGroupedJsonEdiDocument {
        document: imported.document,
        source: imported.source,
        writer_minimums: imported.writer_minimums,
    };
    let guards: Vec<Vec<Json>> = serde_json::from_str(
        r#"[[],[null],[7],["b"],["a"],["a","a"],["b","a"],["a","b"],[null,"a"],["a",null]]"#,
    )
    .unwrap();
    let values: Vec<Vec<Json>> = serde_json::from_str(
        r#"[[],[null],[7],[""],["A"],["A","B"],["","B"],["A",""],[7,"B"],["A",null]]"#,
    )
    .unwrap();
    for guard in &guards {
        for values in &values {
            let mut entries = guard
                .iter()
                .map(|value| format!("\"name\":{value}"))
                .chain(values.iter().map(|value| format!("\"qty\":{value}")))
                .collect::<Vec<_>>();
            if nested {
                entries.push("\"fallback\":\"FALLBACK\"".into());
            }
            let text = format!("{{\"rows\":[{{{}}}]}}", entries.join(","));
            let source = input(&grouped, &[&text]);
            let ir::Instance::DocumentSet(documents) =
                engine::run_with_context(grouped.document.project(), &source, &context).unwrap()
            else {
                panic!("documents");
            };
            let keys = guard
                .iter()
                .filter_map(Json::as_str)
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(documents.len(), keys.len(), "{text}");
            let selected = values
                .iter()
                .filter_map(Json::as_str)
                .next_back()
                .map(|value| {
                    if nested && value.is_empty() {
                        "FALLBACK"
                    } else {
                        value
                    }
                });
            let count = if selected.is_some() {
                guard
                    .iter()
                    .filter(|value| value.as_str().is_some_and(|value| nested || value == "a"))
                    .count()
            } else {
                0
            };
            for document in documents {
                let interchange = &document
                    .value()
                    .field("Interchange")
                    .unwrap()
                    .as_repeated()
                    .unwrap()[0];
                let group = &interchange.field("Group").unwrap().as_repeated().unwrap()[0];
                let message = group.field("Message_850").unwrap();
                let message = message.as_repeated().map_or(message, |items| &items[0]);
                let lines = message.field("Lines").unwrap().as_repeated().unwrap();
                assert_eq!(lines.len(), count, "{text}");
                for line in lines {
                    assert_eq!(
                        line.field("LIN").unwrap().field("F1"),
                        Some(&ir::Instance::Scalar(Value::String(
                            selected.unwrap().into()
                        ))),
                        "{text}"
                    );
                }
            }
        }
    }
}

#[test]
fn grouped_scalar_if_else_preserves_both_declared_branches() {
    let fixture = Fixture::new();
    fixture.change("</children>", r#"<component name="constant" library="core" kind="2" uid="80"><targets><datapoint pos="0" key="180"/></targets><data><constant value="a" datatype="string"/><wsdl/></data></component>
<component name="constant" library="core" kind="2" uid="81"><targets><datapoint pos="0" key="181"/></targets><data><constant value="other" datatype="string"/><wsdl/></data></component>
<component name="equal" library="core" kind="5" uid="82"><sources><datapoint pos="0" key="182"/><datapoint pos="1" key="183"/></sources><targets><datapoint pos="0" key="184"/></targets></component>
<component name="if-else" library="core" kind="4" uid="83"><sources><datapoint pos="0" key="185"/><datapoint pos="1" key="186"/><datapoint pos="2" key="187"/></sources><targets><datapoint pos="0" key="188"/></targets></component></children>"#);
    fixture.change(
        r#"<vertex vertexkey="21"><edges><edge vertexkey="85"/></edges></vertex>"#,
        r#"<vertex vertexkey="21"><edges><edge vertexkey="182"/><edge vertexkey="186"/></edges></vertex>"#,
    );
    fixture.change("</vertices>", r#"<vertex vertexkey="180"><edges><edge vertexkey="183"/></edges></vertex><vertex vertexkey="181"><edges><edge vertexkey="187"/></edges></vertex><vertex vertexkey="184"><edges><edge vertexkey="185"/></edges></vertex><vertex vertexkey="188"><edges><edge vertexkey="85"/></edges></vertex></vertices>"#);
    let imported = fixture.import().unwrap();
    let source = input(
        &imported,
        &[r#"{"rows":[{"name":"a","qty":1},{"name":"b","qty":2}]}"#],
    );
    let ir::Instance::DocumentSet(documents) =
        engine::run(imported.document.project(), &source).unwrap()
    else {
        panic!("document set");
    };
    for (document, expected) in documents.iter().zip(["a", "other"]) {
        let interchange = &document
            .value()
            .field("Interchange")
            .unwrap()
            .as_repeated()
            .unwrap()[0];
        let group = &interchange.field("Group").unwrap().as_repeated().unwrap()[0];
        let message = group.field("Message_850").unwrap();
        let message = message.as_repeated().map_or(message, |items| &items[0]);
        let line = &message.field("Lines").unwrap().as_repeated().unwrap()[0];
        assert_eq!(
            line.field("LIN").unwrap().field("F1"),
            Some(&ir::Instance::Scalar(Value::String(expected.into())))
        );
    }
}

fn input(imported: &mfd::ImportedGroupedJsonEdiDocument, texts: &[&str]) -> Instance {
    let docs = texts
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let document = codegen_runtime::JsonSourceDocument::parse(
                &format!("file{index}.json"),
                text.as_bytes(),
            )
            .unwrap();
            let projected = codegen_runtime::project_json_source_document(
                &imported.source.shape,
                &document,
                Default::default(),
            )
            .unwrap();
            match imported.document.json_grouping_preparation() {
                Some(preparation) => codegen_runtime::prepare_json_grouping_document(
                    preparation,
                    &imported.source.shape,
                    projected,
                    Default::default(),
                )
                .unwrap(),
                None => projected,
            }
        })
        .collect();
    Instance::Group(vec![("Documents".into(), Instance::Repeated(docs))].into())
}

#[test]
fn grouped_keys_keep_empty_values_and_merge_physical_files_without_crossing_owners() {
    let fixture = Fixture::new();
    let before = std::fs::read(fixture.0.join("mapping.mfd")).unwrap();
    let imported = fixture.import().unwrap();
    let source = input(
        &imported,
        &[
            r#"{"rows":[{"name":"a","qty":1},{"name":"b","qty":2},{"name":"","qty":3},{"qty":99},{"name":null,"qty":98}]}"#,
            r#"{"rows":[{"name":"a","qty":4},{"name":"b","qty":5}]}"#,
        ],
    );
    let Instance::DocumentSet(documents) =
        engine::run(imported.document.project(), &source).unwrap()
    else {
        panic!("document set");
    };
    assert_eq!(
        documents.iter().map(|d| d.path()).collect::<Vec<_>>(),
        ["out.edia", "out.edib", "out.edi"]
    );
    for (document, expected) in documents
        .iter()
        .zip([vec!["1", "4"], vec!["2", "5"], vec!["3"]])
    {
        let interchange = &document
            .value()
            .field("Interchange")
            .unwrap()
            .as_repeated()
            .unwrap()[0];
        let group = &interchange.field("Group").unwrap().as_repeated().unwrap()[0];
        let message = group
            .field("Message_850")
            .unwrap_or_else(|| panic!("{group:?}"));
        let message = match message {
            Instance::Repeated(items) => &items[0],
            value => value,
        };
        let lines = message.field("Lines").unwrap().as_repeated().unwrap();
        let actual = lines
            .iter()
            .map(|line| {
                let Instance::Scalar(value) = line.field("LIN").unwrap().field("F2").unwrap()
                else {
                    panic!("scalar qty");
                };
                functions::call("string", std::slice::from_ref(value)).unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            expected
                .into_iter()
                .map(|s| Value::String(s.into()))
                .collect::<Vec<_>>()
        );
    }
    let program = codegen::lower_document(&imported.document).unwrap();
    assert!(
        codegen::prepare_mapforce_x12_writer(
            program.program(),
            &imported.document.project().target_options,
            &imported.writer_minimums
        )
        .is_err()
    );
    // This small import fixture deliberately has no complete ISA/GS schema.
    // Admitting document-set execution must not bypass writer-schema checks.
    assert!(
        codegen::prepare_mapforce_x12_document_writer(
            &program,
            &imported.document.project().target_options,
            &imported.writer_minimums
        )
        .is_err()
    );
    assert!(
        mfd::import_json_edi_document_with_options(
            &fixture.0.join("mapping.mfd"),
            &mfd::ImportOptions::default().with_package_root(&fixture.0)
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(fixture.0.join("mapping.mfd")).unwrap(),
        before
    );
}

#[test]
fn repeated_keys_keep_separate_group_memberships_and_original_member_fields() {
    let fixture = Fixture::new();
    let imported = fixture.import().unwrap();
    let source = input(
        &imported,
        &[r#"{"rows":[{"name":"a","name":"b","qty":1}]}"#],
    );
    let Instance::DocumentSet(documents) =
        engine::run(imported.document.project(), &source).unwrap()
    else {
        panic!("documents");
    };
    assert_eq!(
        documents
            .iter()
            .map(|document| document.path())
            .collect::<Vec<_>>(),
        ["out.edia", "out.edib"]
    );
    assert_eq!(documents[0].value(), documents[1].value());
}

#[test]
fn grouped_filename_admission_keeps_its_raw_pins_and_source_guards() {
    for (from, to) in [
        (
            r#"<datapoint pos="1" key="103"/>"#,
            r#"<datapoint pos="0" key="103"/>"#,
        ),
        (r#"<edge vertexkey="101"/>"#, r#"<edge vertexkey="102"/>"#),
        (
            r#"name="group-by" library="core""#,
            r#"name="group-by" library="foreign""#,
        ),
    ] {
        let fixture = Fixture::new();
        fixture.change(from, to);
        assert!(fixture.import().is_err());
    }
}
