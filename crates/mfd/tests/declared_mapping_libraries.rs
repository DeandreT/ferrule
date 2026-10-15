//! Generic contracts authored source-only. These tests have not been executed
//! as part of the source-preparation task; no original mapping admission.
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FormatOptions, FunctionId, FunctionParameter, FunctionParameterId, Graph, Node,
    Project, Scope, UserFunction,
};
use mfd::{ImportOptions, ResourceBindingKindV1, ResourceBindingV1, ResourceReadSetV1};
use sha2::{Digest, Sha256};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_declared_libraries_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn digest(text: &str) -> [u8; 32] {
    Sha256::digest(text.as_bytes()).into()
}
fn scalar(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}
fn function(library: &str, suffix: &str) -> UserFunction {
    UserFunction {
        library: library.into(),
        name: "Echo".into(),
        description: None,
        parameters: vec![FunctionParameter {
            id: FunctionParameterId::new(1),
            name: "value".into(),
            ty: ScalarType::String,
        }],
        output_name: "result".into(),
        output_type: ScalarType::String,
        body: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::FunctionParameter {
                        parameter: FunctionParameterId::new(1),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String(suffix.into()),
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![0, 1],
                    },
                ),
            ]),
        },
        output: 2,
    }
}
fn project() -> Project {
    Project {
        source: SchemaNode::group("Input", vec![scalar("Value")]),
        target: SchemaNode::group("Output", vec![scalar("A"), scalar("B"), scalar("Local")]),
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        target_options: FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::from([
            (FunctionId::new(1), function("alpha", "-A")),
            (FunctionId::new(2), function("beta", "-B")),
            (FunctionId::new(3), function("local", "-local")),
        ]),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::UserFunctionCall {
                        function: FunctionId::new(1),
                        args: vec![0],
                    },
                ),
                (
                    2,
                    Node::UserFunctionCall {
                        function: FunctionId::new(2),
                        args: vec![0],
                    },
                ),
                (
                    3,
                    Node::UserFunctionCall {
                        function: FunctionId::new(3),
                        args: vec![0],
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: ["A", "B", "Local"]
                .into_iter()
                .enumerate()
                .map(|(i, name)| Binding {
                    target_field: name.into(),
                    node: i as u32 + 1,
                })
                .collect(),
            ..Default::default()
        },
    }
}
fn import_tag(path: &str) -> String {
    format!(r#"<import path="{path}" saverelative="1"/>"#)
}
fn library(definition: &str) -> String {
    // Deliberately unusable unrelated main mapping. Ingestion must not inspect
    // its schema, payload, database, HTTP endpoint or adjacent module names.
    format!(
        r#"<mapping version="26"><component name="unused-main"><structure><children>
      <component library="xml" name="unavailable" kind="14"><data><document schema="DO-NOT-OPEN.xsd" inputinstance="DO-NOT-READ.xml"/></data></component>
      <component library="http" name="DO-NOT-CALL"/>
      <component library="db" name="DO-NOT-QUERY"/>
      </children></structure></component>{definition}</mapping>"#
    )
}
fn setup() -> (Directory, PathBuf, Project, BTreeMap<String, String>) {
    setup_project(project())
}
fn setup_project(project: Project) -> (Directory, PathBuf, Project, BTreeMap<String, String>) {
    let dir = Directory::new();
    let path = dir.0.join("mapping.mfd");
    mfd::export(&project, &path).unwrap();
    let embedded = mfd::import(&path).unwrap();
    assert!(embedded.warnings.is_empty(), "{:?}", embedded.warnings);
    let mut xml = std::fs::read_to_string(&path).unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let declarations = doc
        .root_element()
        .children()
        .filter(|n| {
            n.has_tag_name("component") && matches!(n.attribute("library"), Some("alpha" | "beta"))
        })
        .map(|n| (n.attribute("library").unwrap().to_string(), n.range()))
        .collect::<Vec<_>>();
    let mut libraries = BTreeMap::new();
    for (name, range) in &declarations {
        let text = library(&xml[range.clone()]);
        std::fs::write(dir.0.join(format!("{name}.mfd")), &text).unwrap();
        libraries.insert(name.clone(), text);
    }
    for (_, range) in declarations.into_iter().rev() {
        xml.replace_range(range, "");
    }
    xml = xml.replacen(
        "</mapping>",
        &format!(
            "<settings><libraries>{}{}</libraries></settings></mapping>",
            import_tag("alpha.mfd"),
            import_tag("beta.mfd")
        ),
        1,
    );
    std::fs::write(&path, xml).unwrap();
    (dir, path, embedded.project, libraries)
}

#[test]
fn imported_lazy_branch_keeps_the_scalar_value_and_error_contract() {
    let mut oracle = project();
    let function = oracle.user_functions.get_mut(&FunctionId::new(1)).unwrap();
    function.body.nodes = BTreeMap::from([
        (
            0,
            Node::FunctionParameter {
                parameter: FunctionParameterId::new(1),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String("skip".into()),
            },
        ),
        (
            2,
            Node::Call {
                function: "equal".into(),
                args: vec![0, 1],
            },
        ),
        (
            3,
            Node::Const {
                value: Value::String("safe".into()),
            },
        ),
        (
            4,
            Node::Const {
                value: Value::String("[Y]-[M]-[D]".into()),
            },
        ),
        (
            5,
            Node::Call {
                function: "parse_date".into(),
                args: vec![0, 4],
            },
        ),
        (
            6,
            Node::If {
                condition: 2,
                then: 3,
                else_: 5,
            },
        ),
    ]);
    function.output = 6;
    let (_dir, path, embedded, _) = setup_project(oracle);
    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        serde_json::to_value(&imported.project).unwrap(),
        serde_json::to_value(&embedded).unwrap()
    );
    for value in ["skip", "2026-10-15", "not-a-date"] {
        let source = format_xml::from_str(
            &format!("<Input><Value>{value}</Value></Input>"),
            &imported.project.source,
        )
        .unwrap();
        match (
            engine::run(&embedded, &source),
            engine::run(&imported.project, &source),
        ) {
            (Ok(expected), Ok(actual)) => {
                assert_eq!(actual, expected);
                if value == "skip" {
                    assert_eq!(
                        actual.field("A").and_then(Instance::as_scalar),
                        Some(&Value::String("safe".into()))
                    );
                }
            }
            (Err(expected), Err(actual)) => {
                assert_eq!(actual.to_string(), expected.to_string());
                assert!(
                    matches!(actual, engine::EngineError::Function(functions::FunctionError::InvalidArgument { function, .. }) if function == "parse_date")
                );
            }
            results => panic!("mismatched scalar contract: {results:?}"),
        }
    }
}

fn disconnect_alpha_input(path: &std::path::Path) {
    let mut xml = std::fs::read_to_string(path).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let call = document
        .descendants()
        .find(|n| {
            n.has_tag_name("component")
                && n.attribute("library") == Some("alpha")
                && n.attribute("kind") == Some("19")
        })
        .unwrap();
    let key = call
        .descendants()
        .find_map(|n| n.attribute("inpkey"))
        .unwrap();
    let range = document
        .descendants()
        .find(|n| n.has_tag_name("edge") && n.attribute("vertexkey") == Some(key))
        .unwrap()
        .range();
    xml.replace_range(range, "");
    std::fs::write(path, xml).unwrap();
}

#[test]
fn required_missing_input_is_not_a_null_stub_and_typed_default_is_owner_local() {
    let (dir, path, _, libraries) = setup();
    disconnect_alpha_input(&path);
    let imported = mfd::import(&path).unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .any(|w| w.contains("required imported parameter")
                && w.contains("no connected producer")),
        "{:?}",
        imported.warnings
    );

    let original = &libraries["alpha"];
    let document = roxmltree::Document::parse(original).unwrap();
    let parameter = document
        .descendants()
        .find(|n| n.has_tag_name("component") && n.attribute("kind") == Some("6"))
        .unwrap();
    let range = parameter.range();
    let mut replacement = original[range.clone()].replace(
        "<targets>",
        "<sources><datapoint pos=\"0\" key=\"90000\"/></sources><targets>",
    );
    replacement = replacement.replace("usageKind=\"input\"", "usageKind=\"input\" optional=\"1\"");
    let mut defaulted = original.to_string();
    defaulted.replace_range(range, &replacement);
    // Insert only into the function's own children and graph, never the unused main.
    let document = roxmltree::Document::parse(&defaulted).unwrap();
    let definition = document
        .root_element()
        .children()
        .find(|n| n.attribute("library") == Some("alpha"))
        .unwrap();
    let graph = definition
        .descendants()
        .find(|n| n.has_tag_name("vertices"))
        .unwrap();
    let graph_at = graph.range().end - "</vertices>".len();
    let children = definition
        .descendants()
        .find(|n| n.has_tag_name("children"))
        .unwrap();
    let children_at = children.range().end - "</children>".len();
    defaulted.insert_str(
        graph_at,
        "<vertex vertexkey=\"90001\"><edges><edge vertexkey=\"90000\"/></edges></vertex>",
    );
    defaulted.insert_str(children_at, "<component library=\"core\" name=\"default\" kind=\"2\" uid=\"90001\"><targets><datapoint key=\"90001\"/></targets><data><constant datatype=\"string\" value=\"fallback\"/></data></component>");
    std::fs::write(dir.0.join("alpha.mfd"), &defaulted).unwrap();
    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let input =
        format_xml::from_str("<Input><Value>x</Value></Input>", &imported.project.source).unwrap();
    let output = engine::run(&imported.project, &input).unwrap();
    assert_eq!(
        output.field("A").and_then(Instance::as_scalar),
        Some(&Value::String("fallback-A".into()))
    );
    std::fs::write(
        dir.0.join("alpha.mfd"),
        defaulted.replace("<input datatype=\"string\"/>", "<input/>"),
    )
    .unwrap();
    let imported = mfd::import(&path).unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .any(|w| w.contains("parameter type/default contract")),
        "{:?}",
        imported.warnings
    );
    assert!(
        !imported
            .project
            .user_functions
            .values()
            .any(|f| f.library == "alpha")
    );
}

#[test]
fn local_plus_two_declared_namespaces_match_the_complete_embedded_project() {
    let (dir, path, embedded, libraries) = setup();
    let readset = ResourceReadSetV1::default();
    let options = ImportOptions::default().with_resource_readset_v1(readset.clone());
    let imported = mfd::import_with_options(&path, &options).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        serde_json::to_value(&imported.project).unwrap(),
        serde_json::to_value(&embedded).unwrap()
    );
    assert_eq!(readset.records().len(), 2);
    for (name, text) in libraries {
        assert!(
            readset
                .records()
                .iter()
                .any(|r| r.kind == ResourceBindingKindV1::MappingLibrary
                    && r.original_reference == format!("{name}.mfd")
                    && r.sha256 == digest(&text)
                    && r.delivered)
        );
    }
    let definitions = readset.mapping_library_definitions();
    assert_eq!(definitions.len(), 2);
    assert!(
        definitions
            .iter()
            .all(|r| r.refusal.is_none() && r.declaration_xml.contains("datatype=\"string\""))
    );
    assert!(engine::validate(&imported.project).is_empty());
    let input =
        format_xml::from_str("<Input><Value>x</Value></Input>", &imported.project.source).unwrap();
    let output = engine::run(&imported.project, &input).unwrap();
    for (name, expected) in [("A", "x-A"), ("B", "x-B"), ("Local", "x-local")] {
        assert_eq!(
            output.field(name).and_then(Instance::as_scalar),
            Some(&Value::String(expected.into()))
        );
    }
    let roundtrip = dir.0.join("portable.mfd");
    mfd::export(&imported.project, &roundtrip).unwrap();
    let saved = std::fs::read_to_string(&roundtrip).unwrap();
    assert!(!saved.contains("<import "));
    let restored = mfd::import(&roundtrip).unwrap();
    assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
    assert_eq!(engine::run(&restored.project, &input).unwrap(), output);
}

#[test]
fn relocation_hashes_the_exact_delivered_library_and_never_falls_back() {
    let (dir, path, _, libraries) = setup();
    let text = &libraries["alpha"];
    std::fs::create_dir(dir.0.join("approved")).unwrap();
    std::fs::write(dir.0.join("approved/relocated.mfd"), text).unwrap();
    let binding = ResourceBindingV1::new(
        ResourceBindingKindV1::MappingLibrary,
        &path,
        "alpha.mfd",
        dir.0.join("approved"),
        "relocated.mfd",
        digest(text),
    )
    .unwrap()
    .with_origin_sha256(digest(&std::fs::read_to_string(&path).unwrap()))
    .prepare()
    .unwrap();
    std::fs::write(
        dir.0.join("approved/relocated.mfd"),
        "changed after preparation",
    )
    .unwrap();
    let sink = ResourceReadSetV1::default();
    let options = ImportOptions::default()
        .with_resource_bindings_v1([binding])
        .unwrap()
        .with_resource_readset_v1(sink.clone());
    let imported = mfd::import_with_options(&path, &options).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        sink.records()
            .iter()
            .any(|r| r.from_prepared_snapshot && r.sha256 == digest(text))
    );
    let bad = ResourceBindingV1::new(
        ResourceBindingKindV1::MappingLibrary,
        &path,
        "alpha.mfd",
        &dir.0,
        "alpha.mfd",
        [0; 32],
    )
    .unwrap();
    let options = ImportOptions::default()
        .with_resource_bindings_v1([bad])
        .unwrap();
    assert!(
        mfd::import_with_options(&path, &options)
            .unwrap_err()
            .to_string()
            .contains("SHA-256 mismatch")
    );
}

#[test]
fn transitive_prepared_bytes_and_dependency_mismatch_use_mapping_library_kind() {
    let (dir, path, _, libraries) = setup();
    let leaf = "<mapping/>";
    let root = libraries["alpha"].replace(
        "</mapping>",
        &format!(
            "<settings><libraries>{}</libraries></settings></mapping>",
            import_tag("leaf.mfd")
        ),
    );
    std::fs::write(dir.0.join("alpha.mfd"), &root).unwrap();
    std::fs::write(dir.0.join("leaf.mfd"), leaf).unwrap();
    let binding = ResourceBindingV1::new(
        ResourceBindingKindV1::MappingLibrary,
        &path,
        "alpha.mfd",
        &dir.0,
        "alpha.mfd",
        digest(&root),
    )
    .unwrap()
    .with_dependency_pin("leaf.mfd", digest(leaf))
    .unwrap()
    .prepare()
    .unwrap();
    std::fs::write(dir.0.join("leaf.mfd"), "changed dependency").unwrap();
    let sink = ResourceReadSetV1::default();
    let options = ImportOptions::default()
        .with_resource_bindings_v1([binding])
        .unwrap()
        .with_resource_readset_v1(sink.clone());
    assert!(
        mfd::import_with_options(&path, &options)
            .unwrap()
            .warnings
            .is_empty()
    );
    assert!(sink.records().iter().any(|r| r.sha256 == digest(leaf)
        && r.from_prepared_snapshot
        && r.kind == ResourceBindingKindV1::MappingLibrary));
    let binding = ResourceBindingV1::new(
        ResourceBindingKindV1::MappingLibrary,
        &path,
        "alpha.mfd",
        &dir.0,
        "alpha.mfd",
        digest(&root),
    )
    .unwrap()
    .with_dependency_pin("leaf.mfd", digest(leaf))
    .unwrap();
    let options = ImportOptions::default()
        .with_resource_bindings_v1([binding])
        .unwrap();
    assert!(
        mfd::import_with_options(&path, &options)
            .unwrap_err()
            .to_string()
            .contains("SHA-256 mismatch")
    );
}

#[test]
fn missing_escape_cycle_unknown_format_and_unknown_effect_remain_diagnostics() {
    for (replacement, expected) in [
        (None, "unresolved resource"),
        (Some("<not-a-mapping/>".into()), "root must be"),
        (
            Some(format!(
                "<mapping><settings><libraries>{}</libraries></settings></mapping>",
                import_tag("alpha.mfd")
            )),
            "cycle",
        ),
        (
            Some(format!(
                "<mapping><settings><libraries>{}</libraries></settings></mapping>",
                import_tag("../outside.mfd")
            )),
            "escapes",
        ),
    ] {
        let (dir, path, _, _) = setup();
        match replacement {
            Some(text) => std::fs::write(dir.0.join("alpha.mfd"), text).unwrap(),
            None => std::fs::remove_file(dir.0.join("alpha.mfd")).unwrap(),
        }
        let imported = mfd::import(&path).unwrap();
        assert!(
            imported.warnings.iter().any(|w| w.contains(expected)),
            "{:?}",
            imported.warnings
        );
        assert!(
            !imported
                .project
                .user_functions
                .values()
                .any(|f| f.library == "alpha")
        );
    }
    let (dir, path, _, libraries) = setup();
    let bad = libraries["alpha"].replace("name=\"concat\"", "name=\"sleep\"");
    std::fs::write(dir.0.join("alpha.mfd"), bad).unwrap();
    let imported = mfd::import(&path).unwrap();
    assert!(
        imported.warnings.iter().any(|w| w.contains("effectful")),
        "{:?}",
        imported.warnings
    );
    assert!(
        !imported
            .project
            .user_functions
            .values()
            .any(|f| f.library == "alpha")
    );
}
