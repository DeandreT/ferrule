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

fn object_invoked_fixture() -> Fixture {
    let fixture = invoked_fixture();
    std::fs::write(fixture.0.join("variables.json"), r#"{"type":"object","properties":{"projectCode":{"type":"string"},"fileName":{"type":"string"},"control":{"type":"string"}}}"#).unwrap();
    fixture.change(r#"<entry name="projectCode" type="json-property"><entry name="string" outkey="143"/></entry>"#,
        r#"<entry name="projectCode" type="json-property"><entry name="string" outkey="143"/></entry><entry name="fileName" type="json-property"><entry name="string" outkey="151"/></entry><entry name="control" type="json-property"><entry name="string" outkey="152"/></entry>"#);
    fixture.change("</children>", r#"<component name="constant" library="core" uid="44" kind="2"><targets><datapoint pos="0" key="153"/></targets><data><constant value="K" datatype="string"/></data></component><component name="concat" library="core" uid="45" kind="5"><sources><datapoint pos="0" key="154"/><datapoint pos="1" key="155"/></sources><targets><datapoint pos="0" key="156"/></targets></component></children>"#);
    fixture.change(r#"<vertex vertexkey="143"><edges><edge vertexkey="145"/></edges></vertex>"#,
        r#"<vertex vertexkey="153"><edges><edge vertexkey="145"/></edges></vertex><vertex vertexkey="143"><edges><edge vertexkey="154"/></edges></vertex>"#);
    fixture.change(r#"<vertex vertexkey="149"><edges><edge vertexkey="80"/><edge vertexkey="81"/><edge vertexkey="82"/></edges></vertex>"#,
        r#"<vertex vertexkey="149"><edges><edge vertexkey="155"/></edges></vertex><vertex vertexkey="156"><edges><edge vertexkey="80"/><edge vertexkey="81"/><edge vertexkey="82"/></edges></vertex>"#);
    fixture.change(
        r#"<vertex vertexkey="1"><edges><edge vertexkey="10"/></edges></vertex>"#,
        r#"<vertex vertexkey="151"><edges><edge vertexkey="10"/></edges></vertex>"#,
    );
    fixture.change(
        r#"<vertex vertexkey="22"><edges><edge vertexkey="86"/></edges></vertex>"#,
        r#"<vertex vertexkey="152"><edges><edge vertexkey="86"/></edges></vertex>"#,
    );
    fixture
}

#[test]
fn object_invocation_preserves_named_parameter_ownership_and_late_conversions() {
    let fixture = object_invoked_fixture();
    let imported = mfd::import_object_invoked_json_edi_document_with_options(
        &fixture.0.join("mapping.mfd"),
        &mfd::ImportOptions::default().with_package_root(&fixture.0),
    )
    .unwrap();
    assert_eq!(
        imported.invocation.iteration,
        mapping::JsonParameterIteration::RootStringOccurrences
    );
    assert_eq!(imported.filename_member, "fileName");
    let lowered = codegen::lower_document(&imported.document).unwrap();
    let object_binding = codegen::JsonParameterObjectBinding {
        source_name: imported.parameter_source_name.clone(),
        filename_member: imported.filename_member.clone(),
    };
    assert!(codegen::prepare_json_parameter_invocation(&lowered, &imported.invocation).is_err());
    assert!(
        codegen::prepare_json_parameter_object_invocation(
            &lowered,
            &imported.invocation,
            &object_binding
        )
        .is_ok()
    );
    let wrong_member = codegen::JsonParameterObjectBinding {
        filename_member: "missing".into(),
        ..object_binding.clone()
    };
    assert!(
        codegen::prepare_json_parameter_object_invocation(
            &lowered,
            &imported.invocation,
            &wrong_member
        )
        .is_err()
    );
    let wrong_owner = codegen::JsonParameterObjectBinding {
        source_name: "Other".into(),
        ..object_binding.clone()
    };
    assert!(
        codegen::prepare_json_parameter_object_invocation(
            &lowered,
            &imported.invocation,
            &wrong_owner
        )
        .is_err()
    );
    let mut wrong_iteration = imported.invocation.clone();
    wrong_iteration.iteration = mapping::JsonParameterIteration::DistinctRootStringKeys;
    assert!(
        codegen::prepare_json_parameter_object_invocation(
            &lowered,
            &wrong_iteration,
            &object_binding
        )
        .is_err()
    );
    let name = imported.parameter_source_name;
    let plan = imported.invocation;
    let grouped = mfd::ImportedGroupedJsonEdiDocument {
        document: imported.document,
        source: imported.source,
        writer_minimums: imported.writer_minimums,
    };
    let physical = input(&grouped, &[r#"{"rows":[{"name":"a","qty":2}]}"#]);
    for (text, fails) in [
        (
            r#"{"projectCode":"P","projectCode":"P","fileName":"one.json","control":"100"}"#,
            false,
        ),
        (
            r#"{"projectCode":"P","fileName":"one.json","control":"100","control":"bad"}"#,
            true,
        ),
    ] {
        let prepared = codegen_runtime::PreparedJsonParameterInvocations::prepare(
            &plan,
            &Value::String(text.into()),
            Default::default(),
            Default::default(),
        )
        .unwrap();
        let mut count = 0;
        for invocation in prepared.invocations() {
            count += 1;
            let parameter = codegen_runtime::project_json_source_value(
                &plan.projection,
                invocation.parameter_object(),
                Default::default(),
            )
            .unwrap()
            .unwrap();
            let mut parameters = engine::RuntimeParameters::new();
            let (key, value) = invocation.key_binding();
            parameters.insert(key, value).unwrap();
            let context = engine::ExecutionContext::new(&fixture.0).with_parameters(&parameters);
            let result = engine::run_with_sources_and_context(
                grouped.document.project(),
                &physical,
                vec![(name.clone(), parameter)],
                &context,
            );
            assert_eq!(result.is_err(), fails, "{result:?}");
            if !fails {
                let Instance::DocumentSet(documents) = result.unwrap() else {
                    panic!("documents");
                };
                assert_eq!(documents[0].path(), "PKout.edia");
            }
        }
        assert_eq!(count, if fails { 1 } else { 2 });
    }
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
fn invoked_empty_string_literal_may_omit_value_but_not_type_or_metadata() {
    let import = |literal: &str| {
        let fixture = invoked_fixture();
        fixture.change("</children>", &format!(r#"<component name="constant" library="core" kind="2" uid="90"><targets><datapoint pos="0" key="190"/></targets><data>{literal}</data></component></children>"#));
        fixture.change(
            r#"<vertex vertexkey="21"><edges><edge vertexkey="85"/></edges></vertex>"#,
            r#"<vertex vertexkey="190"><edges><edge vertexkey="85"/></edges></vertex>"#,
        );
        mfd::import_invoked_grouped_json_edi_document_with_options(
            &fixture.0.join("mapping.mfd"),
            &mfd::ImportOptions::default().with_package_root(&fixture.0),
        )
    };
    let omitted = import(r#"<constant datatype="string"/>"#).unwrap();
    let explicit = import(r#"<constant datatype="string" value=""/>"#).unwrap();
    assert_eq!(
        serde_json::to_value(omitted.document.project()).unwrap(),
        serde_json::to_value(explicit.document.project()).unwrap()
    );
    for literal in [
        r#"<constant datatype="decimal"/>"#,
        r#"<constant/>"#,
        r#"<constant datatype="string" extra="1"/>"#,
        r#"<constant datatype="string"><unexpected/></constant>"#,
    ] {
        assert!(import(literal).is_err(), "accepted {literal}");
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

fn invoked_qualifier_fixture() -> Fixture {
    let fixture = invoked_sibling_fixture();
    fixture.change(
        r#"<vertex vertexkey="20"><edges><edge vertexkey="101"/></edges></vertex>"#,
        r#"<vertex vertexkey="20"><edges><edge vertexkey="101"/><edge vertexkey="83"/></edges></vertex>"#,
    );
    fixture.change(
        r#"<vertex vertexkey="188"><edges><edge vertexkey="83"/><edge vertexkey="85"/></edges></vertex>"#,
        r#"<vertex vertexkey="188"><edges><edge vertexkey="602"/></edges></vertex>"#,
    );
    fixture.change("</children>", r#"<component name="NullableQualifier" library="Sample" uid="410" kind="19"><data><root><entry name="inQualifier" inpkey="601" componentid="401"/><entry name="inValue" inpkey="602" componentid="402"/></root><root><entry name="outQualifier" outkey="603" componentid="405"/><entry name="outValue" outkey="604" componentid="406"/></root></data></component>
<component name="constant" library="core" uid="411" kind="2"><targets><datapoint pos="0" key="605"/></targets><data><constant value="QUAL" datatype="string"/></data></component></children>"#);
    fixture.change("</vertices>", r#"<vertex vertexkey="605"><edges><edge vertexkey="601"/></edges></vertex><vertex vertexkey="603"><edges><edge vertexkey="85"/></edges></vertex></vertices>"#);
    fixture.change("</mapping>", r#"<component name="NullableQualifier" library="Sample" uid="400" inline="1"><structure><children>
<component name="inQualifier" library="core" uid="401" kind="6"><sources><datapoint/></sources><targets><datapoint pos="0" key="501"/></targets><data><input datatype="string"/><parameter usageKind="input" name="inQualifier"/></data></component>
<component name="inValue" library="core" uid="402" kind="6"><sources><datapoint/></sources><targets><datapoint pos="0" key="502"/></targets><data><input datatype="string"/><parameter usageKind="input" name="inValue"/></data></component>
<component name="is-null" library="db" uid="403" kind="5"><sources><datapoint pos="0" key="503"/></sources><targets><datapoint pos="0" key="504"/></targets></component>
<component name="if-else" library="core" uid="404" kind="4"><sources><datapoint pos="0" key="505"/><datapoint/><datapoint pos="2" key="507"/></sources><targets><datapoint pos="0" key="508"/></targets></component>
<component name="outQualifier" library="core" uid="405" kind="7"><sources><datapoint pos="0" key="509"/></sources><data><output datatype="string"/><parameter usageKind="output" name="outQualifier"/></data></component>
<component name="outValue" library="core" uid="406" kind="7"><sources><datapoint pos="0" key="510"/></sources><data><output datatype="string"/><parameter usageKind="output" name="outValue"/></data></component>
</children></structure><connections><edge from="502" to="503"/><edge from="504" to="505"/><edge from="501" to="507"/><edge from="508" to="509"/><edge from="502" to="510"/></connections></component></mapping>"#);
    fixture
}

fn invoked_ancestor_guard_fixture() -> Fixture {
    let fixture = invoked_qualifier_fixture();
    let schema = fixture.0.join("source.json");
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&schema).unwrap()).unwrap();
    let properties = value["properties"]["rows"]["items"]["properties"]
        .as_object_mut()
        .unwrap();
    properties.remove("qty");
    properties.insert("flag".into(), serde_json::json!({"type":"string"}));
    properties.insert("details".into(), serde_json::json!({"type":"array","items":{"type":"object","properties":{"qty":{"type":"string"}}}}));
    std::fs::write(schema, serde_json::to_string(&value).unwrap()).unwrap();
    fixture.change(
        r#"<entry name="array">"#,
        r#"<entry name="array" outkey="201">"#,
    );
    fixture.change(r#"<entry name="qty" type="json-property"><entry name="string" outkey="22"/></entry>"#,
        r#"<entry name="flag" type="json-property"><entry name="string" outkey="610"/></entry><entry name="details" type="json-property"><entry name="array"><entry name="item" type="json-item"><entry name="object" outkey="620"><entry name="qty" type="json-property"><entry name="string" outkey="22"/></entry></entry></entry></entry></entry>"#);
    fixture.change(r#"<vertex vertexkey="20"><edges><edge vertexkey="101"/><edge vertexkey="83"/></edges></vertex>"#,
        r#"<vertex vertexkey="201"><edges><edge vertexkey="101"/></edges></vertex><vertex vertexkey="620"><edges><edge vertexkey="83"/></edges></vertex>"#);
    fixture.change(
        r#"<vertex vertexkey="188"><edges><edge vertexkey="602"/></edges></vertex>"#,
        "",
    );
    fixture.change(r#"<vertex vertexkey="22"><edges><edge vertexkey="186"/></edges></vertex>"#,
        r#"<vertex vertexkey="22"><edges><edge vertexkey="186"/><edge vertexkey="602"/></edges></vertex>"#);
    fixture.change(r#"<entry name="LIN">"#, r#"<entry name="LIN" inpkey="84">"#);
    fixture.change("</children>", r#"<component name="empty" library="lang" uid="610" kind="5"><sources><datapoint pos="0" key="611"/></sources><targets><datapoint pos="0" key="612"/></targets></component>
<component name="if-else" library="core" uid="611" kind="4"><sources><datapoint pos="0" key="613"/><datapoint pos="1" key="614"/><datapoint/></sources><targets><datapoint pos="0" key="615"/></targets></component></children>"#);
    fixture.change(
        r#"<vertex vertexkey="603"><edges><edge vertexkey="85"/></edges></vertex>"#,
        "",
    );
    fixture.change("</vertices>", r#"<vertex vertexkey="610"><edges><edge vertexkey="611"/></edges></vertex><vertex vertexkey="612"><edges><edge vertexkey="613"/></edges></vertex><vertex vertexkey="603"><edges><edge vertexkey="614"/></edges></vertex><vertex vertexkey="615"><edges><edge vertexkey="84"/><edge vertexkey="85"/></edges></vertex></vertices>"#);
    let config = fixture.0.join("850.Config");
    let text = std::fs::read_to_string(&config).unwrap().replace(
        r#"<Segment ref="LIN"/>"#,
        r#"<Segment ref="LIN" maxOccurs="unbounded"/>"#,
    );
    std::fs::write(config, text).unwrap();
    fixture
}

#[test]
fn ancestor_empty_guard_preserves_occurrences_and_detail_ownership() {
    let fixture = invoked_ancestor_guard_fixture();
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
    for (flag, count) in [
        (r#""flag":"","#, 1),
        (r#""flag":"","flag":"","#, 2),
        (r#""flag":"x","#, 0),
        ("", 0),
        (r#""flag":null,"#, 0),
    ] {
        let text = format!(
            r#"{{"rows":[{{"name":"a",{flag}"details":[{{"qty":"first"}},{{"qty":"second"}},{{"qty":null}}]}}]}}"#
        );
        let source = input(&grouped, &[&text]);
        let Instance::DocumentSet(documents) =
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
        assert_eq!(lines.len(), 3);
        for (index, line) in lines.iter().enumerate() {
            let inner = line
                .field("LIN")
                .and_then(Instance::as_repeated)
                .unwrap_or(&[]);
            assert_eq!(inner.len(), if index == 2 { 0 } else { count }, "{text}");
            for item in inner {
                assert_eq!(
                    item.field("F1").and_then(Instance::as_scalar),
                    Some(&Value::String("QUAL".into()))
                );
            }
        }
    }
}

#[test]
fn ancestor_guard_refuses_changed_condition_and_nonstring_qualifier() {
    for (from, to) in [
        (
            r#"name="empty" library="lang" uid="610""#,
            r#"name="string-length" library="core" uid="610""#,
        ),
        (
            r#"value="QUAL" datatype="string""#,
            r#"value="1" datatype="decimal""#,
        ),
    ] {
        let fixture = invoked_ancestor_guard_fixture();
        fixture.change(from, to);
        assert!(
            mfd::import_invoked_grouped_json_edi_document_with_options(
                &fixture.0.join("mapping.mfd"),
                &mfd::ImportOptions::default().with_package_root(&fixture.0),
            )
            .is_err()
        );
    }
}

fn invoked_date_fixture() -> Fixture {
    let fixture = invoked_fixture();
    let schema = fixture.0.join("source.json");
    let text = std::fs::read_to_string(&schema)
        .unwrap()
        .replace("\"number\"", "\"string\"");
    std::fs::write(schema, text).unwrap();
    fixture.change(
        r#"<entry name="number" outkey="22"/>"#,
        r#"<entry name="string" outkey="22"/>"#,
    );
    fixture.change(r#"<entry name="F2" inpkey="86"/>"#, r#"<entry name="F2"/>"#);
    fixture.change(
        r#"<vertex vertexkey="21"><edges><edge vertexkey="85"/></edges></vertex>"#,
        "",
    );
    fixture.change(
        r#"<vertex vertexkey="22"><edges><edge vertexkey="86"/></edges></vertex>"#,
        r#"<vertex vertexkey="22"><edges><edge vertexkey="601"/></edges></vertex>"#,
    );
    fixture.change("</children>", r#"<component name="constant" library="core" uid="600" kind="2"><targets><datapoint pos="0" key="600"/></targets><data><constant value="[Y,4-4][M,2-2][D,2-2]" datatype="string"/></data></component>
<component name="format-date" library="core" uid="601" kind="5"><sources><datapoint pos="0" key="601"/><datapoint pos="1" key="602"/><datapoint/></sources><targets><datapoint pos="0" key="603"/></targets></component></children>"#);
    fixture.change("</vertices>", r#"<vertex vertexkey="600"><edges><edge vertexkey="602"/></edges></vertex><vertex vertexkey="603"><edges><edge vertexkey="85"/></edges></vertex></vertices>"#);
    fixture
}

fn invoked_integer_fixture() -> Fixture {
    let fixture = invoked_fixture();
    let path = fixture.0.join("source.json");
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\"number\"", "\"integer\"");
    std::fs::write(path, text).unwrap();
    fixture.change(r#"<entry name="F2" inpkey="86"/>"#, r#"<entry name="F2"/>"#);
    fixture.change(
        r#"<vertex vertexkey="21"><edges><edge vertexkey="85"/></edges></vertex>"#,
        "",
    );
    fixture.change(
        r#"<vertex vertexkey="22"><edges><edge vertexkey="86"/></edges></vertex>"#,
        r#"<vertex vertexkey="22"><edges><edge vertexkey="85"/></edges></vertex>"#,
    );
    fixture
}

fn invoked_date_prefix_fixture() -> Fixture {
    let fixture = invoked_date_fixture();
    fixture.change(r#"<component name="format-date" library="core" uid="601" kind="5"><sources><datapoint pos="0" key="601"/><datapoint pos="1" key="602"/><datapoint/></sources>"#,
        r#"<component name="parse-date" library="core" uid="601" kind="5"><sources><datapoint pos="0" key="601"/><datapoint pos="1" key="602"/></sources>"#);
    fixture
}

#[test]
fn integer_source_schema_preserves_fractional_input_selection_but_truncates_its_value() {
    let fixture = invoked_integer_fixture();
    let imported = mfd::import_invoked_grouped_json_edi_document_with_options(
        &fixture.0.join("mapping.mfd"),
        &mfd::ImportOptions::default().with_package_root(&fixture.0),
    )
    .unwrap();
    let truncates = |project: &mapping::Project| {
        project.graph.nodes.values().any(|node| {
            matches!(node, mapping::Node::Call { function, .. } if function == "decimal_truncate")
        })
    };
    assert!(truncates(imported.document.project()));
    let ordinary = invoked_fixture();
    let floating = mfd::import_invoked_grouped_json_edi_document_with_options(
        &ordinary.0.join("mapping.mfd"),
        &mfd::ImportOptions::default().with_package_root(&ordinary.0),
    )
    .unwrap();
    assert!(!truncates(floating.document.project()));
}

fn invoked_context_sum_fixture() -> Fixture {
    let fixture = invoked_fixture();
    fixture.change(
        r#"<entry name="Lines" inpkey="83">"#,
        r#"<entry name="Lines">"#,
    );
    fixture.change(r#"<entry name="LIN" inpkey="84">"#, r#"<entry name="LIN">"#);
    fixture.change(r#"<entry name="F2" inpkey="86"/>"#, r#"<entry name="F2"/>"#);
    fixture.change(r#"<vertex vertexkey="20"><edges><edge vertexkey="83"/><edge vertexkey="84"/></edges></vertex>"#, r#"<vertex vertexkey="20"><edges><edge vertexkey="620"/></edges></vertex>"#);
    fixture.change(
        r#"<vertex vertexkey="21"><edges><edge vertexkey="85"/></edges></vertex>"#,
        "",
    );
    fixture.change(
        r#"<vertex vertexkey="22"><edges><edge vertexkey="86"/></edges></vertex>"#,
        r#"<vertex vertexkey="22"><edges><edge vertexkey="621"/></edges></vertex>"#,
    );
    fixture.change("</children>", r#"<component name="sum" library="core" uid="620" kind="5"><sources><datapoint pos="0" key="620"/><datapoint pos="1" key="621"/></sources><targets><datapoint pos="0" key="622"/></targets></component></children>"#);
    fixture.change(
        "</vertices>",
        r#"<vertex vertexkey="622"><edges><edge vertexkey="85"/></edges></vertex></vertices>"#,
    );
    fixture
}

#[test]
fn context_sum_keeps_per_object_totals_and_evaluates_later_overflow() {
    let fixture = invoked_context_sum_fixture();
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
        (r#"{"rows":[{"name":"a","qty":2,"qty":3}]}"#, "5"),
        (
            r#"{"rows":[{"name":"a","qty":2},{"name":"a","qty":3}]}"#,
            "2",
        ),
        (r#"{"rows":[{"name":"a"},{"name":"a","qty":3}]}"#, "0"),
        (
            r#"{"rows":[{"name":"a","qty":0},{"name":"a","qty":3}]}"#,
            "0",
        ),
    ] {
        let source = input(&grouped, &[text]);
        let Instance::DocumentSet(documents) =
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
            lines[0]
                .field("LIN")
                .unwrap()
                .field("F1")
                .and_then(Instance::as_scalar),
            Some(&Value::String(expected.into()))
        );
    }
    let source = input(
        &grouped,
        &[r#"{"rows":[{"name":"a","qty":2},{"name":"a","qty":1e1000}]}"#],
    );
    assert!(engine::run_with_context(grouped.document.project(), &source, &context).is_err());
}

#[test]
fn context_sum_rejects_a_numeric_context_pin() {
    let fixture = invoked_context_sum_fixture();
    fixture.change(
        r#"<vertex vertexkey="20"><edges><edge vertexkey="620"/></edges></vertex>"#,
        r#"<vertex vertexkey="22"><edges><edge vertexkey="620"/></edges></vertex>"#,
    );
    assert!(
        mfd::import_invoked_grouped_json_edi_document_with_options(
            &fixture.0.join("mapping.mfd"),
            &mfd::ImportOptions::default().with_package_root(&fixture.0),
        )
        .is_err()
    );
}

#[test]
fn invoked_date_format_reuses_the_typed_runtime_and_rejects_extra_arguments() {
    let fixture = invoked_date_fixture();
    let import = || {
        mfd::import_invoked_grouped_json_edi_document_with_options(
            &fixture.0.join("mapping.mfd"),
            &mfd::ImportOptions::default().with_package_root(&fixture.0),
        )
    };
    let imported = import().unwrap();
    assert!(imported.document.project().graph.nodes.values().any(
        |node| matches!(node, mapping::Node::Call { function, .. } if function == "format_date")
    ));
    fixture.change(
        r#"<datapoint pos="1" key="602"/><datapoint/>"#,
        r#"<datapoint pos="1" key="602"/><datapoint pos="2" key="604"/>"#,
    );
    fixture.change(
        r#"<edge vertexkey="602"/>"#,
        r#"<edge vertexkey="602"/><edge vertexkey="604"/>"#,
    );
    assert!(import().is_err());
}

#[test]
fn sibling_udf_qualifier_tests_presence_without_discarding_an_empty_string() {
    let fixture = invoked_qualifier_fixture();
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
        (r#"{"rows":[{"name":"a","qty":"A"}]}"#, Some("QUAL")),
        (r#"{"rows":[{"name":"a","qty":""}]}"#, Some("QUAL")),
        (r#"{"rows":[{"name":"a","qty":null}]}"#, None),
        (r#"{"rows":[{"name":"a"}]}"#, None),
        (
            r#"{"rows":[{"name":"b","name":"a","qty":""}]}"#,
            Some("QUAL"),
        ),
        (r#"{"rows":[{"name":"b","qty":"A"}]}"#, None),
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
            let line = &message.field("Lines").unwrap().as_repeated().unwrap()[0];
            let field = line.field("LIN").unwrap().field("F1");
            assert_eq!(
                field.and_then(Instance::as_scalar),
                Some(&Value::String(expected.unwrap_or("").into())),
                "{text}"
            );
        }
    }
}

#[test]
fn sibling_udf_qualifier_refuses_a_different_condition_or_nonstring_literal() {
    for (from, to) in [
        (
            r#"name="is-null" library="db" uid="403""#,
            r#"name="is-not-null" library="db" uid="403""#,
        ),
        (
            r#"value="QUAL" datatype="string""#,
            r#"value="1" datatype="decimal""#,
        ),
    ] {
        let fixture = invoked_qualifier_fixture();
        fixture.change(from, to);
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
