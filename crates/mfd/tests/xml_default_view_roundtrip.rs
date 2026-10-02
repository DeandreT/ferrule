use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_default_view_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn authored_project(
    directory: &Path,
    abstract_base: bool,
    included: bool,
    derived: bool,
) -> Project {
    let base = format!(
        r#"<xs:complexType name="Base" abstract="{abstract_base}"><xs:sequence><xs:element name="Label" type="xs:string"/></xs:sequence></xs:complexType>"#
    );
    let extension = if derived {
        r#"<xs:complexType name="Extended"><xs:complexContent><xs:extension base="t:Base"><xs:sequence><xs:element name="Code" type="xs:string"/></xs:sequence></xs:extension></xs:complexContent></xs:complexType>"#
    } else {
        ""
    };
    let declarations = format!(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:t="urn:default-view" targetNamespace="urn:default-view" elementFormDefault="qualified">{base}{extension}</xs:schema>"#
    );
    std::fs::write(directory.join("types.xsd"), declarations).unwrap();
    let declarations = if included {
        r#"<xs:include schemaLocation="types.xsd"/>"#.to_string()
    } else {
        format!("{base}{extension}")
    };
    let root = format!(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:t="urn:default-view" targetNamespace="urn:default-view" elementFormDefault="qualified">{declarations}<xs:element name="Envelope"><xs:complexType><xs:sequence><xs:element name="Record" type="t:Base" minOccurs="0"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#
    );
    let schema_path = directory.join("root.xsd");
    std::fs::write(&schema_path, root).unwrap();
    let source =
        format_xml::xsd::import_root(&schema_path, Some("{urn:default-view}Envelope")).unwrap();
    Project {
        source,
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Label", ScalarType::String)],
        ),
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
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["Record".into(), "Label".into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Label".into(),
                node: 0,
            }],
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

fn roundtrip(project: &Project, path: &Path) -> Project {
    assert!(engine::validate(project).is_empty());
    assert!(mfd::export(project, path).unwrap().is_empty());
    let result = mfd::import_with_profile(
        path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap();
    assert!(result.imported.warnings.is_empty());
    assert!(engine::validate(&result.imported.project).is_empty());
    result.imported.project
}

fn check_default(abstract_base: bool, included: bool, derived: bool) {
    let directory = Directory::new();
    let project = authored_project(&directory.0, abstract_base, included, derived);
    let original = project.source.child("Record").unwrap();
    let expected = (!abstract_base && derived).then_some("{urn:default-view}Base");
    assert_eq!(original.xml_default_type.as_deref(), expected);
    let imported = roundtrip(&project, &directory.0.join("mapping.mfd"));
    let generated = format_xml::xsd::import_root(
        &directory.0.join("mapping-source.xsd"),
        Some("{urn:default-view}Envelope"),
    )
    .unwrap();
    assert_eq!(
        generated
            .child("Record")
            .unwrap()
            .xml_default_type
            .as_deref(),
        expected
    );
    assert_eq!(
        imported.source.child("Record").unwrap(),
        generated.child("Record").unwrap()
    );
    assert_eq!(
        imported
            .source
            .child("Record")
            .unwrap()
            .xml_default_type
            .as_deref(),
        expected
    );
    let second = roundtrip(&imported, &directory.0.join("second.mfd"));
    assert_eq!(
        second.source.child("Record").unwrap(),
        imported.source.child("Record").unwrap()
    );
}

#[test]
fn conditioned_view_retains_resolved_concrete_default() {
    check_default(false, false, true);
}

#[test]
fn conditioned_view_retains_included_concrete_default() {
    check_default(false, true, true);
}

#[test]
fn conditioned_view_does_not_infer_abstract_default() {
    check_default(true, false, true);
}

#[test]
fn conditioned_view_does_not_infer_included_abstract_default() {
    check_default(true, true, true);
}

#[test]
fn plain_legacy_type_does_not_gain_alternative_default() {
    check_default(false, false, false);
}

#[test]
fn unresolved_conditioned_type_remains_explicit_strict_rejection() {
    let directory = Directory::new();
    let project = authored_project(&directory.0, false, false, true);
    let path = directory.0.join("mapping.mfd");
    roundtrip(&project, &path);
    let original = std::fs::read_to_string(&path).unwrap();
    let corrupted = original.replace("{urn:default-view}Extended", "{urn:default-view}Missing");
    assert_ne!(corrupted, original);
    std::fs::write(&path, corrupted).unwrap();
    let result = mfd::import_with_profile(
        &path,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    );
    assert!(matches!(result, Err(mfd::MfdError::IncompatibleImport(_))));
}
