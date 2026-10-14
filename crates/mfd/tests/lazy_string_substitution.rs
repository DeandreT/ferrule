use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::Value;
use mapping::{Node, Project};

#[path = "fixtures/lazy-string-substitution/project_oracle.rs"]
mod project_oracle;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/lazy-string-substitution")
        .join(name)
}

fn verify(project: &Project, case: &serde_json::Value) {
    let input = format_json::from_str(case["input"].as_str().unwrap(), &project.source);
    if case["reject"] == true {
        assert!(input.is_err(), "{case}");
        return;
    }
    let mut parameters = engine::RuntimeParameters::new();
    for (name, value) in case["parameters"].as_object().unwrap() {
        parameters
            .insert(
                name,
                match value {
                    serde_json::Value::String(value) => Value::String(value.clone()),
                    serde_json::Value::Null => Value::Null,
                    _ => panic!("unsupported authored parameter: {case}"),
                },
            )
            .unwrap();
    }
    let input = input.unwrap();
    let saved_input = input.clone();
    let saved_parameters = format!("{parameters:?}");
    let context = engine::ExecutionContext::new(Path::new("lazy-substitution.mfd"))
        .with_parameters(&parameters);
    let result = engine::run_with_context(project, &input, &context);
    assert_eq!(input, saved_input, "caller input changed: {case}");
    assert_eq!(
        format!("{parameters:?}"),
        saved_parameters,
        "caller parameters changed: {case}"
    );
    if let Some(error) = case.get("missing_parameter") {
        assert!(
            matches!(result, Err(engine::EngineError::MissingRuntimeParameter { node, ref name })
            if node == error["node"].as_u64().unwrap() as u32 && name == error["name"].as_str().unwrap()),
            "{case}"
        );
    } else if let Some(error) = case.get("execution_error") {
        assert!(
            matches!(result, Err(engine::EngineError::Function(functions::FunctionError::InvalidArgument { function, message }))
            if function == error["function"].as_str().unwrap() && message == error["message"].as_str().unwrap()),
            "{case}"
        );
    } else {
        assert_eq!(
            format_json::to_value(&project.target, &result.unwrap()).unwrap(),
            case["expected"],
            "{case}"
        );
    }
}

#[test]
fn connected_json_strings_have_complete_lazy_projects_and_literal_outcomes() {
    for design in [
        "substitute-null",
        "substitute-missing",
        "physical",
        "constant",
    ] {
        let imported = mfd::import(&fixture(&format!("{design}.mfd"))).unwrap();
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        assert_eq!(
            serde_json::to_value(&imported.project).unwrap(),
            serde_json::to_value(project_oracle::expected(design)).unwrap(),
            "{design}"
        );
        assert!(engine::validate(&imported.project).is_empty(), "{design}");
        let cases: Vec<serde_json::Value> = serde_json::from_slice(
            &std::fs::read(fixture(&format!("{design}-cases.json"))).unwrap(),
        )
        .unwrap();
        for case in cases {
            verify(&imported.project, &case);
        }
    }
}

#[test]
fn explicit_ir_substitution_calls_keep_eager_fallback_demand() {
    let mut project = project_oracle::expected("substitute-null");
    project.graph.nodes.insert(
        0,
        Node::Call {
            function: "substitute_missing".into(),
            args: vec![1, 5],
        },
    );
    verify(
        &project,
        &serde_json::json!({
            "input":"{\"Rows\":[{\"Value\":\"LEFT\"}]}", "parameters":{},
            "missing_parameter":{"node":2,"name":"payload"}
        }),
    );
    assert_eq!(
        functions::call("exists", &[Value::Null]).unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        functions::call("exists", &[Value::String(String::new())]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        functions::call("exists", &[Value::xml_nil()]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        functions::call(
            "substitute_missing",
            &[Value::xml_nil(), Value::String("R".into())]
        )
        .unwrap(),
        Value::String("R".into())
    );
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temporary(PathBuf);
impl Temporary {
    fn new(text: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "ferrule_lazy_string_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        for name in [
            "source.schema.json",
            "physical-source.schema.json",
            "payload.schema.json",
            "target.schema.json",
        ] {
            std::fs::copy(fixture(name), root.join(name)).unwrap();
        }
        std::fs::write(root.join("design.mfd"), text).unwrap();
        Self(root)
    }
    fn import(&self) -> mfd::Imported {
        mfd::import(&self.0.join("design.mfd")).unwrap()
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn unconnected_ambiguous_and_nonstring_operands_keep_the_ordinary_path() {
    let original = std::fs::read_to_string(fixture("substitute-null.mfd")).unwrap();
    for text in [
        original.replace("<datapoint pos=\"0\" key=\"500\"/>", "<datapoint pos=\"0\"/>"),
        original.replace("<edge vertexkey=\"500\"/>", ""),
        original.replace("<edge vertexkey=\"501\"/>", ""),
        original.replace("pos=\"1\" key=\"501\"", "pos=\"bad\" key=\"501\""),
        original.replace("<datapoint pos=\"1\" key=\"501\"/>", "<datapoint pos=\"1\" key=\"501\"/><datapoint pos=\"1\" key=\"503\"/>"),
        original.replace("</targets></component>\n<component name=\"report\"", "</targets><properties/></component>\n<component name=\"report\""),
        original.replace("</vertices>", "<vertex vertexkey=\"424\"><edges><edge vertexkey=\"500\"/></edges></vertex></vertices>"),
        original.replace("</vertices>", "<vertex vertexkey=\"424\"><edges><edge vertexkey=\"501\"/></edges></vertex></vertices>"),
        original.replace("library=\"db\" kind=\"5\"", "library=\"foreign\" kind=\"5\""),
        original.replace("key=\"500\"/>", "key=\"500\" default=\"unknown\"/>"),
    ] {
        assert_ne!(text, original);
        let imported = Temporary::new(&text).import();
        assert!(!imported.project.graph.nodes.values().any(|node| matches!(node, Node::If { .. })), "{text}");
    }
    // Unsupported raw owners still participate in output-key ownership.
    for key in [101, 401, 421, 502] {
        for declaration in [
            format!("<data><root><entry name=\"ignored\" outkey=\"{key}\"/></root></data>"),
            format!("<targets><datapoint pos=\"0\" key=\"{key}\"/></targets>"),
        ] {
            let owner = format!(
                "<component name=\"unknown\" library=\"unknown\" kind=\"99\">{declaration}</component>"
            );
            for text in [
                original.replace("</children>", &format!("{owner}</children>")),
                original.replace("</mapping>", &format!("<component name=\"other-root\"><structure><children>{owner}</children></structure></component></mapping>")),
            ] {
                let imported = Temporary::new(&text).import();
                assert!(!imported.project.graph.nodes.values().any(|node| matches!(node, Node::If { .. })), "key={key}: {text}");
            }
        }
    }
    for design in ["substitute-null", "physical"] {
        let temporary =
            Temporary::new(&std::fs::read_to_string(fixture(&format!("{design}.mfd"))).unwrap());
        let filename = if design == "physical" {
            "physical-source.schema.json"
        } else {
            "source.schema.json"
        };
        let mut schema: serde_json::Value =
            serde_json::from_slice(&std::fs::read(temporary.0.join(filename)).unwrap()).unwrap();
        let field = if design == "physical" {
            "Replacement"
        } else {
            "Value"
        };
        schema["properties"]["Rows"]["items"]["properties"][field] =
            serde_json::json!({"type":"integer"});
        std::fs::write(
            temporary.0.join(filename),
            serde_json::to_vec(&schema).unwrap(),
        )
        .unwrap();
        let imported = temporary.import();
        assert!(
            !imported
                .project
                .graph
                .nodes
                .values()
                .any(|node| matches!(node, Node::If { .. }))
        );
    }
}

#[test]
fn raw_string_constant_metadata_is_not_normalized_into_a_lazy_proof() {
    let original = std::fs::read_to_string(fixture("constant.mfd")).unwrap();
    for text in [
        original.replace("datatype=\"string\" value=\"FALLBACK\"", "datatype=\"string\" value=\"FALLBACK\" default=\"unknown\""),
        original.replace("</data></component>", "</data><properties/></component>"),
        original.replace("<constant datatype=\"string\" value=\"FALLBACK\"/>", "<constant datatype=\"string\" value=\"FALLBACK\"/><constant datatype=\"string\" value=\"OTHER\"/>"),
        original.replace("<data><constant datatype=\"string\" value=\"FALLBACK\"/></data>", "<data><constant datatype=\"string\" value=\"FALLBACK\"/></data><data><constant datatype=\"string\" value=\"OTHER\"/></data>"),
        original.replace("key=\"102\"/>", "key=\"102\" default=\"unknown\"/>"),
        original.replace("</children>", "<component name=\"unknown\" library=\"unknown\" kind=\"99\"><targets><datapoint pos=\"0\" key=\"102\"/></targets></component></children>"),
    ] {
        assert_ne!(text, original);
        let imported = Temporary::new(&text).import();
        assert!(!imported.project.graph.nodes.values().any(|node| matches!(node, Node::If { .. })), "{text}");
    }
}

#[test]
fn exported_conditionals_keep_laziness_after_project_codec_and_reimport() {
    let imported = mfd::import(&fixture("substitute-null.mfd")).unwrap();
    let project: Project =
        serde_json::from_slice(&serde_json::to_vec(&imported.project).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(&project).unwrap(),
        serde_json::to_value(project_oracle::expected("substitute-null")).unwrap()
    );
    let temporary =
        Temporary::new(&std::fs::read_to_string(fixture("substitute-null.mfd")).unwrap());
    let output = temporary.0.join("exported.mfd");
    mfd::export(&project, &output).unwrap();
    let text = std::fs::read_to_string(&output).unwrap();
    assert!(text.contains("name=\"if-else\""));
    assert!(
        !text.contains("name=\"substitute-null\"") && !text.contains("name=\"substitute-missing\"")
    );
    let roundtrip = mfd::import(&output).unwrap();
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixture("substitute-null-cases.json")).unwrap())
            .unwrap();
    let parameter_nodes: Vec<_> = roundtrip
        .project
        .graph
        .nodes
        .iter()
        .filter_map(|(id, node)| {
            matches!(node, Node::RuntimeParameter { name, .. } if name == "payload").then_some(*id)
        })
        .collect();
    let [parameter_node] = parameter_nodes.as_slice() else {
        panic!("one exported payload parameter is required");
    };
    for mut case in cases {
        // Graph allocation belongs to the importer; semantic errors retain
        // their parameter identity after canonical export reorders nodes.
        if let Some(error) = case.get_mut("missing_parameter") {
            error["node"] = serde_json::json!(parameter_node);
        }
        verify(&roundtrip.project, &case);
    }
}
