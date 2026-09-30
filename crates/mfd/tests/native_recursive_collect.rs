use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value, XmlRepeatingChoice};
use mapping::{
    FormatOptions, Graph, Node, Project, Scope, ScopeConstruction, ScopeIteration, SequenceExpr,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_native_recursive_collect_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
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

fn project() -> Project {
    let name = SchemaNode::scalar("name", ScalarType::String).attribute();
    let size = SchemaNode::scalar("size", ScalarType::Int).attribute();
    let file = SchemaNode::group("file", vec![name.clone(), size]).repeating();
    let child = SchemaNode::recursive_group("directory", "directory").repeating();
    let mut source = SchemaNode::group("directory", vec![file, child, name]);
    assert!(source.set_xml_repeating_choices(vec![XmlRepeatingChoice {
        required: false,
        repeating: true,
        members: vec!["file".into(), "directory".into()],
    }]));
    let target = SchemaNode::group(
        "FileList",
        vec![SchemaNode::scalar("File", ScalarType::String).repeating()],
    );
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                0,
                Node::Const {
                    value: Value::String(String::new()),
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::String("\\".into()),
                },
            ),
            (
                2,
                Node::SourceField {
                    path: Vec::new(),
                    frame: None,
                },
            ),
        ]),
    };
    Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        },
        target_options: FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "File".into(),
                iteration: ScopeIteration::Sequence(SequenceExpr::RecursiveCollect {
                    collection: Vec::new(),
                    children: vec!["directory".into()],
                    descent_value: vec!["name".into()],
                    values: vec!["file".into()],
                    value: vec!["name".into()],
                    prefix: 0,
                    separator: 1,
                    item: 2,
                }),
                construction: ScopeConstruction::Scalar { value: 2 },
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn paths(output: &Instance) -> Vec<&str> {
    output
        .field("File")
        .and_then(Instance::as_repeated)
        .unwrap_or_else(|| panic!("repeated File values in {output:?}"))
        .iter()
        .map(|item| match item.as_scalar() {
            Some(Value::String(path)) => path.as_str(),
            other => panic!("unexpected file path {other:?}"),
        })
        .collect()
}

fn xml(project: &Project, output: &Instance) -> Result<String, format_xml::XmlFormatError> {
    format_xml::to_string_with_options(
        &project.target,
        output,
        &format_xml::XmlWriteOptions {
            declaration: false,
            indent: false,
            default_namespace: None,
        },
    )
}

fn assert_restored_collect(project: &Project) {
    let [scope] = project.root.children.as_slice() else {
        panic!("expected one recursive file-list scope");
    };
    let ScopeIteration::Sequence(SequenceExpr::RecursiveCollect {
        collection,
        children,
        descent_value,
        values,
        value,
        prefix,
        separator,
        item,
    }) = &scope.iteration
    else {
        panic!("expected a native recursive-collect sequence");
    };
    assert!(collection.is_empty());
    assert_eq!(children, &["directory"]);
    assert_eq!(descent_value, &["name"]);
    assert_eq!(values, &["file"]);
    assert_eq!(value, &["name"]);
    assert_eq!(
        scope.construction,
        ScopeConstruction::Scalar { value: *item }
    );
    assert!(
        matches!(project.graph.nodes.get(prefix), Some(Node::Const { value: Value::String(text) }) if text.is_empty())
    );
    assert!(
        matches!(project.graph.nodes.get(separator), Some(Node::Const { value: Value::String(text) }) if text == "\\")
    );
    assert!(
        matches!(project.graph.nodes.get(item), Some(Node::SourceField { path, frame: None }) if path.is_empty())
    );
}

fn strict_rejected(project: &Project, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let report = mfd::preflight_export(project, path)?;
    assert!(
        report
            .issues
            .iter()
            .any(|issue| { issue.feature == mfd::ExportCompatibilityFeature::RecursiveComponent })
    );
    assert!(matches!(
        mfd::export_with_profile(project, path, mfd::ExportProfile::NativeMfd),
        Err(mfd::MfdError::IncompatibleExport(_))
    ));
    assert!(!path.exists());
    Ok(())
}

#[test]
fn interleaved_choice_still_emits_direct_files_before_child_files()
-> Result<(), Box<dyn std::error::Error>> {
    let project = project();
    assert!(engine::validate(&project).is_empty());
    let input = format_xml::from_str(
        r#"<directory name="root"><directory name="child"><file name="nested.xml" size="3"/></directory><file name="direct.xml" size="2"/></directory>"#,
        &project.source,
    )?;
    let expected = engine::run(&project, &input)?;
    assert_eq!(
        paths(&expected),
        vec!["\\root\\direct.xml", "\\root\\child\\nested.xml"]
    );

    let dir = TempDir::new()?;
    let design = dir.0.join("native.mfd");
    let report = mfd::preflight_export(&project, &design)?;
    assert!(report.is_native_compatible(), "{report}");
    mfd::export_with_profile(&project, &design, mfd::ExportProfile::NativeMfd)?;
    let emitted = std::fs::read_to_string(&design)?;
    assert!(emitted.contains("library=\"user\""));
    assert!(!emitted.contains("library=\"ferrule\""));
    let mapping = roxmltree::Document::parse(&emitted)?;
    let definition = mapping
        .root_element()
        .children()
        .find(|node| {
            node.has_tag_name("component")
                && node.attribute("editable") == Some("1")
                && node.attribute("library") == Some("user")
        })
        .ok_or("missing editable recursive function")?;
    let cloned_files = definition
        .descendants()
        .filter(|node| {
            node.has_tag_name("component")
                && node.attribute("library") == Some("xml")
                && node.attribute("name") == Some("FileList")
        })
        .flat_map(|component| {
            component
                .descendants()
                .filter(|node| node.has_tag_name("entry") && node.attribute("name") == Some("File"))
        })
        .collect::<Vec<_>>();
    assert_eq!(cloned_files.len(), 2);
    assert_eq!(cloned_files[0].attribute("clone"), None);
    assert_eq!(cloned_files[1].attribute("clone"), Some("1"));
    let restored = mfd::import(&design)?;
    assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
    assert_restored_collect(&restored.project);
    assert!(engine::validate(&restored.project).is_empty());
    let actual = engine::run(&restored.project, &input)?;
    assert_eq!(paths(&actual), paths(&expected));
    assert_eq!(xml(&restored.project, &actual)?, xml(&project, &expected)?);
    Ok(())
}

#[test]
fn altered_prefix_separator_and_schema_keep_native_guard_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let baseline = project();
    for (label, id, value) in [("prefix", 0, "prefix"), ("separator", 1, "/")] {
        let mut changed = baseline.clone();
        changed.graph.nodes.insert(
            id,
            Node::Const {
                value: Value::String(value.into()),
            },
        );
        assert!(engine::validate(&changed).is_empty());
        strict_rejected(&changed, &dir.0.join(format!("{label}.mfd")))?;
    }
    let mut changed_schema = baseline.clone();
    let SchemaKind::Group { children, .. } = &mut changed_schema.source.kind else {
        unreachable!()
    };
    let SchemaKind::Group {
        children: file_fields,
        ..
    } = &mut children[0].kind
    else {
        unreachable!()
    };
    file_fields.push(SchemaNode::scalar("extra", ScalarType::String).attribute());
    assert!(engine::validate(&changed_schema).is_empty());
    strict_rejected(&changed_schema, &dir.0.join("changed-schema.mfd"))?;
    Ok(())
}

#[test]
fn shared_graph_and_sequence_controls_keep_native_guard_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let mut shared_graph = project();
    shared_graph.graph.nodes.insert(
        3,
        Node::Call {
            function: "concat".into(),
            args: vec![0, 1],
        },
    );
    assert!(engine::validate(&shared_graph).is_empty());
    strict_rejected(&shared_graph, &dir.0.join("shared-graph.mfd"))?;

    let mut sorted = project();
    sorted.root.children[0].sort_by = Some(0);
    assert!(engine::validate(&sorted).is_empty());
    assert!(matches!(
        mfd::preflight_export(&sorted, &dir.0.join("sorted.mfd")),
        Err(mfd::MfdError::Unsupported(_))
    ));
    assert!(matches!(
        mfd::export_with_profile(
            &sorted,
            &dir.0.join("sorted.mfd"),
            mfd::ExportProfile::NativeMfd
        ),
        Err(mfd::MfdError::Unsupported(_))
    ));
    assert!(!dir.0.join("sorted.mfd").exists());
    Ok(())
}

#[test]
#[ignore = "needs the local ignored ReferenceSamples corpus"]
fn local_flatten_hierarchy_native_roundtrip_keeps_exact_90_paths()
-> Result<(), Box<dyn std::error::Error>> {
    let samples = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/ReferenceSamples")
        .canonicalize()?;
    let imported = mfd::import_with_options(
        &samples.join("FlattenHierarchy.mfd"),
        &mfd::ImportOptions::default().with_package_root(&samples),
    )?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let source = format_xml::read(&samples.join("Directory.xml"), &imported.project.source)?;
    let before = engine::run(&imported.project, &source)?;
    let expected_paths = paths(&before);
    assert_eq!(expected_paths.len(), 90);
    assert_eq!(
        expected_paths
            .iter()
            .filter(|path| path.matches('\\').count() == 3)
            .count(),
        61
    );
    assert_eq!(
        expected_paths
            .iter()
            .filter(|path| path.matches('\\').count() == 4)
            .count(),
        24
    );
    assert_eq!(
        expected_paths
            .iter()
            .filter(|path| path.matches('\\').count() == 5)
            .count(),
        5
    );

    let dir = TempDir::new()?;
    let design = dir.0.join("native.mfd");
    let report = mfd::preflight_export(&imported.project, &design)?;
    assert!(report.is_native_compatible(), "{report}");
    mfd::export_with_profile(&imported.project, &design, mfd::ExportProfile::NativeMfd)?;
    let emitted = std::fs::read_to_string(&design)?;
    assert!(!emitted.contains("library=\"ferrule\""));
    let restored = mfd::import(&design)?;
    assert!(restored.warnings.is_empty(), "{:?}", restored.warnings);
    assert_restored_collect(&restored.project);
    assert!(engine::validate(&restored.project).is_empty());
    let after = engine::run(&restored.project, &source)?;
    assert_eq!(paths(&after), paths(&before));
    assert_eq!(
        xml(&restored.project, &after)?,
        xml(&imported.project, &before)?
    );
    Ok(())
}
