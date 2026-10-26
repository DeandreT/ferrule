use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-xml-schema-reference-{}-{}",
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

#[test]
fn physical_xml_targets_explicitly_preserve_the_no_hint_default() {
    let schema = SchemaNode::group(
        "Envelope",
        vec![
            SchemaNode::scalar("Amount", ScalarType::Int),
            SchemaNode::scalar("Label", ScalarType::String),
        ],
    );
    let mut project = Project {
        source: schema.clone(),
        target: schema,
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
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Amount".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Label".into()],
                        frame: None,
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Amount".into(),
                    node: 0,
                },
                Binding {
                    target_field: "Label".into(),
                    node: 1,
                },
            ],
            ..Default::default()
        },
    };
    project.extra_targets.push(mapping::NamedTarget {
        name: "Additional".into(),
        path: Some("additional.xml".into()),
        schema: project.target.clone(),
        options: Default::default(),
        root: project.root.clone(),
    });
    let directory = Directory::new();
    let path = directory.0.join("mapping.mfd");
    let report = mfd::export_with_profile(&project, &path, mfd::ExportProfile::NativeMfd).unwrap();
    assert!(report.is_native_compatible());
    let xml = std::fs::read_to_string(&path).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let mut targets = 0;
    for component in document
        .descendants()
        .filter(|node| node.has_tag_name("component") && node.attribute("library") == Some("xml"))
    {
        let target = component
            .descendants()
            .any(|node| node.has_tag_name("entry") && node.attribute("inpkey").is_some());
        let properties = component
            .children()
            .find(|node| node.has_tag_name("properties"));
        if target {
            targets += 1;
            assert_eq!(
                properties.and_then(|node| node.attribute("AddSchemaLocation")),
                Some("0")
            );
        } else {
            assert!(
                properties
                    .and_then(|node| node.attribute("AddSchemaLocation"))
                    .is_none()
            );
        }
    }
    assert_eq!(targets, 2);
    let restored = mfd::import_with_profile(
        &path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap()
    .imported
    .project;
    assert_eq!(restored.target_options.xml_schema_hints, None);
    assert!(
        restored
            .extra_targets
            .iter()
            .all(|target| target.options.xml_schema_hints.is_none())
    );
    for amount in [i64::MIN, 0, i64::MAX] {
        let input = format_xml::from_str(
            &format!(
                "<Envelope><Amount>{amount}</Amount><Label>kept &amp; complete</Label></Envelope>"
            ),
            &project.source,
        )
        .unwrap();
        let expected = engine::run_outputs(&project, &input).unwrap();
        let actual = engine::run_outputs(&restored, &input).unwrap();
        assert_eq!(actual.primary, expected.primary);
        assert_eq!(actual.extras, expected.extras);
        assert_eq!(
            actual
                .primary
                .field("Amount")
                .and_then(ir::Instance::as_scalar),
            Some(&Value::Int(amount))
        );
        let output = format_xml::to_string(&restored.target, &actual.primary).unwrap();
        assert!(!output.contains("SchemaLocation"));
        assert_eq!(
            format_xml::from_str(&output, &restored.target).unwrap(),
            actual.primary
        );
    }
}
