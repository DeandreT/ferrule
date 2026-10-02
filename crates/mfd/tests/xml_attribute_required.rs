use ir::{ScalarType, SchemaNode};
use mapping::{Binding, Graph, Node, Project, Scope};
use mfd::{ExportProfile, ImportIssueKind, ImportOptions, ImportProfile, MfdError};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "ferrule_attribute_mapping_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn source(root: &str, profile: &str) -> String {
    let body = match profile {
        "default" => format!(
            r#"<xs:attribute name="Code" type="xs:string" default="fallback"/><xs:element name="{root}"><xs:complexType><xs:attribute ref="Code" use="required"/></xs:complexType></xs:element>"#
        ),
        "view" => format!(
            r#"<xs:complexType name="Base"><xs:attribute name="Code" type="xs:string"/></xs:complexType><xs:complexType name="Derived"><xs:complexContent><xs:restriction base="Base"><xs:attribute name="Code" type="xs:string" use="required"/></xs:restriction></xs:complexContent></xs:complexType><xs:element name="{root}" type="Base"/>"#
        ),
        _ => format!(
            r#"<xs:element name="{root}"><xs:complexType><xs:attribute name="Code" type="xs:string" use="required"/></xs:complexType></xs:element>"#
        ),
    };
    format!(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">{body}</xs:schema>"#)
}
fn entry(root: &str, port: &str) -> String {
    format!(
        r#"<entry name="FileInstance"><entry name="document"><entry name="{root}"><entry name="Code" type="attribute" {port}/><entry name="Unused"/><entry name="Nested"><entry name="Later"/><entry name="Last"/></entry></entry></entry></entry>"#
    )
}
fn design() -> String {
    format!(
        r#"<mapping version="26"><component name="map"><structure><children><component name="source" library="xml" kind="14"><data><root>{}</root><document schema="source.xsd" inputinstance="input.xml" instanceroot="{{}}Input"/></data></component><component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root>{}</root><document schema="target.xsd" outputinstance="output.xml" instanceroot="{{}}Output"/></data></component></children><graph directed="1"><edges><edge edgekey="1"><data><dataconnection type="2"/></data></edge></edges><vertices><vertex vertexkey="10"><edges><edge vertexkey="20" edgekey="1"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
        entry("Input", "outkey=\"10\""),
        entry("Output", "inpkey=\"20\"")
    )
}

#[test]
fn rejected_required_profiles_keep_complete_entry_tree_and_executable_warning_gate() {
    for profile in ["default", "view"] {
        for side in ["source", "target"] {
            let d = Dir::new();
            for (name, root) in [("source", "Input"), ("target", "Output")] {
                std::fs::write(
                    d.0.join(format!("{name}.xsd")),
                    source(root, if name == side { profile } else { "plain" }),
                )
                .unwrap();
            }
            let path = d.0.join("mapping.mfd");
            std::fs::write(&path, design()).unwrap();
            let typed = format_xml::xsd::import_root(
                &d.0.join(format!("{side}.xsd")),
                Some(if side == "source" { "Input" } else { "Output" }),
            )
            .unwrap_err();
            match profile {
                "default" => assert!(matches!(
                    typed,
                    format_xml::XmlFormatError::UnsupportedXmlAttributeDefault { .. }
                )),
                _ => assert!(matches!(
                    typed,
                    format_xml::XmlFormatError::UnsupportedXmlAlternativeAttributeUse { .. }
                )),
            }
            let imported = mfd::import(&path).unwrap();
            assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
            let diagnostic = imported
                .warnings
                .iter()
                .filter(|warning| warning.contains(&typed.to_string()))
                .collect::<Vec<_>>();
            assert_eq!(diagnostic.len(), 1, "{:?}", imported.warnings);
            assert!(diagnostic[0].contains("falling back to the entry tree"));
            let fallback = if side == "source" {
                &imported.project.source
            } else {
                &imported.project.target
            };
            assert!(fallback.child("Code").unwrap().attribute);
            assert!(!fallback.child("Code").unwrap().xml_attribute_required);
            assert!(fallback.child("Unused").is_some());
            assert!(fallback.child("Nested").unwrap().child("Later").is_some());
            assert!(fallback.child("Nested").unwrap().child("Last").is_some());
            let assessment = mfd::assess_import(&imported);
            assert!(!assessment.executable);
            assert_eq!(
                assessment
                    .issues
                    .iter()
                    .filter(|issue| issue.kind == ImportIssueKind::ImportWarning)
                    .count(),
                imported.warnings.len()
            );
            assert!(matches!(
                mfd::import_with_profile(
                    &path,
                    &ImportOptions::default(),
                    ImportProfile::Executable
                ),
                Err(MfdError::IncompatibleImport(_))
            ));
        }
    }
}

fn project(default: bool) -> Project {
    let mut code = SchemaNode::scalar("Code", ScalarType::String).attribute();
    code.xml_attribute_required = true;
    if default {
        code.default = Some("prior".into());
    }
    Project {
        source: SchemaNode::group(
            "Input",
            vec![SchemaNode::scalar("Code", ScalarType::String).attribute()],
        ),
        target: SchemaNode::group("Output", vec![code]),
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["Code".into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Code".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn native_schema_export_rejection_is_atomic_for_missing_and_existing_artifacts() {
    let p = project(true);
    assert!(engine::validate(&p).is_empty());
    let d = Dir::new();
    let path = d.0.join("missing/mapping.mfd");
    for result in [
        mfd::preflight_export(&p, &path),
        mfd::export_with_profile(&p, &path, ExportProfile::NativeMfd),
    ] {
        assert!(matches!(
            result,
            Err(MfdError::SchemaExport(
                format_xml::XmlFormatError::UnsupportedXmlAttributeDefault { .. }
            ))
        ));
        assert!(!path.parent().unwrap().exists());
    }
    let path = d.0.join("mapping.mfd");
    let files = [
        path.clone(),
        d.0.join("mapping-source.xsd"),
        d.0.join("mapping-target.xsd"),
    ];
    for (index, file) in files.iter().enumerate() {
        std::fs::write(file, format!("sentinel-{index}")).unwrap();
    }
    assert!(matches!(
        mfd::export_with_profile(&p, &path, ExportProfile::NativeMfd),
        Err(MfdError::SchemaExport(
            format_xml::XmlFormatError::UnsupportedXmlAttributeDefault { .. }
        ))
    ));
    for (index, file) in files.iter().enumerate() {
        assert_eq!(
            std::fs::read_to_string(file).unwrap(),
            format!("sentinel-{index}")
        );
    }
    assert_eq!(std::fs::read_dir(&d.0).unwrap().count(), 3);
}

#[test]
fn ordinary_required_attribute_native_export_reimport_and_lenient_mapping_are_exact() {
    let d = Dir::new();
    let p = project(false);
    let path = d.0.join("mapping.mfd");
    let report = mfd::export_with_profile(&p, &path, ExportProfile::NativeMfd).unwrap();
    assert!(report.is_native_compatible());
    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        imported
            .project
            .target
            .child("Code")
            .unwrap()
            .xml_attribute_required
    );
    assert!(
        !imported
            .project
            .source
            .child("Code")
            .unwrap()
            .xml_attribute_required
    );
    for text in ["<Input/>", r#"<Input Code=""/>"#, r#"<Input Code="x"/>"#] {
        let input = format_xml::from_str(text, &p.source).unwrap();
        assert_eq!(
            engine::run(&p, &input).unwrap(),
            engine::run(&imported.project, &input).unwrap()
        );
    }
}
