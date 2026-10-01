use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope};
use mfd::{ExportCompatibility, ExportProfile};

const VALUE: &str = "start\tline\nreturn\r\nend & <\" &#13;";
const ENCODED: &str = "start&#9;line&#10;return&#13;&#10;end &amp; &lt;&quot; &amp;#13;";
const FIELDS: [&str; 4] = [
    "FixedElement",
    "DefaultElement",
    "fixedAttribute",
    "defaultAttribute",
];

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_xml_schema_whitespace_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        FIELDS
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let mut node = SchemaNode::scalar(*name, ScalarType::String);
                if index % 2 == 0 {
                    node.fixed = Some(VALUE.into());
                } else {
                    node.default = Some(VALUE.into());
                }
                node.attribute = index >= 2;
                node
            })
            .collect(),
    )
}

fn project() -> Project {
    Project {
        source: schema("Source"),
        target: schema("Target"),
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: FormatOptions::default(),
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: FIELDS
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    (
                        index as u32,
                        Node::SourceField {
                            path: vec![(*name).into()],
                            frame: None,
                        },
                    )
                })
                .collect(),
        },
        root: Scope {
            bindings: FIELDS
                .iter()
                .enumerate()
                .map(|(index, name)| Binding {
                    target_field: (*name).into(),
                    node: index as u32,
                })
                .collect(),
            ..Scope::default()
        },
    }
}

fn assert_runtime(project: &Project) -> Result<(), Box<dyn Error>> {
    let input = format!(
        "<Source fixedAttribute=\"{ENCODED}\"><FixedElement>{ENCODED}</FixedElement><DefaultElement/></Source>"
    );
    let source = format_xml::from_str(&input, &project.source)?;
    for name in FIELDS {
        assert_eq!(
            source.field(name).and_then(Instance::as_scalar),
            Some(&Value::String(VALUE.into())),
            "{name}"
        );
    }
    assert!(matches!(
        format_xml::from_str(&input.replace("start&#9;", "changed&#9;"), &project.source),
        Err(format_xml::XmlFormatError::FixedValue { .. })
    ));
    let target = engine::run(project, &source)?;
    let output = format_xml::to_string(&project.target, &target)?;
    assert!(output.contains("return&#xD;\nend"), "{output}");
    assert_eq!(format_xml::from_str(&output, &project.target)?, target);
    let mut invalid = target.clone();
    let Instance::Group(fields) = &mut invalid else {
        return Err("expected a target group".into());
    };
    let (_, fixed) = fields
        .iter_mut()
        .find(|(name, _)| name == "FixedElement")
        .ok_or("missing FixedElement")?;
    *fixed = Instance::Scalar(Value::String("different".into()));
    assert!(matches!(
        format_xml::to_string(&project.target, &invalid),
        Err(format_xml::XmlFormatError::FixedValue { name, .. }) if name == "FixedElement"
    ));
    Ok(())
}

#[test]
fn strict_xml_roundtrips_preserve_fixed_default_values_and_boundary_behavior()
-> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    let original = project();
    assert_runtime(&original)?;
    let mut current = original.clone();
    for cycle in 0..2 {
        let path = directory.0.join(format!("cycle-{cycle}.mfd"));
        let report = mfd::preflight_export(&current, &path)?;
        assert_eq!(report.compatibility, ExportCompatibility::NativeMfd);
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(
            mfd::export_with_profile(&current, &path, ExportProfile::NativeMfd)?,
            report
        );
        for side in ["source", "target"] {
            let xsd =
                std::fs::read_to_string(directory.0.join(format!("cycle-{cycle}-{side}.xsd")))?;
            assert!(xsd.contains(&format!("fixed=\"{ENCODED}\"")), "{xsd}");
            assert!(xsd.contains(&format!("default=\"{ENCODED}\"")), "{xsd}");
        }
        let imported = mfd::import(&path)?;
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert_eq!(imported.project.source, original.source);
        assert_eq!(imported.project.target, original.target);
        assert_runtime(&imported.project)?;
        current = imported.project;
    }
    Ok(())
}

#[test]
fn forbidden_xml_schema_values_fail_before_artifact_publication() -> Result<(), Box<dyn Error>> {
    let directory = TempDirectory::new()?;
    for character in ['\0', '\u{b}', '\u{c}', '\u{fffe}', '\u{ffff}'] {
        let mut project = project();
        let ir::SchemaKind::Group { children, .. } = &mut project.source.kind else {
            return Err("expected a source group".into());
        };
        children[0].fixed = Some(format!("before{character}after"));
        let path = directory.0.join("uncreated/mapping.mfd");
        assert!(
            mfd::preflight_export(&project, &path).is_err(),
            "{character:?}"
        );
        assert!(
            mfd::export_with_profile(&project, &path, ExportProfile::NativeMfd).is_err(),
            "{character:?}"
        );
        assert!(!directory.0.join("uncreated").exists());
    }
    Ok(())
}
