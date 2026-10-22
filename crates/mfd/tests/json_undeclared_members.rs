//! Authored JSON target members admitted by an open object without being
//! declared, such as `code` beside a declared `Code`.
//!
//! Expected Projects are authored here independently of the importer. The
//! shared case catalogue also drives the generated Rust and C# hosts.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ir::{IntegerRange, NumericRange, ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const EXECUTABLE_DESIGNS: [&str; 4] = ["open", "typed", "nested", "matching"];

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/json-undeclared-members")
        .join(name)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_json_undeclared_{}_{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn int(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Int)
}

fn any(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
        .json_any()
        .unwrap()
}

fn open(name: &str, children: Vec<SchemaNode>, dynamic: SchemaNode) -> SchemaNode {
    SchemaNode::group(name, children)
        .with_dynamic_fields(dynamic)
        .unwrap()
}

/// An authored member: an exact copy of the dynamic value schema under its
/// own name, built without the importer's helper.
fn member(name: &str, mut value: SchemaNode) -> SchemaNode {
    value.name = name.into();
    value.json_undeclared_property = true;
    value
}

fn non_negative() -> SchemaNode {
    int("*")
        .with_numeric_range(NumericRange::Integer(
            IntegerRange::new(Some(0), None).unwrap(),
        ))
        .unwrap()
}

fn bind(target_field: &str, node: u32) -> Binding {
    Binding {
        target_field: target_field.into(),
        node,
    }
}

fn source_field(name: &str) -> Node {
    Node::SourceField {
        path: vec![name.into()],
        frame: None,
    }
}

fn expected_project(design: &str) -> Project {
    let source = open(
        "root",
        vec![
            int("Value"),
            SchemaNode::scalar("Label", ScalarType::String),
        ],
        any("*"),
    );
    let code_only = |dynamic: SchemaNode| {
        open(
            "root",
            vec![int("Code"), member("code", dynamic.clone())],
            dynamic,
        )
        .with_required_fields(vec!["code".into()])
        .unwrap()
    };
    let target = match design {
        "open" => code_only(any("*")),
        "typed" => code_only(non_negative()),
        "nested" => open(
            "root",
            vec![
                int("Code"),
                open(
                    "Item",
                    vec![int("Code"), member("code", any("*"))],
                    any("*"),
                ),
                member("code", any("*")),
            ],
            any("*"),
        ),
        "matching" => open("root", vec![int("code")], any("*"))
            .with_required_fields(vec!["code".into()])
            .unwrap(),
        other => panic!("no authored Project for {other}"),
    };
    let (nodes, root) = if design == "nested" {
        (
            BTreeMap::from([(0, source_field("Value")), (1, source_field("Label"))]),
            Scope {
                bindings: vec![bind("Code", 0), bind("code", 1)],
                children: vec![Scope {
                    target_field: "Item".into(),
                    bindings: vec![bind("code", 0)],
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
    } else {
        (
            BTreeMap::from([(0, source_field("Value"))]),
            Scope {
                bindings: vec![bind("code", 0)],
                ..Default::default()
            },
        )
    };
    let json = FormatOptions {
        json_document: true,
        ..Default::default()
    };
    Project {
        source,
        target,
        source_path: Some("source.json".into()),
        target_path: Some("target.json".into()),
        source_options: json.clone(),
        target_options: json,
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph { nodes },
        root,
    }
}

fn cases(design: &str) -> TestResult<Vec<serde_json::Value>> {
    let catalogue: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(fixture("cases.json"))?)?;
    let cases = catalogue
        .into_iter()
        .filter(|case| case["design"] == design)
        .collect::<Vec<_>>();
    assert!(!cases.is_empty(), "{design}");
    Ok(cases)
}

#[test]
fn permitted_members_import_as_complete_projects() -> TestResult<()> {
    for design in EXECUTABLE_DESIGNS {
        let path = fixture(&format!("{design}.mfd"));
        let imported = mfd::import(&path)?;
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        let expected = expected_project(design);
        assert_eq!(
            mapping::project_file::encode_pretty(&imported.project)?,
            mapping::project_file::encode_pretty(&expected)?,
            "{design}: complete imported Project"
        );
        let executable = mfd::import_with_profile(
            &path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )?;
        assert_eq!(
            mapping::project_file::encode_pretty(&executable.imported.project)?,
            mapping::project_file::encode_pretty(&expected)?,
            "{design}: complete Executable Project"
        );
        assert!(engine::validate(&imported.project).is_empty(), "{design}");
    }
    Ok(())
}

#[test]
fn distinct_names_are_never_case_folded() -> TestResult<()> {
    let target = mfd::import(&fixture("open.mfd"))?.project.target;
    let names = match &target.kind {
        ir::SchemaKind::Group { children, .. } => children
            .iter()
            .map(|child| (child.name.as_str(), child.json_undeclared_property))
            .collect::<Vec<_>>(),
        _ => panic!("target root is an object"),
    };
    assert_eq!(names, [("Code", false), ("code", true)]);
    assert_eq!(
        target.json_property_presence("code"),
        Some(ir::EffectivePresence::Required)
    );
    assert_eq!(
        target.json_property_presence("Code"),
        Some(ir::EffectivePresence::Optional)
    );
    Ok(())
}

#[test]
fn closed_objects_still_refuse_undeclared_members() -> TestResult<()> {
    let path = fixture("closed.mfd");
    let imported = mfd::import(&path)?;
    assert_eq!(
        imported.warnings,
        ["target port path `code` not found in schema"]
    );
    let target = &imported.project.target;
    assert!(target.child("code").is_none());
    assert!(target.dynamic_fields().is_none());
    assert!(matches!(
        mfd::import_with_profile(
            &path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        ),
        Err(mfd::MfdError::IncompatibleImport(_))
    ));
    Ok(())
}

/// Ferrule refuses an absent required member, as it does for the declared
/// matching-name control, instead of emitting the reference's `{}`.
#[test]
fn native_outputs_match_the_case_catalogue() -> TestResult<()> {
    for design in EXECUTABLE_DESIGNS {
        let project = mfd::import(&fixture(&format!("{design}.mfd")))?.project;
        for case in cases(design)? {
            let input = case["input"].as_str().unwrap();
            let source = format_json::from_str(input, &project.source);
            if case["reject"] == true {
                assert!(source.is_err(), "{design}: source accepted {case}");
                continue;
            }
            let output = engine::run(&project, &source?)?;
            let text = format_json::to_string(&project.target, &output);
            match case["output_error"].as_str() {
                Some("missing_required") => assert!(
                    matches!(
                        text,
                        Err(format_json::JsonFormatError::MissingRequiredProperty {
                            ref object,
                            ref property,
                        }) if object == "root" && property == "code"
                    ),
                    "{design}: {case}: {text:?}"
                ),
                Some("numeric_range") => assert!(
                    matches!(
                        text,
                        Err(format_json::JsonFormatError::RangeMismatch { ref name, .. })
                            if name == "code"
                    ),
                    "{design}: {case}: {text:?}"
                ),
                Some(other) => panic!("unknown output error {other}"),
                None => {
                    let text = text?;
                    assert_eq!(
                        text,
                        format!("{}\n", serde_json::to_string_pretty(&case["expected"])?),
                        "{design}: literal native text bytes: {case}"
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn json_schema_export_keeps_the_authored_schema() -> TestResult<()> {
    for design in ["open", "typed", "nested"] {
        let project = mfd::import(&fixture(&format!("{design}.mfd")))?.project;
        let authored =
            format_json::json_schema::import(&fixture(&format!("{design}.schema.json")))?;
        assert_ne!(project.target, authored, "{design}");
        assert_eq!(
            format_json::json_schema::export(&project.target)?,
            format_json::json_schema::export(&authored)?,
            "{design}"
        );
    }
    Ok(())
}

#[test]
fn members_roundtrip_through_mfd_export() -> TestResult<()> {
    let dir = TempDir::new("roundtrip")?;
    for design in EXECUTABLE_DESIGNS {
        let project = mfd::import(&fixture(&format!("{design}.mfd")))?.project;
        let exported = dir.0.join(format!("{design}.mfd"));
        let warnings = mfd::export(&project, &exported)?;
        assert!(warnings.is_empty(), "{design}: {warnings:?}");
        let reimported = mfd::import(&exported)?;
        assert!(
            reimported.warnings.is_empty(),
            "{design}: {:?}",
            reimported.warnings
        );
        assert_eq!(reimported.project.target, project.target, "{design}");
        assert_eq!(
            serde_json::to_value(&reimported.project.root)?,
            serde_json::to_value(&project.root)?,
            "{design}"
        );
    }
    Ok(())
}
