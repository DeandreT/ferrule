use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{DynamicBinding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration};
use mfd::ExportProfile;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-csv-dynamic-format-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn computed_property_project() -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Item",
                    vec![
                        SchemaNode::scalar("Key", ScalarType::String),
                        SchemaNode::scalar("Value", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group("Values", Vec::new())
                    .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::String))
                    .unwrap(),
            ],
        ),
        source_path: Some("source.json".into()),
        target_path: None,
        source_options: FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        },
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Item".into(), "Key".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Item".into(), "Value".into()],
                        frame: None,
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Values".into(),
                iteration: ScopeIteration::Source(vec!["Item".into()]),
                dynamic_bindings: vec![DynamicBinding { key: 0, value: 1 }],
                merge_dynamic_fields: true,
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn pathless_computed_json_target_does_not_discard_explicit_csv_options()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let control = computed_property_project();
    assert!(engine::validate(&control).is_empty());
    let source = format_json::from_str(
        r#"{"Item":[{"Key":"first","Value":"one"},{"Key":"second","Value":"two"}]}"#,
        &control.source,
    )?;
    let expected = engine::run(&control, &source)?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&format_json::to_string(
            &control.target,
            &expected,
        )?)?,
        serde_json::json!({"Values":{"first":"one","second":"two"}})
    );
    for (profile_index, profile) in [ExportProfile::FerruleExtensions, ExportProfile::NativeMfd]
        .into_iter()
        .enumerate()
    {
        let control_path = dir.0.join(format!("control-{profile_index}.mfd"));
        mfd::export_with_profile(&control, &control_path, profile)?;
        let reimported = mfd::import(&control_path)?;
        assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
        assert_eq!(expected, engine::run(&reimported.project, &source)?);

        for (case, options) in [
            (
                "quote",
                FormatOptions {
                    csv_quote: Some('\''),
                    ..FormatOptions::default()
                },
            ),
            (
                "quote-disabled",
                FormatOptions {
                    csv_quote_disabled: true,
                    ..FormatOptions::default()
                },
            ),
            (
                "bom",
                FormatOptions {
                    csv_utf8_bom: true,
                    ..FormatOptions::default()
                },
            ),
            (
                "empty-text",
                FormatOptions {
                    csv_preserve_empty_strings: true,
                    ..FormatOptions::default()
                },
            ),
        ] {
            let mut project = control.clone();
            project.target_options = options;
            assert!(engine::validate(&project).is_empty(), "{case}");
            let parent = dir.0.join(format!("rejected-{profile_index}-{case}"));
            let output = parent.join("mapping.mfd");
            let error = mfd::export_with_profile(&project, &output, profile).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("computed JSON property targets conflict with a non-JSON format"),
                "{case}: {error}"
            );
            assert!(!parent.exists(), "{case} created an export directory");

            std::fs::create_dir(&parent)?;
            std::fs::write(&output, b"sentinel")?;
            assert!(mfd::export_with_profile(&project, &output, profile).is_err());
            assert_eq!(std::fs::read(&output)?, b"sentinel");
            assert_eq!(std::fs::read_dir(&parent)?.count(), 1);
        }
    }
    Ok(())
}
