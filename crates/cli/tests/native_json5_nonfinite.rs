use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ir::{ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope};

fn project(explicit: bool) -> Project {
    let scalar = || {
        SchemaNode::scalar("Value", ScalarType::Float)
            .nullable()
            .unwrap()
    };
    Project {
        source: SchemaNode::group("Input", vec![scalar()]),
        target: SchemaNode::group("Result", vec![scalar()]),
        source_path: None,
        target_path: Some("output.json".into()),
        source_options: FormatOptions {
            json5: explicit,
            ..FormatOptions::default()
        },
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    frame: None,
                    path: vec!["Value".into()],
                },
            )]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn native_json5_payload_routes_reject_nonfinite_before_nullable_mapping() -> anyhow::Result<()> {
    let mut originals = Vec::new();
    for explicit in [false, true] {
        let project = project(explicit);
        let path = Path::new(if explicit {
            "input.json"
        } else {
            "input.json5"
        });
        for token in [
            "Infinity",
            "+Infinity",
            "-Infinity",
            "NaN",
            "+NaN",
            "-NaN",
            "1e999",
            "-1e999",
        ] {
            for source in [
                format!("{{Value:{token}}}"),
                format!("{{Value:{token},Value:1.5}}"),
                format!("{{Value:1.5,extra:{token}}}"),
            ] {
                let document = cli::PayloadDocument::new(path, source.as_bytes())?;
                let actual = cli::run_project_value_payloads(
                    &project,
                    Path::new("/virtual/project.json"),
                    &cli::PayloadRunOptions::new(document),
                );
                println!("EXPLICIT {explicit} INPUT {source:?} ORIGINAL {actual:#?}");
                if let Err(error) = &actual {
                    for (index, cause) in error.chain().enumerate() {
                        println!("ORIGINAL CAUSE {index} {cause:#?}");
                    }
                }
                originals.push((explicit, source, actual));
            }
        }
    }
    assert_eq!(originals.len(), 48);
    for (explicit, source, actual) in originals {
        let error = actual.expect_err(&format!("explicit={explicit} accepted {source}"));
        assert!(
            error.chain().any(|cause| cause
                .downcast_ref::<format_json::JsonFormatError>()
                .is_some_and(|original| matches!(
                    original,
                    format_json::JsonFormatError::Json5(_)
                ))),
            "lost native parser cause for {source}: {error:#}"
        );
    }
    Ok(())
}

#[test]
fn native_json5_payload_routes_preserve_null_absence_and_finite_output() -> anyhow::Result<()> {
    let mut originals = Vec::new();
    for explicit in [false, true] {
        let project = project(explicit);
        let path = Path::new(if explicit {
            "input.json"
        } else {
            "input.json5"
        });
        for (source, expected) in [
            ("{Value:null}", "{\n  \"Value\": null\n}\n"),
            ("{}", "{}\n"),
            ("{Value:1.5,}", "{\n  \"Value\": 1.5\n}\n"),
            (
                "{/* Infinity NaN */ Value:0x10}",
                "{\n  \"Value\": 16.0\n}\n",
            ),
            ("{Value:1.5,Value:2.5}", "{\n  \"Value\": 2.5\n}\n"),
        ] {
            let document = cli::PayloadDocument::new(path, source.as_bytes())?;
            let actual = cli::run_project_value_payloads(
                &project,
                Path::new("/virtual/project.json"),
                &cli::PayloadRunOptions::new(document),
            );
            println!(
                "EXPLICIT {explicit} INPUT {source:?} EXPECTED {expected:?} ORIGINAL {actual:#?}"
            );
            originals.push((explicit, source, expected, actual));
        }
    }
    assert_eq!(originals.len(), 10);
    for (explicit, source, expected, actual) in originals {
        let expected = cli::PayloadRunOutcome {
            records_written: 1,
            artifacts: vec![cli::PayloadArtifact {
                target: "Result".into(),
                records_written: 1,
                path: PathBuf::from("/virtual/output.json"),
                bytes: expected.as_bytes().to_vec(),
            }],
        };
        assert_eq!(actual?, expected, "explicit={explicit} {source}");
    }
    Ok(())
}
