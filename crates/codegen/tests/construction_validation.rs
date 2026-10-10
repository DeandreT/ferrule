use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use engine::{ValidationEndpoint, ValidationOwner, ValidationScopeLocation, ValidationScopeStep};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Graph, NamedTarget, Node, Project, Scope, ScopeConstruction};

fn project() -> Project {
    Project {
        source: SchemaNode::group("Source", Vec::new()),
        target: SchemaNode::group("Primary", Vec::new()),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope::default(),
    }
}

fn named(name: &str, scalar: bool) -> NamedTarget {
    NamedTarget {
        name: name.into(),
        path: None,
        schema: if scalar {
            SchemaNode::scalar(name, ScalarType::String)
        } else {
            SchemaNode::group(name, Vec::new())
        },
        options: Default::default(),
        root: Scope {
            target_field: name.into(),
            ..Scope::default()
        },
    }
}

fn evidence(label: &str, project: &Project) -> Result<PathBuf, Box<dyn Error>> {
    let path = std::env::temp_dir().join(format!(
        "ferrule_construction_{label}_{}_{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir(&path)?;
    fs::write(
        path.join("project.json"),
        serde_json::to_vec_pretty(project)?,
    )?;
    eprintln!("complete construction evidence: {}", path.display());
    Ok(path)
}

fn retain(path: &Path, label: &str, value: impl std::fmt::Debug) -> Result<(), std::io::Error> {
    fs::write(
        path.join(format!("{label}.debug.txt")),
        format!("{value:#?}"),
    )
}

#[test]
fn constructed_scalar_roots_have_exact_primary_and_named_owners() -> Result<(), Box<dyn Error>> {
    let mut primary = project();
    primary.target = SchemaNode::scalar("Primary", ScalarType::String);
    let mut extra = project();
    extra.extra_targets = vec![named("First", false), named("Text", true)];
    for (label, project, owner) in [
        ("primary", primary, ValidationEndpoint::Target),
        (
            "named",
            extra,
            ValidationEndpoint::NamedTarget {
                index: 1,
                name: "Text".into(),
            },
        ),
    ] {
        let path = evidence(label, &project)?;
        retain(&path, "expected-owner", &owner)?;
        let issues = engine::validate(&project);
        retain(&path, "native-validation", &issues)?;
        let lowered = codegen::lower(&project);
        retain(&path, "lowering", &lowered)?;
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].location, "root scope");
        assert_eq!(
            issues[0].message,
            "constructed scope requires a group target schema"
        );
        assert_eq!(
            issues[0].owner,
            Some(ValidationOwner::Scope(ValidationScopeLocation {
                target: owner,
                path: Vec::new(),
            }))
        );
        let diagnostics = lowered.expect_err("incompatible construction refuses lowering");
        assert!(
            matches!(diagnostics.diagnostics(), [codegen::Diagnostic::Validation { location, message }]
            if location == "root scope" && message == "constructed scope requires a group target schema")
        );
        assert_eq!(
            project.extra_targets.len(),
            if label == "named" { 2 } else { 0 }
        );
    }
    Ok(())
}

#[test]
fn constructed_children_keep_shape_and_missing_path_diagnostics_distinct()
-> Result<(), Box<dyn Error>> {
    for scalar in [true, false] {
        let mut project = project();
        project.target = SchemaNode::group(
            "Primary",
            vec![SchemaNode::scalar("Text", ScalarType::String)],
        );
        let name = if scalar { "Text" } else { "Missing" };
        project.root.children.push(Scope {
            target_field: name.into(),
            ..Scope::default()
        });
        let path = evidence(
            if scalar {
                "scalar-child"
            } else {
                "missing-child"
            },
            &project,
        )?;
        let issues = engine::validate(&project);
        retain(&path, "complete-native-validation", &issues)?;
        let expected = if scalar {
            vec![
                "target scope is not a group",
                "constructed scope requires a group target schema",
            ]
        } else {
            vec!["target scope does not exist"]
        };
        retain(&path, "expected-before-assert", &expected)?;
        assert_eq!(
            issues
                .iter()
                .map(|issue| issue.message.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        for issue in issues {
            assert_eq!(issue.location, format!("scope `{name}`"));
            assert_eq!(
                issue.owner,
                Some(ValidationOwner::Scope(ValidationScopeLocation {
                    target: ValidationEndpoint::Target,
                    path: vec![ValidationScopeStep::Child(0)],
                }))
            );
        }
    }
    Ok(())
}

#[test]
fn explicit_scalar_and_constructed_group_targets_keep_all_values_and_identities()
-> Result<(), Box<dyn Error>> {
    let mut mixed = project();
    mixed.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("AUTHORED".into()),
        },
    );
    let mut text = named("Text", true);
    text.root.construction = ScopeConstruction::Scalar { value: 1 };
    mixed.extra_targets = vec![named("First", false), text, named("Last", false)];
    let mut scalar_primary = project();
    scalar_primary.target = SchemaNode::scalar("Primary", ScalarType::String);
    scalar_primary.graph = mixed.graph.clone();
    scalar_primary.root.construction = ScopeConstruction::Scalar { value: 1 };
    scalar_primary.extra_targets = vec![named("Group", false)];
    for (label, project) in [("mixed", mixed), ("scalar-primary", scalar_primary)] {
        let path = evidence(label, &project)?;
        let input = Instance::Group(Vec::new().into());
        let expected_primary = if label == "mixed" {
            Instance::Group(Vec::new().into())
        } else {
            Instance::Scalar(Value::String("AUTHORED".into()))
        };
        let expected_extras = if label == "mixed" {
            vec![
                engine::NamedOutput {
                    name: "First".into(),
                    instance: Instance::Group(Vec::new().into()),
                },
                engine::NamedOutput {
                    name: "Text".into(),
                    instance: Instance::Scalar(Value::String("AUTHORED".into())),
                },
                engine::NamedOutput {
                    name: "Last".into(),
                    instance: Instance::Group(Vec::new().into()),
                },
            ]
        } else {
            vec![engine::NamedOutput {
                name: "Group".into(),
                instance: Instance::Group(Vec::new().into()),
            }]
        };
        retain(
            &path,
            "input-and-expected-before-run",
            (&input, &expected_primary, &expected_extras),
        )?;
        let issues = engine::validate(&project);
        retain(&path, "native-validation", &issues)?;
        let lowered = codegen::lower(&project);
        retain(&path, "lowering", &lowered)?;
        let output = engine::run_outputs(&project, &input);
        retain(&path, "complete-native-output", &output)?;
        assert!(issues.is_empty());
        let program = lowered?;
        let validation = codegen::validate_program(&program);
        retain(&path, "generated-validation", &validation)?;
        validation?;
        assert_eq!(program.target, project.target);
        assert_eq!(
            program
                .extra_targets
                .iter()
                .map(|target| target.name.as_str())
                .collect::<Vec<_>>(),
            project
                .extra_targets
                .iter()
                .map(|target| target.name.as_str())
                .collect::<Vec<_>>()
        );
        for (actual, original) in program.extra_targets.iter().zip(&project.extra_targets) {
            assert_eq!(actual.target, original.schema);
        }
        let output = output?;
        assert_eq!(output.primary, expected_primary);
        assert_eq!(output.extras, expected_extras);
    }
    Ok(())
}
