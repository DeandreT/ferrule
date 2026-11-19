use super::{TempDir, fixtures, generated_hosts, imported, refused, replace, run};
use serde_json::Value;

fn fixture() -> TempDir {
    let d = TempDir::new();
    for n in [
        "mapping.mfd",
        "lookup.schema.json",
        "target.schema.json",
        "primary.schema.json",
    ] {
        std::fs::copy(fixtures().join("explicit-item").join(n), d.0.join(n)).unwrap();
    }
    d
}

#[test]
fn explicit_item_complete_results_and_typed_errors() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../fixtures/json-array-lookup/explicit-item/cases.json"
    ))
    .unwrap();
    for root in [true, false] {
        let d = fixture();
        let path = d.0.join("mapping.mfd");
        if !root {
            replace(
                &path,
                r#"<vertex vertexkey="124"><edges><edge vertexkey="134"/></edges></vertex>"#,
                "",
            );
        }
        let project = imported(&path);
        let mut expected_schema = project.target.clone();
        if !root {
            // The disconnected target has no Label binding. Reading expected JSON
            // with the full schema would synthesize Label: Null for that omission.
            let ir::SchemaKind::Group { children, .. } = &mut expected_schema.kind else {
                panic!("expected a group target");
            };
            let label = children
                .iter()
                .position(|child| child.name == "Label")
                .unwrap();
            children.remove(label);
        }
        for case in &cases {
            let actual = run(&project, case);
            if let Some(expected) = case["cardinality_error"].as_str() {
                assert!(
                    matches!(actual,Err(engine::EngineError::MappingException{message:Some(ref message),..}) if message==expected),
                    "{case}: {actual:?}"
                );
            } else if let Some(expected) = case["error"].as_str() {
                assert!(
                    matches!(&actual,Err(engine::EngineError::Function(functions::FunctionError::InvalidArgument{function:"json_parse_array_length",message})) if *message==expected),
                    "{case}: {actual:?}"
                );
            } else {
                let mut expected = case["expected"].clone();
                if !root {
                    expected.as_object_mut().unwrap().remove("Label");
                }
                let out = actual.unwrap();
                assert_eq!(
                    out,
                    format_json::from_str(&expected.to_string(), &expected_schema).unwrap(),
                    "root connected={root}: {case}"
                );
            }
        }
    }
}

#[test]
fn explicit_item_refuses_missing_driver_foreign_branch_and_shared_predicate() {
    for (from, to) in [
        (
            r#"<vertex vertexkey="151"><edges><edge vertexkey="152"/></edges></vertex>"#,
            "",
        ),
        (
            r#"<vertex vertexkey="4"><edges><edge vertexkey="101"/>"#,
            r#"<vertex vertexkey="6"><edges><edge vertexkey="101"/>"#,
        ),
        (
            r#"<edge vertexkey="102"/>"#,
            r#"<edge vertexkey="102"/><edge vertexkey="134"/>"#,
        ),
    ] {
        let d = fixture();
        let p = d.0.join("mapping.mfd");
        if to.contains(r#"vertexkey="134""#) {
            replace(
                &p,
                r#"<vertex vertexkey="124"><edges><edge vertexkey="134"/></edges></vertex>"#,
                "",
            );
        }
        replace(&p, from, to);
        refused(&p);
    }
}

#[test]
#[ignore = "compiles generated Rust and C# Release consumers"]
fn explicit_item_release_consumers() {
    generated_hosts("explicit-item");
}
