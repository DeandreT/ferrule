//! `import-xsd` and native file runs with XSD 1.1 wildcard exclusions.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use mapping::{FormatOptions, Graph, Project, Scope, ScopeConstruction};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_xml_wildcard_exclusions_{}_{}",
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

const SCHEMA: &str = r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
    xmlns:t="urn:ferrule:wx:t" targetNamespace="urn:ferrule:wx:t" elementFormDefault="qualified">
  <xs:element name="Root"><xs:complexType>
    <xs:sequence>
      <xs:element name="Head" type="xs:string"/>
      <xs:any xmlns:a="urn:ferrule:wx:a" notNamespace="##targetNamespace ##local"
              notQName="a:Secret" processContents="lax" minOccurs="0" maxOccurs="unbounded"/>
    </xs:sequence>
    <xs:anyAttribute notQName="internal" processContents="skip"/>
  </xs:complexType></xs:element>
</xs:schema>"###;

fn input(content: &str, attributes: &str) -> String {
    format!(
        r#"<t:Root xmlns:t="urn:ferrule:wx:t" xmlns:a="urn:ferrule:wx:a" {attributes}><t:Head>h</t:Head>{content}</t:Root>"#
    )
}

#[test]
fn import_xsd_and_file_runs_apply_wildcard_exclusions() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let xsd = temp.0.join("schema.xsd");
    std::fs::write(&xsd, SCHEMA)?;

    let output = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["import-xsd", "--xsd"])
        .arg(&xsd)
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let schema: ir::SchemaNode = serde_json::from_slice(&output.stdout)?;
    let elements = schema
        .child(ir::XML_ELEMENTS_FIELD)
        .ok_or_else(|| anyhow::anyhow!("missing element()"))?;
    assert!(matches!(
        elements.xml_wildcard_namespace,
        Some(ir::XmlWildcardNamespaceConstraint::Not { .. })
    ));
    assert!(elements.xml_wildcard_excluded_names.is_some());
    assert!(
        schema
            .child(ir::XML_ATTRIBUTES_FIELD)
            .is_some_and(|attributes| attributes.xml_wildcard_excluded_names.is_some())
    );

    let options = FormatOptions {
        xml_document: true,
        ..FormatOptions::default()
    };
    let project = Project {
        source: schema.clone(),
        target: schema,
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: options.clone(),
        target_options: options,
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        },
    };
    assert!(engine::validate(&project).is_empty());
    let project_path = temp.0.join("project.json");
    // The project file declares the exclusion capability.
    std::fs::write(
        &project_path,
        mapping::project_file::encode_pretty(&project)?,
    )?;
    let input_path = temp.0.join("input.xml");
    let output_path = temp.0.join("output.xml");

    std::fs::write(&input_path, input("<a:Other>o</a:Other>", r#"mode="m""#))?;
    cli::run_project(&project_path, &input_path, &output_path)?;
    let written = std::fs::read_to_string(&output_path)?;
    assert!(
        written.contains("Other") && written.contains("mode=\"m\""),
        "{written}"
    );

    for (content, attributes, expected) in [
        ("<a:Secret/>", "", "excludes element `Secret`"),
        ("", r#"internal="x""#, "excludes attribute `internal`"),
    ] {
        std::fs::write(&input_path, input(content, attributes))?;
        let error = cli::run_project(&project_path, &input_path, &output_path).unwrap_err();
        assert!(format!("{error:#}").contains(expected), "{error:#}");
    }
    Ok(())
}
