use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{
    IntegerRange, ItemCountRange, NumericRange, ScalarType, SchemaNode, StringLengthRange, Value,
};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration};

const PREVIEW: &str = r#"{"Header":{"Count":3}}"#;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/constructed-parser-runtime-inputs")
        .join(name)
}

fn payload_schema() -> SchemaNode {
    SchemaNode::group(
        "Payload",
        vec![
            SchemaNode::group(
                "Header",
                vec![
                    SchemaNode::scalar("Count", ScalarType::Int)
                        .with_numeric_range(NumericRange::Integer(
                            IntegerRange::new(Some(0), Some(9)).unwrap(),
                        ))
                        .unwrap(),
                    SchemaNode::scalar("Unused", ScalarType::String)
                        .with_string_length_range(StringLengthRange::new(0, Some(3)).unwrap())
                        .unwrap(),
                ],
            )
            .with_required_fields(vec!["Count".into()])
            .unwrap(),
            SchemaNode::scalar("Tail", ScalarType::Bool),
        ],
    )
    .with_required_fields(vec!["Header".into()])
    .unwrap()
}

fn expected(nested: bool) -> Project {
    let mut records = SchemaNode::group(
        "Records",
        vec![
            SchemaNode::scalar("Text", ScalarType::String),
            SchemaNode::scalar("Count", ScalarType::Int),
        ],
    )
    .repeating()
    .with_item_count_range(ItemCountRange::new(1, Some(1)).unwrap())
    .unwrap();
    records.json_unique_items = true;
    let mut source =
        SchemaNode::group("Source", vec![SchemaNode::scalar("Count", ScalarType::Int)]);
    let mut target = SchemaNode::group("Report", vec![records.clone()]);
    let mut root = Scope {
        children: vec![Scope {
            target_field: "Records".into(),
            bindings: vec![
                Binding {
                    target_field: "Text".into(),
                    node: 0,
                },
                Binding {
                    target_field: "Count".into(),
                    node: 4,
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    };
    if nested {
        source = SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group("Rows", vec![SchemaNode::scalar("Count", ScalarType::Int)])
                    .repeating()
                    .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
                    .unwrap(),
            ],
        );
        target = SchemaNode::group(
            "Report",
            vec![
                SchemaNode::group("Batches", vec![records])
                    .repeating()
                    .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
                    .unwrap(),
            ],
        );
        root = Scope {
            children: vec![Scope {
                target_field: "Batches".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                children: root.children,
                ..Default::default()
            }],
            ..Default::default()
        };
    }
    Project {
        source,
        target,
        source_path: Some("source.json".into()),
        target_path: Some("report.json".into()),
        source_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        target_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::Const {
                        value: Value::String("created".into()),
                    },
                ),
                (
                    1,
                    Node::RuntimeParameter {
                        name: "payload".into(),
                        ty: ScalarType::String,
                        preview: Some(PREVIEW.into()),
                    },
                ),
                (
                    2,
                    Node::Const {
                        value: Value::String(serde_json::to_string(&payload_schema()).unwrap()),
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::String(r#"["Header","Count"]"#.into()),
                    },
                ),
                (
                    4,
                    Node::Call {
                        function: "json_parse_field".into(),
                        args: vec![1, 2, 3],
                    },
                ),
            ]),
        },
        root,
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

fn verify_case(project: &Project, case: &serde_json::Value) {
    let source = format_json::from_str(case["input"].as_str().unwrap(), &project.source);
    if case["reject"] == true {
        assert!(source.is_err(), "{case}");
        return;
    }
    let mut parameters = engine::RuntimeParameters::new();
    for (name, value) in case["parameters"].as_object().unwrap() {
        let value = match value {
            serde_json::Value::String(value) => Value::String(value.clone()),
            serde_json::Value::Bool(value) => Value::Bool(*value),
            serde_json::Value::Number(value) => Value::Int(value.as_i64().unwrap()),
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Object(marker)
                if marker.len() == 1
                    && marker.get("non_finite").and_then(serde_json::Value::as_str)
                        == Some("positive-infinity") =>
            {
                Value::Float(f64::INFINITY)
            }
            _ => panic!("unsupported authored parameter: {case}"),
        };
        parameters.insert(name, value).unwrap();
    }
    let context =
        engine::ExecutionContext::new(Path::new("parser-input.mfd")).with_parameters(&parameters);
    let result = engine::run_with_context(project, &source.unwrap(), &context);
    if let Some(expected) = case.get("missing_parameter") {
        let Err(engine::EngineError::MissingRuntimeParameter { node, name }) = result else {
            panic!("missing parameter result differs: {case}: {result:?}");
        };
        assert_eq!(node, expected["node"].as_u64().unwrap() as u32, "{case}");
        assert_eq!(name, expected["name"].as_str().unwrap(), "{case}");
    } else if let Some(expected) = case.get("parameter_type_error") {
        let Err(engine::EngineError::RuntimeParameterType {
            node,
            name,
            expected: ty,
            found,
        }) = result
        else {
            panic!("parameter type result differs: {case}: {result:?}");
        };
        assert_eq!(node, expected["node"].as_u64().unwrap() as u32, "{case}");
        assert_eq!(name, expected["name"].as_str().unwrap(), "{case}");
        assert_eq!(expected["expected"], "string", "{case}");
        assert_eq!(ty, ScalarType::String, "{case}");
        assert_eq!(found, expected["found"].as_str().unwrap(), "{case}");
    } else if let Some(expected) = case.get("execution_error") {
        let Err(engine::EngineError::Function(functions::FunctionError::InvalidArgument {
            function,
            message,
        })) = result
        else {
            panic!("JSON parser error differs: {case}: {result:?}");
        };
        assert_eq!(function, expected["function"].as_str().unwrap(), "{case}");
        assert_eq!(message, expected["message"].as_str().unwrap(), "{case}");
    } else {
        let output = format_json::to_string(&project.target, &result.unwrap()).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output).unwrap(),
            case["expected"],
            "{case}"
        );
    }
}

#[test]
fn required_runtime_parser_fields_keep_complete_projects_values_and_errors() {
    for design in [
        "json-scalar.mfd",
        "edi-scalar.mfd",
        "json-copy-all.mfd",
        "edi-copy-all.mfd",
        "nested-json-copy-all.mfd",
        "nested-edi-copy-all.mfd",
    ] {
        let path = fixture(design);
        let imported = mfd::import_with_options(
            &path,
            &mfd::ImportOptions::default()
                .with_package_root(path.parent().unwrap().parent().unwrap()),
        )
        .unwrap();
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        let nested = design.starts_with("nested-");
        let oracle = expected(nested);
        assert_eq!(
            serde_json::to_value(&imported.project).unwrap(),
            serde_json::to_value(&oracle).unwrap(),
            "{design}"
        );
        assert!(engine::validate(&imported.project).is_empty(), "{design}");
        let catalog = std::fs::read_to_string(fixture(if nested {
            "nested-cases.json"
        } else {
            "cases.json"
        }))
        .unwrap();
        for case in serde_json::from_str::<Vec<serde_json::Value>>(&catalog).unwrap() {
            verify_case(&imported.project, &case);
        }
    }
}

static NEXT: AtomicU64 = AtomicU64::new(0);

struct TemporaryFixture(PathBuf);

impl TemporaryFixture {
    fn new(text: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "ferrule_parser_input_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let design = root.join("design");
        std::fs::create_dir(&design).unwrap();
        let prior = root.join("constructed-variable-drivers");
        std::fs::create_dir(&prior).unwrap();
        for name in [
            "source.schema.json",
            "target.schema.json",
            "nested-source.schema.json",
            "nested-target.schema.json",
            "variable.schema.json",
        ] {
            std::fs::copy(
                fixture("../constructed-variable-drivers").join(name),
                prior.join(name),
            )
            .unwrap();
        }
        for name in ["payload.schema.json", "variable-array.schema.json"] {
            std::fs::copy(fixture(name), design.join(name)).unwrap();
        }
        std::fs::write(design.join("mapping.mfd"), text).unwrap();
        Self(root)
    }

    fn assert_refused(&self, name: &str) {
        let imported = mfd::import_with_options(
            &self.0.join("design/mapping.mfd"),
            &mfd::ImportOptions::default().with_package_root(&self.0),
        );
        match imported {
            Err(_) => {}
            Ok(imported) => assert!(
                !imported.warnings.is_empty(),
                "{name}: unsupported construction admitted"
            ),
        }
    }

    fn assert_record_refused(&self, name: &str, warning: &str, unknown_owner: bool) {
        let imported = mfd::import_with_options(
            &self.0.join("design/mapping.mfd"),
            &mfd::ImportOptions::default().with_package_root(&self.0),
        )
        .unwrap();
        let mut warnings = Vec::new();
        if unknown_owner {
            warnings.push(
                "skipped component `unknown-shadow`: unsupported library `unknown` (only xml/json/csv/fixed-length/flextext/edi/db/xlsx/protobuf/pdf-source, requestless HTTP GET XML, scalar user-defined functions, and core/lang function components and supported XPath 2 functions import)"
                    .to_string(),
            );
        }
        warnings.push(warning.to_string());
        assert_eq!(imported.warnings, warnings, "{name}");

        // The unsupported Copy-All contributes no Records scope, binding,
        // parser call, or runtime parameter. Final Project pruning removes
        // the unused eager literal because no published scope consumes it.
        let mut oracle = expected(false);
        oracle.root = Scope::default();
        oracle.graph = Graph::default();
        assert_eq!(
            serde_json::to_value(&imported.project).unwrap(),
            serde_json::to_value(&oracle).unwrap(),
            "{name}: a refused record retained project state"
        );
    }
}

impl Drop for TemporaryFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn parser_input_proof_refuses_ambiguous_and_unsupported_producers() {
    let original = std::fs::read_to_string(fixture("json-copy-all.mfd")).unwrap();
    for (name, text) in [
        ("missing parser input", original.replace("<vertex vertexkey=\"401\"><edges><edge vertexkey=\"410\"/></edges></vertex>", "")),
        ("invalid optional setting", original.replace("usageKind=\"input\" name=\"payload\"", "usageKind=\"input\" name=\"payload\" optional=\"2\"")),
        ("preview only", original.replace("<parameter usageKind=\"input\" name=\"payload\"/>", "")),
        ("non String parameter", original.replace("<input datatype=\"string\"", "<input datatype=\"boolean\"")),
        ("duplicate parameter declaration", original.replace("<parameter usageKind=\"input\" name=\"payload\"/>", "<parameter usageKind=\"input\" name=\"payload\"/><parameter usageKind=\"input\" name=\"other\"/>")),
        ("duplicate output position", original.replace("<datapoint pos=\"0\" key=\"401\"/>", "<datapoint pos=\"0\" key=\"401\"/><datapoint pos=\"0\" key=\"402\"/>")),
        ("keyless output", original.replace("pos=\"0\" key=\"401\"", "pos=\"0\"")),
        ("secondary output", original.replace("pos=\"0\" key=\"401\"", "pos=\"1\" key=\"401\"")),
        ("duplicate input position", original.replace("<datapoint pos=\"0\" key=\"400\"/>", "<datapoint pos=\"0\" key=\"400\"/><datapoint pos=\"0\" key=\"405\"/>")),
        ("connected default", original.replace("</vertices>", "<vertex vertexkey=\"101\"><edges><edge vertexkey=\"400\"/></edges></vertex></vertices>")),
        ("foreign input producer", original.replace("name=\"payload\" library=\"core\" kind=\"6\"", "name=\"payload\" library=\"lang\" kind=\"6\"")),
        ("duplicate parser output", original.replace("outkey=\"412\"", "outkey=\"411\"")),
        ("parser parent control", original.replace("<entry name=\"FileInstance\" inpkey=\"410\">", "<entry name=\"FileInstance\" inpkey=\"410\"><entry name=\"parent-context\"/>")),
        ("parser compute control", original.replace("<entry name=\"FileInstance\" inpkey=\"410\">", "<entry name=\"FileInstance\" inpkey=\"410\"><entry name=\"compute-when\"/>")),
        ("runtime parameter is not presence", original.replace("<vertex vertexkey=\"101\"><edges><edge vertexkey=\"22\"/></edges></vertex>", "<vertex vertexkey=\"412\"><edges><edge vertexkey=\"22\"/></edges></vertex>")),
        ("physical parameter shadow", original.replace("outkey=\"12\"", "outkey=\"401\"")),
    ] {
        assert_ne!(text, original, "{name}");
        TemporaryFixture::new(&text).assert_refused(name);
    }
}

fn assert_raw_shadow_census(text: &str, key: u32, shadow_owner: &str) {
    let document = roxmltree::Document::parse(text).unwrap();
    let components = document
        .descendants()
        .find(|node| node.has_tag_name("children"))
        .unwrap();
    let key = key.to_string();
    let mut owners = BTreeMap::new();
    for component in components
        .children()
        .filter(|node| node.has_tag_name("component"))
    {
        let declarations = component
            .children()
            .filter(|node| node.has_tag_name("data"))
            .flat_map(|data| data.children().filter(|node| node.has_tag_name("root")))
            .flat_map(|root| root.descendants())
            .filter(|node| {
                node.has_tag_name("entry") && node.attribute("outkey") == Some(key.as_str())
            })
            .count()
            + component
                .children()
                .filter(|node| node.has_tag_name("targets"))
                .flat_map(|targets| targets.children())
                .filter(|node| {
                    node.has_tag_name("datapoint") && node.attribute("key") == Some(key.as_str())
                })
                .count();
        if declarations != 0 {
            owners.insert(component.attribute("name").unwrap(), declarations);
        }
    }
    let producer = if key == "401" { "payload" } else { "parsed" };
    assert_eq!(owners, BTreeMap::from([(producer, 1), (shadow_owner, 1)]));

    // The source reader lowers only the first entry. A declaration after it
    // must remain visible to raw ownership proof even without a selected port.
    let source = components
        .children()
        .find(|node| node.attribute("name") == Some("source"))
        .unwrap();
    let first_entry = source
        .children()
        .find(|node| node.has_tag_name("data"))
        .unwrap()
        .children()
        .find(|node| node.has_tag_name("root"))
        .unwrap()
        .children()
        .find(|node| node.has_tag_name("entry"))
        .unwrap();
    assert!(first_entry.descendants().all(|node| {
        !node.has_tag_name("entry") || node.attribute("outkey") != Some(key.as_str())
    }));
    if shadow_owner == "unknown-shadow" {
        let unknown = components
            .children()
            .find(|node| node.attribute("name") == Some(shadow_owner))
            .unwrap();
        assert_eq!(unknown.attribute("library"), Some("unknown"));
        assert_eq!(unknown.attribute("kind"), Some("31"));
    }
}

#[test]
fn parser_input_proof_refuses_raw_shadows_without_normalized_ports() {
    let original = std::fs::read_to_string(fixture("json-copy-all.mfd")).unwrap();
    let physical_root_end =
        "</root><json schema=\"../constructed-variable-drivers/source.schema.json\"";
    assert_eq!(original.matches(physical_root_end).count(), 1);
    for (name, key) in [("runtime parameter", 401), ("parser scalar", 411)] {
        // The physical reader walks the first root entry. This second entry
        // retains an output declaration without producing a normalized port.
        let physical = original.replace(
            physical_root_end,
            &format!("<entry name=\"unlowered\" outkey=\"{key}\"/>{physical_root_end}"),
        );
        assert_ne!(physical, original, "{name}");
        assert_raw_shadow_census(&physical, key, "source");
        let warning = if key == 411 {
            "variable `buffer` cannot construct repeating target `Records`: a selected array root cannot have connected descendant overrides; iteration skipped"
        } else {
            "iteration into `Records` comes from an unsupported feed; skipped"
        };
        TemporaryFixture::new(&physical).assert_record_refused(
            &format!("unlowered physical {name} shadow"),
            warning,
            false,
        );

        // The unknown owner is skipped during import. Its raw declaration must
        // still prevent an otherwise valid parser scalar proof.
        let unknown = original.replace("</children>", &format!(
            "<component name=\"unknown-shadow\" library=\"unknown\" kind=\"31\"><data><root><entry name=\"unlowered\" outkey=\"{key}\"/></root></data></component></children>"
        ));
        assert_ne!(unknown, original, "{name}");
        assert_raw_shadow_census(&unknown, key, "unknown-shadow");
        TemporaryFixture::new(&unknown).assert_record_refused(
            &format!("unknown {name} shadow"),
            "iteration into `Records` comes from an unsupported feed; skipped",
            true,
        );
    }
}

#[test]
fn parser_fields_preserve_parent_scope_and_nonrepeating_path_refusals() {
    let original = std::fs::read_to_string(fixture("nested-json-copy-all.mfd")).unwrap();
    let control = r#"<component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="120"/><datapoint pos="1" key="121"/></sources><targets><datapoint pos="0" key="110"/></targets></component><component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="105"/></targets><data><constant value="true" datatype="boolean"/></data></component>"#;
    let filtered = original.replace("</children>", &format!("{control}</children>"))
        .replace("<vertex vertexkey=\"10\"><edges><edge vertexkey=\"40\"/></edges></vertex>",
            "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"120\"/></edges></vertex><vertex vertexkey=\"105\"><edges><edge vertexkey=\"121\"/></edges></vertex><vertex vertexkey=\"110\"><edges><edge vertexkey=\"40\"/></edges></vertex>");
    assert_ne!(filtered, original);
    TemporaryFixture::new(&filtered).assert_refused("filtered actual parent scope");

    let original = std::fs::read_to_string(fixture("json-copy-all.mfd")).unwrap();
    let repeated = TemporaryFixture::new(&original);
    let schema_path = repeated.0.join("design/payload.schema.json");
    let mut schema: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&schema_path).unwrap()).unwrap();
    let header = schema["properties"]["Header"].take();
    schema["properties"]["Header"] = serde_json::json!({"type":"array", "items":header});
    std::fs::write(&schema_path, serde_json::to_vec(&schema).unwrap()).unwrap();
    repeated.assert_refused("parser output crosses repetition");
}
