use std::collections::{BTreeMap, BTreeSet};

use ir::Value;
use mapping::{AggregateOp, Node, SequenceExpr};

use super::{
    TempDir, copy_all_fixtures, emit, fixtures, generated_hosts, imported, refused, replace, run,
};

fn cases() -> Vec<serde_json::Value> {
    serde_json::from_str(include_str!(
        "../fixtures/json-array-lookup/root-scalars/cases.json"
    ))
    .unwrap()
}

#[test]
fn independent_root_scalars_preserve_lookup_ownership_and_complete_validation() {
    let project = imported(&fixtures().join("root-scalars/mapping.mfd"));
    let cases = cases();
    assert_eq!(cases.len(), 16, "retain the complete literal catalog");
    for case in cases {
        let result = run(&project, &case);
        if case["parameter_failure"] == "missing" {
            assert!(
                matches!(result, Err(engine::EngineError::MissingRuntimeParameter { name, .. }) if name == "LookupText"),
                "{case}"
            );
        } else if let Some(message) = case["error"].as_str() {
            let allowed = case["error_functions"].as_array().unwrap();
            assert_eq!(message, "input does not match the JSON schema");
            if case["primary"]["Items"].as_array().unwrap().is_empty() {
                assert_eq!(allowed, &[serde_json::json!("json_parse_field")]);
            }
            assert!(
                matches!(result, Err(engine::EngineError::Function(functions::FunctionError::InvalidArgument {
                    function, message: actual
                })) if actual == message && allowed.iter().any(|allowed| allowed.as_str() == Some(function))),
                "{case}: {result:?}"
            );
        } else {
            assert_eq!(
                result.unwrap(),
                format_json::from_str(&case["expected"].to_string(), &project.target).unwrap(),
                "{case}"
            );
        }
    }
}

#[test]
fn lookup_without_root_demand_still_validates_the_complete_composed_document() {
    let mut project = imported(&fixtures().join("root-scalars/mapping.mfd"));
    project.root.bindings.clear();
    for mut case in cases()
        .into_iter()
        .filter(|case| case.get("error").is_some())
    {
        // Force the lookup, independently of all metadata bindings, even for
        // late duplicate and unconnected-field errors in the retained catalog.
        case["primary"] = serde_json::json!({"Items":[{"Code":"A","Ordinal":1}]});
        let result = run(&project, &case);
        assert!(
            matches!(
                result,
                Err(engine::EngineError::Function(
                    functions::FunctionError::InvalidArgument {
                        function: "json_parse_array_length",
                        message: "input does not match the JSON schema"
                    }
                ))
            ),
            "{case}: {result:?}"
        );
    }
}

fn string_constant(project: &mapping::Project, node: mapping::NodeId) -> &str {
    let Node::Const {
        value: Value::String(value),
    } = &project.graph.nodes[&node]
    else {
        panic!("expected String descriptor: {node}");
    };
    value
}

#[test]
fn root_and_lookup_reads_keep_one_global_input_complete_schema_and_distinct_frames() {
    let project = imported(&fixtures().join("root-scalars/mapping.mfd"));
    let mut inputs = BTreeSet::new();
    let mut descriptors = BTreeSet::new();
    let mut fields = BTreeMap::new();
    let mut counts = BTreeMap::new();
    for (id, node) in &project.graph.nodes {
        let Node::Call { function, args } = node else {
            continue;
        };
        if !matches!(
            function.as_str(),
            "json_parse_field" | "json_parse_array_length" | "json_parse_array_field"
        ) {
            continue;
        }
        *counts.entry(function.as_str()).or_insert(0) += 1;
        inputs.insert(args[0]);
        descriptors.insert(string_constant(&project, args[1]));
        if function == "json_parse_field" {
            let path: Vec<String> =
                serde_json::from_str(string_constant(&project, args[2])).unwrap();
            assert_eq!(path.len(), 1);
            assert!(fields.insert(path[0].clone(), *id).is_none());
        }
    }
    assert_eq!(
        counts,
        BTreeMap::from([
            ("json_parse_field", 3),
            ("json_parse_array_length", 1),
            ("json_parse_array_field", 2),
        ])
    );
    assert_eq!(inputs.len(), 1, "all reads use the same global input");
    assert!(matches!(
        &project.graph.nodes[inputs.first().unwrap()],
        Node::RuntimeParameter { name, ty: ir::ScalarType::String, .. }
        if name == "LookupText"
    ));
    assert_eq!(descriptors.len(), 1, "all reads use the complete schema");
    let descriptor: ir::SchemaNode = serde_json::from_str(descriptors.first().unwrap()).unwrap();
    let descriptor_json = serde_json::to_value(descriptor).unwrap();
    let declared = &descriptor_json["kind"]["children"];
    for name in [
        "Label",
        "Revision",
        "Enabled",
        "Rows",
        "Detail",
        "ExtraRows",
        "Guard",
    ] {
        assert!(
            declared
                .as_array()
                .unwrap()
                .iter()
                .any(|node| node["name"] == name)
        );
    }
    assert_eq!(
        descriptor_json["kind"]["required"],
        serde_json::json!(["Enabled"])
    );

    assert!(project.root.source().unwrap().is_none());
    assert_eq!(project.root.bindings.len(), 3);
    for binding in &project.root.bindings {
        assert_eq!(Some(&binding.node), fields.get(&binding.target_field));
    }
    let [results] = project.root.children.as_slice() else {
        panic!("only the physical primary repetition owns Results");
    };
    assert_eq!(results.target_field, "Results");
    assert_eq!(results.source().unwrap().unwrap(), ["Items"]);
    let found = results
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Found")
        .unwrap();
    let Node::SequenceAggregate {
        function: AggregateOp::ItemAt,
        sequence:
            SequenceExpr::Generate {
                from: None,
                to,
                item,
            },
        predicate: Some(predicate),
        expression: Some(value),
        arg: Some(first),
    } = &project.graph.nodes[&found.node]
    else {
        panic!("lookup uses its private generated scan, not a physical source");
    };
    assert!(
        matches!(&project.graph.nodes[item], Node::SourceField { path, frame: None } if path.is_empty())
    );
    assert!(matches!(
        &project.graph.nodes[first],
        Node::Const {
            value: Value::Int(1)
        }
    ));
    let Node::Call {
        function,
        args: length_args,
    } = &project.graph.nodes[to]
    else {
        panic!("array length");
    };
    assert_eq!(function, "json_parse_array_length");
    assert_eq!(string_constant(&project, length_args[2]), r#"["Rows"]"#);
    let Node::Call { function, args } = &project.graph.nodes[predicate] else {
        panic!("lookup equality");
    };
    assert_eq!(function, "equal");
    assert_eq!(args.len(), 2);
    assert!(
        matches!(&project.graph.nodes[&args[1]], Node::SourceField { path, frame: Some(frame) } if path == &["Code"] && frame == &["Items"])
    );
    for (field, expected) in [(args[0], "Code"), (*value, "Value")] {
        let Node::Call { function, args } = &project.graph.nodes[&field] else {
            panic!("array projection");
        };
        assert_eq!(function, "json_parse_array_field");
        assert_eq!(&args[..3], length_args);
        assert_eq!(args[4], *item);
        assert_eq!(
            serde_json::from_str::<Vec<String>>(string_constant(&project, args[3])).unwrap(),
            [expected]
        );
    }
}

#[test]
fn root_scalar_composition_refuses_nested_structural_shadowed_and_shared_array_feeds() {
    for feed in ["24", "127", "128", "129", "130"] {
        let dir = copy_all_fixtures();
        let path = dir.0.join("root-scalars/mapping.mfd");
        replace(
            &path,
            "<vertex vertexkey=\"124\"><edges><edge vertexkey=\"134\"/></edges></vertex>",
            &format!(
                "<vertex vertexkey=\"{feed}\"><edges><edge vertexkey=\"134\"/></edges></vertex>"
            ),
        );
        refused(&path);
    }
    for (from, to) in [
        ("outkey=\"128\"", "outkey=\"124\""),
        ("outkey=\"2\"", "outkey=\"124\""),
        (
            "<parameter usageKind=\"stringparse\"/>",
            "<parameter usageKind=\"stringparse\"/><entry name=\"parent-context\"/>",
        ),
        (
            "</children>",
            "<component name=\"constant\" library=\"core\" kind=\"2\"><data><constant value=\"shadow\" datatype=\"string\"/></data><targets><datapoint key=\"124\"/></targets></component></children>",
        ),
        (
            "<vertex vertexkey=\"124\"><edges><edge vertexkey=\"134\"/></edges></vertex>",
            "<vertex vertexkey=\"23\"><edges><edge vertexkey=\"134\"/></edges></vertex>",
        ),
        (
            "<vertex vertexkey=\"125\"><edges><edge vertexkey=\"135\"/></edges></vertex>",
            "<vertex vertexkey=\"22\"><edges><edge vertexkey=\"135\"/></edges></vertex>",
        ),
        (
            "<vertex vertexkey=\"42\"><edges><edge vertexkey=\"51\"/></edges></vertex>",
            "<vertex vertexkey=\"42\"><edges><edge vertexkey=\"51\"/><edge vertexkey=\"136\"/></edges></vertex>",
        ),
    ] {
        let dir = copy_all_fixtures();
        let path = dir.0.join("root-scalars/mapping.mfd");
        if to.contains("<edge vertexkey=\"136\"/>") {
            replace(
                &path,
                "<vertex vertexkey=\"126\"><edges><edge vertexkey=\"136\"/></edges></vertex>",
                "",
            );
        }
        replace(&path, from, to);
        refused(&path);
    }
    for replacement in [
        "\"Label\": {}",
        "\"Label\": {\"type\":\"array\",\"items\":{\"type\":\"string\"}}",
    ] {
        let dir = copy_all_fixtures();
        let root = dir.0.join("root-scalars");
        replace(
            &root.join("lookup.schema.json"),
            "\"Label\": {\"type\":[\"string\",\"null\"]}",
            replacement,
        );
        refused(&root.join("mapping.mfd"));
    }
}

#[test]
fn filtered_final_key_branch_remains_refused_without_a_reference_backed_proof() {
    let questions: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../fixtures/json-array-lookup/filtered-key/reference-cases.json"
    ))
    .unwrap();
    assert_eq!(questions.len(), 25);
    for question in questions {
        assert!(
            question["observe"]
                .as_str()
                .is_some_and(|text| !text.is_empty())
        );
        assert!(
            question.get("expected").is_none(),
            "reference questions are not oracles"
        );
    }
    refused(&fixtures().join("filtered-key/mapping.mfd"));
    let dir = copy_all_fixtures();
    let path = dir.0.join("filtered-key/mapping.mfd");
    replace(
        &path,
        "</vertices>",
        "<vertex vertexkey=\"124\"><edges><edge vertexkey=\"134\"/></edges></vertex></vertices>",
    );
    refused(&path);
}

#[test]
fn root_scalar_lookup_graph_lowers_and_emits_both_backends() {
    let dir = TempDir::new();
    emit(&dir.0, "root-scalars");
}

#[test]
#[ignore = "compiles generated Rust and C# Release hosts; requires Cargo and .NET 10"]
fn generated_root_scalar_lookup_release_hosts_match_complete_outputs_and_typed_failures() {
    generated_hosts("root-scalars");
}
