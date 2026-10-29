use super::*;

pub(super) fn project() -> TestResult<Project> {
    let directory = TempDir::new("xml_cast_suppression")?;
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/target_float_cast");
    for name in ["source.xsd", "target.xsd"] {
        std::fs::copy(fixture.join(name), directory.0.join(name))?;
    }
    let text = std::fs::read_to_string(fixture.join("cast-first.mfd"))?;
    std::fs::write(
        directory.0.join("mapping.mfd"),
        text.replace("cast-in-subtree", "suppress-cast-in-subtree"),
    )?;
    let imported = mfd::import(&directory.0.join("mapping.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut project = imported.project;
    project.source_path = Some("input.xml".into());
    project.target_path = Some("output.xml".into());
    Ok(project)
}

#[test]
fn explicit_xml_cast_suppression_keeps_text_and_strict_json_boundaries() -> TestResult<()> {
    let project = project()?;
    let mut cases = ["9007199254740993", "\t 2.5 \n", "not-a-number", "00029"].into_iter().map(|value| {
        serde_json::json!({"input":format!("<Envelope><Numerator>34</Numerator><Denominator>2</Denominator><Text>{value}</Text><Label>safe</Label></Envelope>"),
            "expected":value, "invalid":value == "not-a-number", "second":"17", "numerator":34.0})
    }).collect::<Vec<_>>();
    cases.push(serde_json::json!({"input":"<Envelope><Numerator>0.0000002</Numerator><Denominator>2</Denominator><Text>00029</Text><Label>safe</Label></Envelope>", "expected":"00029", "invalid":false, "second":"0.0000001", "numerator":0.0000002}));
    json_text_boundaries::run_generated_boundary_cases_with_harnesses(
        &project,
        &cases,
        "xml_cast_suppression",
        include_str!("xml_cast_suppression/Host.rs.txt"),
        include_str!("xml_cast_suppression/Host.cs.txt"),
    )?;
    let mut integer_project = project;
    let node = integer_project.root.bindings.iter().find(|binding| {
        binding.target_field == "Amount" && matches!(integer_project.graph.nodes.get(&binding.node), Some(Node::Call { function, .. }) if function == "divide")
    }).expect("independent numeric target binding").node;
    integer_project.graph.nodes.insert(
        node,
        Node::Const {
            value: Value::Int(17),
        },
    );
    let cases = vec![
        serde_json::json!({"input":"<Envelope><Numerator>34</Numerator><Denominator>2</Denominator><Text>00029</Text><Label>safe</Label></Envelope>", "expected":"00029", "invalid":false, "second":"17", "numerator":34.0, "typed_int":true}),
    ];
    json_text_boundaries::run_generated_boundary_cases_with_harnesses(
        &integer_project,
        &cases,
        "xml_cast_suppression_int",
        include_str!("xml_cast_suppression/Host.rs.txt"),
        include_str!("xml_cast_suppression/Host.cs.txt"),
    )
}

#[test]
fn invalid_explicit_scalar_policies_never_degrade_to_an_optional_xml_adapter() -> TestResult<()> {
    let project = project()?;
    let program = codegen::lower(&project)?;
    assert!(program.xml_scalar_preservation().is_some());
    let mut invalid = program.clone();
    invalid.xml_boundary.as_mut().unwrap().output.scalar_output =
        Some(ir::XmlScalarOutputPolicy::Unresolved {
            request: ir::XmlScalarOutputUnresolved::for_reason(
                ir::XmlScalarOutputUnresolvedReason::UnsupportedShape,
            ),
        });
    assert!(codegen_csharp::emit(&invalid).is_err());
    let options = codegen_rust::Options {
        package_name: "suppression".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Version("0.1.0".into()),
    };
    assert!(codegen_rust::emit(&invalid, &options).is_err());
    let mut no_xml_source = project.clone();
    no_xml_source.source_options.xml_document = false;
    no_xml_source.source_options.json_document = true;
    assert!(codegen::lower(&no_xml_source).is_err());
    Ok(())
}

fn document_project() -> TestResult<Project> {
    let order = |name: &str| {
        serde_json::json!({"name": name, "kind": {"kind": "group", "children": [
            {"name": "Orders", "kind": {"kind": "group", "children": [
                {"name": "Order", "repeating": true, "kind": {"kind": "group", "children": [
                    {"name": "Id", "kind": {"kind": "scalar", "ty": "int"}},
                    {"name": "Label", "kind": {"kind": "scalar", "ty": "string"}},
                    {"name": "Price", "kind": {"kind": "scalar", "ty": "float"}}
                ]}}
            ]}}
        ]}})
    };
    Ok(serde_json::from_value(serde_json::json!({
        "source": order("Input"),
        "target": order("Output"),
        "source_path": "input.xml",
        "target_path": "output.xml",
        "source_options": {"xml_document": true},
        "target_options": {"xml_document": true, "xml_scalar_output": {"mode": "suppress_cast_in_document_v1"}},
        "graph": {"nodes": {
            "0": {"kind": "const", "value": "00000.00"},
            "1": {"kind": "call", "function": "format_number", "args": [2, 0]},
            "2": {"kind": "source_field", "path": ["Price"], "frame": ["Orders", "Order"]},
            "3": {"kind": "source_field", "path": ["Id"], "frame": ["Orders", "Order"]},
            "4": {"kind": "source_field", "path": ["Label"], "frame": ["Orders", "Order"]}
        }},
        "root": {"target_field": "", "children": [{"target_field": "Orders", "children": [
            {"target_field": "Order", "source": ["Orders", "Order"], "bindings": [
                {"target_field": "Id", "node": 3},
                {"target_field": "Label", "node": 4},
                {"target_field": "Price", "node": 1}
            ]}
        ]}]}
    }))?)
}

#[test]
fn document_cast_suppression_matches_the_interpreter_in_generated_code() -> TestResult<()> {
    let project = document_project()?;
    let input = "<Input><Orders><Order><Id>7</Id><Label>a</Label><Price>25</Price></Order>\
        <Order><Id>8</Id><Label>b</Label><Price>2.3</Price></Order></Orders></Input>";
    let output = engine::run(&project, &format_xml::from_str(input, &project.source)?)?;
    let expected = format_xml::to_string_with_options(
        &project.target,
        &output,
        &format_xml::XmlWriteOptions {
            scalar_output: project.target_options.xml_scalar_output.clone(),
            ..Default::default()
        },
    )?;
    assert!(
        expected.contains("<Price>00025.00</Price>")
            && expected.contains("<Price>00002.30</Price>"),
        "{expected}"
    );
    let cases = vec![serde_json::json!({"input": input, "expected": expected,
        "json": {"Orders": {"Order": [{"Id": 7, "Label": "a", "Price": 25}]}}})];
    json_text_boundaries::run_generated_boundary_cases_with_harnesses(
        &project,
        &cases,
        "xml_cast_suppression_document",
        include_str!("xml_cast_suppression/DocumentHost.rs.txt"),
        include_str!("xml_cast_suppression/DocumentHost.cs.txt"),
    )
}
