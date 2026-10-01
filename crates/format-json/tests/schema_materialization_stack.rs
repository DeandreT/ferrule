use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use format_json::{JsonFormatError, json_schema};
use ir::{ScalarType, SchemaKind};
use serde_json::{Map, Value, json};

const WORKER_ENV: &str = "FERRULE_SCHEMA_MATERIALIZATION_STACK_WORKER";

fn reference_chain(count: usize, objects: bool) -> String {
    let mut definitions = Map::new();
    for index in 0..count {
        let schema = if index + 1 == count {
            json!({"type":"string","minLength":2})
        } else {
            let reference = json!({"$ref":format!("#/$defs/N{}", index + 1)});
            if objects {
                json!({"type":"object","properties":{"next":reference}})
            } else {
                json!({"allOf":[reference]})
            }
        };
        definitions.insert(format!("N{index}"), schema);
    }
    json!({"title":"Root","$ref":"#/$defs/N0","$defs":definitions}).to_string()
}

fn syntactic_chain(count: usize, objects: bool) -> String {
    let mut schema = json!({"type":"string","minLength":2});
    for _ in 0..count {
        schema = if objects {
            json!({"type":"object","properties":{"next":schema}})
        } else {
            json!({"allOf":[schema]})
        };
    }
    schema["title"] = json!("Root");
    schema.to_string()
}

fn mixed_reference_chain(count: usize, groups_per_definition: usize) -> String {
    let mut definitions = Map::new();
    for index in 0..count {
        let mut schema = if index + 1 == count {
            json!({"type":"string","minLength":2})
        } else {
            json!({"$ref":format!("#/$defs/N{}", index + 1)})
        };
        for _ in 0..groups_per_definition {
            schema = json!({"type":"object","properties":{"next":schema}});
        }
        definitions.insert(format!("N{index}"), schema);
    }
    json!({"title":"Root","$ref":"#/$defs/N0","$defs":definitions}).to_string()
}

fn manual_predicate_chain(count: usize, role: &str) -> String {
    let mut definitions = Map::new();
    for index in 0..count {
        let mut schema = if index + 1 == count {
            match role {
                "propertyNames" => json!({"type":"string","minLength":2}),
                "required" => json!({"required":["value"]}),
                "never" => Value::Bool(false),
                _ => panic!("unexpected test predicate role"),
            }
        } else {
            json!({"$ref":format!("#/$defs/N{}", index + 1)})
        };
        for _ in 0..8 {
            schema = json!({"allOf":[schema]});
        }
        definitions.insert(format!("N{index}"), schema);
    }
    let mut schema = json!({
        "title":"Root", "type":"object",
        "properties":{"trigger":{"type":"string"},"value":{"type":"string"}},
        "$defs":definitions
    });
    let reference = json!({"$ref":"#/$defs/N0"});
    if role == "propertyNames" {
        schema["propertyNames"] = reference;
    } else {
        schema["dependentSchemas"] = json!({"trigger":reference});
    }
    schema.to_string()
}

fn branching_composition(count: usize) -> String {
    let mut definitions = Map::new();
    for index in 0..count {
        let schema = if index + 1 == count {
            json!({"type":"string","minLength":2})
        } else {
            let reference = json!({"$ref":format!("#/$defs/N{}", index + 1)});
            json!({"allOf":[reference.clone(),reference]})
        };
        definitions.insert(format!("N{index}"), schema);
    }
    json!({"title":"Root","$ref":"#/$defs/N0","$defs":definitions}).to_string()
}

fn branching_property_names(count: usize) -> String {
    let mut composition: Value = serde_json::from_str(&branching_composition(count)).unwrap();
    json!({
        "type":"object", "propertyNames":{"$ref":"#/$defs/N0"},
        "$defs":composition["$defs"].take()
    })
    .to_string()
}

fn linear_contains_chain(count: usize) -> String {
    let mut definitions = Map::new();
    for index in 0..count {
        let schema = if index + 1 == count {
            json!({"type":"string","const":"ok"})
        } else {
            json!({
                "type":"array", "items":{"type":"string"},
                "allOf":[{"contains":{"$ref":format!("#/$defs/N{}", index + 1)}}]
            })
        };
        definitions.insert(format!("N{index}"), schema);
    }
    json!({"title":"Root","$ref":"#/$defs/N0","$defs":definitions}).to_string()
}

fn check_import(text: String, group_depth: usize) {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let schema = json_schema::import_str(&text).unwrap();
            assert_eq!(schema.name, "Root");
            let mut leaf = &schema;
            for _ in 0..group_depth {
                leaf = leaf.child("next").unwrap();
            }
            assert_eq!(
                leaf.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::String
                }
            );
            assert_eq!(leaf.string_length_range.unwrap().minimum(), 2);
        })
        .unwrap()
        .join()
        .unwrap();
}

fn check_depth_rejection(text: String) {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            assert!(matches!(
                json_schema::import_str(&text),
                Err(JsonFormatError::SchemaResourceLimit {
                    kind: "schema materialization depth",
                    limit: 64
                })
            ));
            let recovered = json_schema::import_str(r#"{"type":"string","minLength":2}"#).unwrap();
            assert_eq!(recovered.string_length_range.unwrap().minimum(), 2);
        })
        .unwrap()
        .join()
        .unwrap();
}

fn run_isolated(name: &str) -> bool {
    if std::env::var_os(WORKER_ENV).is_some() {
        return false;
    }
    // An isolated child turns a future stack overflow into a test failure
    // instead of aborting every other test in the package.
    let executable = std::env::current_exe().unwrap();
    #[cfg(unix)]
    let mut child = {
        let mut child = Command::new("sh");
        child
            .args(["-c", "ulimit -c 0; exec \"$@\"", "schema-stack-worker"])
            .arg(&executable);
        child
    };
    #[cfg(not(unix))]
    let mut child = Command::new(executable);
    let mut child = child
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(WORKER_ENV, "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let timed_out = loop {
        if child.try_wait().unwrap().is_some() {
            break false;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            break true;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let output = child.wait_with_output().unwrap();
    assert!(
        !timed_out && output.status.success(),
        "child failed with {} (30s deadline exceeded: {timed_out})\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}

#[test]
fn materialization_preserves_bounded_nested_schemas_on_a_two_mib_stack() {
    if run_isolated("materialization_preserves_bounded_nested_schemas_on_a_two_mib_stack") {
        return;
    }
    check_import(reference_chain(64, true), 63);
    check_import(reference_chain(64, false), 0);
    check_import(syntactic_chain(48, true), 48);
    check_import(syntactic_chain(48, false), 0);
    check_import(mixed_reference_chain(8, 8), 64);
    check_import(mixed_reference_chain(16, 4), 64);
    check_depth_rejection(mixed_reference_chain(16, 8));
}

#[test]
fn manual_predicate_routes_share_expanded_depth_limits() {
    if run_isolated("manual_predicate_routes_share_expanded_depth_limits") {
        return;
    }
    for role in ["propertyNames", "required", "never"] {
        let control = manual_predicate_chain(4, role);
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                let schema = json_schema::import_str(&control).unwrap();
                match role {
                    "propertyNames" => {
                        assert_eq!(
                            schema
                                .json_property_names
                                .as_ref()
                                .unwrap()
                                .length()
                                .unwrap()
                                .minimum(),
                            2
                        );
                        assert!(format_json::from_str(r#"{"trigger":"yes"}"#, &schema).is_ok());
                        assert!(matches!(
                            format_json::from_str(r#"{"x":"yes"}"#, &schema),
                            Err(JsonFormatError::InvalidPropertyName { .. })
                        ));
                    }
                    "required" => {
                        assert!(
                            format_json::from_str(r#"{"trigger":"yes","value":"ok"}"#, &schema)
                                .is_ok()
                        );
                        assert!(matches!(
                            format_json::from_str(r#"{"trigger":"yes"}"#, &schema),
                            Err(JsonFormatError::MissingDependentProperty { .. })
                        ));
                    }
                    "never" => {
                        assert!(
                            schema.json_dependent_schemas.as_ref().unwrap().as_slice()[0]
                                .predicate()
                                .is_never()
                        );
                        assert!(format_json::from_str("{}", &schema).is_ok());
                        assert!(matches!(
                            format_json::from_str(r#"{"trigger":"yes"}"#, &schema),
                            Err(JsonFormatError::DependentSchemaMismatch { .. })
                        ));
                    }
                    _ => panic!("unexpected test predicate role"),
                }
            })
            .unwrap()
            .join()
            .unwrap();
        check_depth_rejection(manual_predicate_chain(16, role));
    }
}

#[test]
fn branching_materialization_has_one_cumulative_parse_budget() {
    if run_isolated("branching_materialization_has_one_cumulative_parse_budget") {
        return;
    }
    check_import(branching_composition(6), 0);
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let contains = json_schema::import_str(
                r##"{
                "type":"array", "items":{"type":"string"},
                "allOf":[{"contains":{"$ref":"#/$defs/Expected"}}],
                "$defs":{"Expected":{"type":"string","const":"ok"}}
            }"##,
            )
            .unwrap();
            assert!(format_json::from_str(r#"["ok"]"#, &contains).is_ok());
            assert!(matches!(
                format_json::from_str(r#"["bad"]"#, &contains),
                Err(JsonFormatError::ContainsCountMismatch { .. })
            ));
            assert!(matches!(
                json_schema::import_str(&branching_property_names(32)),
                Err(JsonFormatError::SchemaResourceLimit {
                    kind: "schema parse steps",
                    limit: 100_000
                })
            ));
            let recovered = json_schema::import_str(r#"{"type":"string","minLength":2}"#).unwrap();
            assert_eq!(recovered.string_length_range.unwrap().minimum(), 2);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn linear_all_of_contains_references_retain_each_predicate() {
    if run_isolated("linear_all_of_contains_references_retain_each_predicate") {
        return;
    }
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let schema = json_schema::import_str(&linear_contains_chain(32)).unwrap();
            let mut current = &schema;
            for _ in 0..31 {
                assert!(current.repeating);
                let constraints = current.json_contains.as_ref().unwrap().as_slice();
                assert_eq!(constraints.len(), 1);
                assert_eq!(constraints[0].range().minimum(), 1);
                current = constraints[0].predicate().as_schema().unwrap();
            }
            assert!(!current.repeating);
            assert_eq!(current.fixed.as_deref(), Some("ok"));
            assert_eq!(
                current.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::String
                }
            );
        })
        .unwrap()
        .join()
        .unwrap();
}
