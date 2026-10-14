use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration, ScopeSequence};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/json-enclosing-frame-clones").join(name)
}

struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!("ferrule_json_implicit_{}_{}",
            std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&path).unwrap();
        for name in ["source.schema.json", "target.schema.json"] {
            std::fs::copy(fixture(name), path.join(name)).unwrap();
        }
        Self(path)
    }

    fn import(&self, design: &str) -> mfd::Imported {
        let path = self.0.join("mapping.mfd");
        std::fs::write(&path, design).unwrap();
        mfd::import(&path).unwrap()
    }

    fn schema(&self, name: &str, edit: impl FnOnce(&mut serde_json::Value)) {
        let path = self.0.join(name);
        let mut value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        edit(&mut value);
        std::fs::write(path, serde_json::to_string(&value).unwrap()).unwrap();
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

fn assert_refused(project: &Project) {
    fn any_concatenation(scope: &Scope) -> bool {
        scope.concatenated().is_some() || scope.children.iter().any(any_concatenation)
    }
    assert!(!any_concatenation(&project.root), "unexpected occurrence scope: {:?}", project.root);
}

fn required_group(name: &str, children: Vec<SchemaNode>) -> SchemaNode {
    let required = children.iter().map(|child| child.name.clone()).collect();
    SchemaNode::group(name, children).with_required_fields(required).unwrap()
}

fn row_children() -> Vec<SchemaNode> {
    vec![
        SchemaNode::scalar("Flag", ScalarType::Bool),
        SchemaNode::scalar("Id", ScalarType::String),
        SchemaNode::scalar("Zero", ScalarType::Int),
    ]
}

fn expected_project(literals: bool) -> Project {
    let source = required_group("Source", vec![required_group("Rows", row_children()).repeating()]);
    let mut record_children = vec![SchemaNode::scalar("Branch", ScalarType::String)];
    record_children.extend(row_children());
    let target = required_group("Target", vec![required_group("Batches", vec![
        required_group("Records", record_children).repeating(),
    ]).repeating()]);
    let frame = Some(vec!["Rows".into()]);
    let mut graph = Graph {
        nodes: BTreeMap::from([
            (0, Node::Const { value: Value::String("left".into()) }),
            (1, Node::Const { value: Value::String("right".into()) }),
            (2, Node::SourceField { path: vec!["Flag".into()], frame: frame.clone() }),
            (3, Node::SourceField { path: vec!["Id".into()], frame: frame.clone() }),
            (4, Node::SourceField { path: vec!["Zero".into()], frame }),
        ]),
    };
    if literals {
        graph.nodes.insert(2, Node::Const { value: Value::Bool(false) });
        graph.nodes.insert(3, Node::Const { value: Value::String("".into()) });
        graph.nodes.insert(4, Node::Const { value: Value::Int(0) });
    }
    let segment = |branch_node| Scope {
        bindings: vec![
            Binding { target_field: "Branch".into(), node: branch_node },
            Binding { target_field: "Flag".into(), node: 2 },
            Binding { target_field: "Id".into(), node: 3 },
            Binding { target_field: "Zero".into(), node: 4 },
        ],
        ..Default::default()
    };
    Project {
        source,
        target,
        source_path: Some("source.json".into()),
        target_path: Some("target.json".into()),
        source_options: FormatOptions { json_document: true, ..Default::default() },
        target_options: FormatOptions { json_document: true, ..Default::default() },
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "Batches".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                children: vec![Scope {
                    target_field: "Records".into(),
                    iteration: ScopeIteration::Concatenate(ScopeSequence::new(segment(0), vec![segment(1)])),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}


// These whole-Project expectations are authored independently of the importer.
// Each variant must preserve schemas, graph allocation, paths, options and scopes.
#[test]
fn ordinary_json_implicit_occurrences_keep_complete_project_order_and_cardinality() {
    for name in ["implicit.mfd", "reversed.mfd", "wrapped.mfd", "literals.mfd"] {
        let imported = mfd::import(&fixture(name)).unwrap();
        assert!(imported.warnings.is_empty(), "{name}: {:?}", imported.warnings);
        assert_eq!(serde_json::to_value(&imported.project).unwrap(),
            serde_json::to_value(expected_project(name == "literals.mfd")).unwrap(), "{name}");
        assert!(engine::validate(&imported.project).is_empty(), "{name}");
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "fixtures/json-enclosing-frame-clones/output-oracles.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let input = format_json::from_str(&case["input"].to_string(), &imported.project.source).unwrap();
            let output = engine::run(&imported.project, &input).unwrap();
            let actual: serde_json::Value = serde_json::from_str(
                &format_json::to_string(&imported.project.target, &output).unwrap()).unwrap();
            let mut expected = case["expected"].clone();
            if name == "literals.mfd" {
                for batch in expected["Batches"].as_array_mut().unwrap() {
                    for record in batch["Records"].as_array_mut().unwrap() {
                        record["Flag"] = serde_json::json!(false);
                        record["Id"] = serde_json::json!("");
                        record["Zero"] = serde_json::json!(0);
                    }
                }
            }
            assert_eq!(actual, expected, "{name}: {}", case["name"]);
            assert_eq!(actual["Batches"].as_array().unwrap().len(),
                case["input"]["Rows"].as_array().unwrap().len());
            assert!(actual["Batches"].as_array().unwrap().iter()
                .all(|batch| batch["Records"].as_array().unwrap().len() == 2));
        }
    }
}

#[test]
fn ordinary_json_input_boundary_rejects_missing_null_and_wrong_type_before_execution() {
    let project = expected_project(false);
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/json-enclosing-frame-clones/boundary-oracles.json")).unwrap();
    for case in cases.as_array().unwrap() {
        assert!(format_json::from_str(&case["input"].to_string(), &project.source).is_err(),
            "{}", case["name"]);
    }
    for invalid in [
        serde_json::json!({"Batches":null}),
        serde_json::json!({"Batches":[null]}),
        serde_json::json!({"Batches":[{"Records":null}]}),
        serde_json::json!({"Batches":[{"Records":[null]}]}),
        serde_json::json!({"Batches":[{"Records":[{"Branch":"left","Flag":false,"Zero":0}]}]}),
    ] {
        assert!(format_json::from_str(&invalid.to_string(), &project.target).is_err());
    }
}

#[test]
fn json_implicit_nullable_array_containers_and_object_items_refuse_independently() {
    let text = std::fs::read_to_string(fixture("implicit.mfd")).unwrap();
    for (file, pointer) in [
        ("source.schema.json", "/properties/Rows"),
        ("target.schema.json", "/properties/Batches"),
        ("target.schema.json", "/properties/Batches/items/properties/Records"),
    ] {
        for domain in ["container", "item", "both"] {
            let dir = FixtureDir::new();
            dir.schema(file, |schema| {
                let array = schema.pointer_mut(pointer).unwrap();
                if domain != "item" { array["type"] = serde_json::json!(["array", "null"]); }
                if domain != "container" { array["items"]["type"] = serde_json::json!(["object", "null"]); }
            });
            assert_refused(&dir.import(&text).project);
        }
    }
}

#[test]
fn json_implicit_optional_or_nullable_physical_leaf_and_incomplete_required_target_refuse() {
    let text = std::fs::read_to_string(fixture("implicit.mfd")).unwrap();
    for mode in 0..5 {
        let dir = FixtureDir::new();
        if mode < 4 {
            dir.schema("source.schema.json", |schema| match mode {
                0 => schema["required"] = serde_json::json!([]),
                1 => schema["properties"]["Rows"]["items"]["required"] = serde_json::json!(["Flag", "Zero"]),
                2 => schema["properties"]["Rows"]["items"]["properties"]["Id"]["type"] = serde_json::json!(["string", "null"]),
                _ => schema["properties"]["Rows"]["items"]["properties"]["Zero"]["type"] = serde_json::json!("number"),
            });
        }
        let text = if mode == 4 { text.replace(r#"<edge from="13" to="64"/>"#, "") } else { text.clone() };
        assert_refused(&dir.import(&text).project);
    }
}

#[test]
fn json_implicit_requires_plain_closed_authoritative_schemas() {
    let text = std::fs::read_to_string(fixture("implicit.mfd")).unwrap();
    for mode in 0..6 {
        let dir = FixtureDir::new();
        dir.schema("source.schema.json", |schema| match mode {
            0 => schema["additionalProperties"] = serde_json::json!(true),
            1 => schema["properties"]["Rows"]["minItems"] = serde_json::json!(1),
            2 => schema["properties"]["Rows"]["items"]["properties"]["Id"]["minLength"] = serde_json::json!(1),
            3 => schema["properties"]["Rows"]["uniqueItems"] = serde_json::json!(true),
            4 => schema["properties"]["Rows"]["items"]["properties"]["Zero"]["minimum"] = serde_json::json!(0),
            _ => schema["properties"]["Rows"]["items"]["properties"]["Flag"]["const"] = serde_json::json!(false),
        });
        assert_refused(&dir.import(&text).project);
    }
    for name in ["source", "target"] {
        for replacement in [String::new(), format!(r#"schema="missing-{name}.json""#)] {
            let dir = FixtureDir::new();
            let text = text.replace(&format!(r#"schema="{name}.schema.json""#), &replacement);
            assert_refused(&dir.import(&text).project);
        }
    }
}

#[test]
fn json_implicit_raw_literal_schema_roles_and_boundary_controls_cannot_normalize_into_proofs() {
    let text = std::fs::read_to_string(fixture("implicit.mfd")).unwrap();
    let edits = [
        (r#"value="left" datatype="string""#, r#"value="left""#),
        (r#"value="left" datatype="string""#, r#"datatype="string""#),
        (r#"value="left" datatype="string""#, r#"value="1" datatype="integer""#),
        (r#"<constant value="left" datatype="string"/>"#, r#"<constant value="left" datatype="string"/><constant value="ignored" datatype="string"/>"#),
        (r#"<constant value="left" datatype="string"/>"#, r#"<constant value="left" datatype="string"><unknown/></constant>"#),
        (r#"<entry name="object" outkey="10">"#, r#"<entry name="string" outkey="10">"#),
        (r#"<entry name="object" inpkey="40">"#, r#"<entry name="string" inpkey="40">"#),
        (r#"<entry name="integer" outkey="13"/>"#, r#"<entry name="string" outkey="13"/>"#),
        (r#"<entry name="integer" inpkey="64"/>"#, r#"<entry name="string" inpkey="64"/>"#),
        (r#"<entry name="string" outkey="12"/>"#, r#"<entry name="string" outkey="12"><outputnodefunctions/></entry>"#),
        (r#"<entry name="string" inpkey="63"/>"#, r#"<entry name="string" inpkey="63"><inputnodefunctions/></entry>"#),
        (r#"<entry name="string" inpkey="63"/>"#, r#"<entry name="string" inpkey="63" casttotargettypemode="1"/>"#),
        (r#"inputinstance="source.json""#, r#"inputinstance="source.json" jsonlines="1""#),
        (r#"outputinstance="target.json""#, r#"outputinstance="target.json" jsonlines="1""#),
        (r#"inputinstance="source.json""#, r#"inputinstance="source.JSON5""#),
        (r#"outputinstance="target.json""#, r#"outputinstance="target.JSON5""#),
    ];
    for (before, after) in edits {
        assert!(text.contains(before), "missing mutation operand {before}");
        assert_refused(&FixtureDir::new().import(&text.replace(before, after)).project);
    }
}

#[test]
fn json_implicit_raw_object_alternatives_wrapper_ambiguity_and_nested_clones_refuse() {
    let text = std::fs::read_to_string(fixture("implicit.mfd")).unwrap();
    for (before, after) in [
        (r#"<entry name="object" clone="1"/>"#, r#"<entry name="null"/>"#),
        (r#"<entry name="object" clone="1"/>"#, r#"<entry name="object" type="json-subtype"/>"#),
        (r#"<entry name="Records" type="json-property"><entry name="array">"#, r#"<entry name="Records" type="json-property"><entry name="array"/><entry name="array">"#),
        (r#"<entry name="Batches" type="json-property"><entry name="array"><entry name="item" type="json-item">"#, r#"<entry name="Batches" type="json-property"><entry name="array"><entry name="item" type="json-item"><entry name="object"/></entry><entry name="item" type="json-item">"#),
        (r#"<entry name="object" inpkey="40">"#, r#"<entry name="object"/><entry name="object" inpkey="40">"#),
    ] {
        assert!(text.contains(before));
        assert_refused(&FixtureDir::new().import(&text.replace(before, after)).project);
    }
}

#[test]
fn json_implicit_optional_unbound_nested_object_is_omitted_with_complete_project() {
    let dir = FixtureDir::new();
    dir.schema("target.schema.json", |schema| {
        schema["properties"]["Batches"]["items"]["properties"]["Records"]["items"]["properties"]["Meta"] =
            serde_json::json!({"type":"object","properties":{"Need":{"type":"string"}},"required":["Need"],"additionalProperties":false});
    });
    let text = std::fs::read_to_string(fixture("implicit.mfd")).unwrap();
    let imported = dir.import(&text);
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut expected = expected_project(false);
    let ir::SchemaKind::Group { children, .. } = &mut expected.target.kind else { panic!("target"); };
    let ir::SchemaKind::Group { children, .. } = &mut children[0].kind else { panic!("batches"); };
    let ir::SchemaKind::Group { children, .. } = &mut children[0].kind else { panic!("records"); };
    children.insert(3, required_group("Meta", vec![SchemaNode::scalar("Need", ScalarType::String)]));
    assert_eq!(serde_json::to_value(imported.project).unwrap(), serde_json::to_value(expected).unwrap());
}

fn write_artifacts(directory: &Path, artifacts: &codegen::ArtifactSet) -> std::io::Result<()> {
    for file in artifacts.files() {
        let path = directory.join(file.path.as_str());
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
        std::fs::write(path, &file.contents)?;
    }
    Ok(())
}

#[test]
fn authored_json_implicit_projects_lower_for_generated_rust_and_csharp_qualification() {
    for name in ["implicit.mfd", "reversed.mfd", "wrapped.mfd", "literals.mfd"] {
        let imported = mfd::import(&fixture(name)).unwrap();
        assert!(imported.warnings.is_empty(), "{name}: {:?}", imported.warnings);
        assert_eq!(serde_json::to_value(&imported.project).unwrap(),
            serde_json::to_value(expected_project(name == "literals.mfd")).unwrap(), "{name}");
        let project = imported.project;
        let program = codegen::lower(&project).unwrap();
        let rust = codegen_rust::emit(&program, &codegen_rust::Options {
            package_name: "ferrule-json-implicit-occurrences".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime")
                    .canonicalize().unwrap().display().to_string()),
        }).unwrap();
        let csharp = codegen_csharp::emit(&program).unwrap();
        if let Some(root) = std::env::var_os("FERRULE_JSON_IMPLICIT_ARTIFACT_ROOT") {
            let output = PathBuf::from(root).join(name.trim_end_matches(".mfd"));
            std::fs::create_dir(&output).unwrap();
            write_artifacts(&output.join("rust"), &rust).unwrap();
            write_artifacts(&output.join("csharp"), &csharp).unwrap();
        }
    }
}

fn authored_many_branches(count: usize, extra_binding: bool) -> String {
    let text = std::fs::read_to_string(fixture("implicit.mfd")).unwrap();
    let start = text.find("<entry name=\"object\"><entry name=\"Branch\"").unwrap();
    let end = text.find("<entry name=\"object\" clone=\"1\"/>").unwrap()
        + "<entry name=\"object\" clone=\"1\"/>".len();
    let mut objects = String::new();
    let mut edges = String::from(r#"<edge from="10" to="40"/>"#);
    for index in 0..count {
        let first = 1000 + index * 4;
        objects.push_str(&format!(r#"<entry name="object"><entry name="Branch" type="json-property"><entry name="string" inpkey="{first}"/></entry><entry name="Flag" type="json-property"><entry name="boolean" inpkey="{}"/></entry><entry name="Id" type="json-property"><entry name="string" inpkey="{}"/></entry><entry name="Zero" type="json-property"><entry name="integer" inpkey="{}"/></entry>"#,
            first + 1, first + 2, first + 3));
        for (feed, input) in [(101, first), (11, first + 1), (12, first + 2), (13, first + 3)] {
            edges.push_str(&format!(r#"<edge from="{feed}" to="{input}"/>"#));
        }
        if extra_binding && index + 1 == count {
            objects.push_str(r#"<entry name="Extra" type="json-property"><entry name="string" inpkey="2000"/></entry>"#);
            edges.push_str(r#"<edge from="102" to="2000"/>"#);
        }
        objects.push_str("</entry>");
    }
    let mut design = text[..start].to_string();
    design.push_str(&objects);
    design.push_str(&text[end..]);
    let start = design.find("<connections>").unwrap();
    let end = design.find("</connections>").unwrap();
    design.replace_range(start..end, &format!("<connections>{edges}"));
    design
}

#[test]
fn json_implicit_exact_64_branch_256_binding_limit_keeps_all_equal_feeds_and_full_project() {
    let imported = FixtureDir::new().import(&authored_many_branches(64, false));
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut expected = expected_project(false);
    let mut segment = Scope::default();
    segment.bindings = vec![
        Binding { target_field: "Branch".into(), node: 0 },
        Binding { target_field: "Flag".into(), node: 2 },
        Binding { target_field: "Id".into(), node: 3 },
        Binding { target_field: "Zero".into(), node: 4 },
    ];
    expected.root.children[0].children[0].iteration = ScopeIteration::Concatenate(
        ScopeSequence::new(segment.clone(), vec![segment; 63]));
    assert_eq!(serde_json::to_value(&imported.project).unwrap(), serde_json::to_value(expected).unwrap());
    let input = format_json::from_str(r#"{"Rows":[{"Flag":false,"Id":"","Zero":0}]}"#,
        &imported.project.source).unwrap();
    let actual = engine::run(&imported.project, &input).unwrap();
    let actual: serde_json::Value = serde_json::from_str(
        &format_json::to_string(&imported.project.target, &actual).unwrap()).unwrap();
    let record = serde_json::json!({"Branch":"left","Flag":false,"Id":"","Zero":0});
    assert_eq!(actual, serde_json::json!({"Batches":[{"Records":vec![record; 64]}]}));
}

#[test]
fn json_implicit_65_branches_or_257_bindings_refuse_without_partial_occurrences() {
    assert_refused(&FixtureDir::new().import(&authored_many_branches(65, false)).project);
    let dir = FixtureDir::new();
    dir.schema("target.schema.json", |schema| {
        schema["properties"]["Batches"]["items"]["properties"]["Records"]["items"]["properties"]["Extra"] =
            serde_json::json!({"type":"string"});
    });
    assert_refused(&dir.import(&authored_many_branches(64, true)).project);
}
