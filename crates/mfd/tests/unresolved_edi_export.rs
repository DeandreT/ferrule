use std::collections::BTreeSet;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use mapping::{Node, Project, Scope};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_unresolved_edi_export_{}_{}",
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

#[test]
fn unresolved_edi_record_parameter_ports_survive_export_and_reimport() -> Result<(), Box<dyn Error>>
{
    let directory = TempDir::new()?;
    write_fixture(&directory.0)?;
    let imported = mfd::import(&directory.0.join("mapping.mfd"))?;

    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("could not compile external configuration"));
    assert_eq!(imported.project.runtime_dependencies().len(), 1);
    assert_eq!(
        imported
            .project
            .source_options
            .edi_config_reference
            .as_ref()
            .and_then(mapping::EdiConfigDependency::reference),
        Some("Unavailable/Envelope.Config")
    );
    for path in [
        ["Batch", "Line", "Detail", "Code"],
        ["Batch", "Line", "Detail", "Quantity"],
    ] {
        assert!(
            schema_at(&imported.project.source, &path).is_some_and(|node| {
                matches!(
                    &node.kind,
                    ir::SchemaKind::Scalar {
                        ty: ir::ScalarType::String
                    }
                ) && !node.repeating
            })
        );
    }
    assert_eq!(
        target_bindings(&imported.project.root),
        BTreeSet::from(["Code".to_string(), "Quantity".to_string()])
    );
    let expected_source_fields = source_fields(&imported.project);

    let roundtrip = directory.0.join("roundtrip.mfd");
    assert!(
        mfd::export(&imported.project, &roundtrip)?.is_empty(),
        "the enriched opaque ports must make every connection exportable"
    );
    assert!(
        std::fs::read_to_string(&roundtrip)?.contains("ferrule-unresolved-config=\"1\""),
        "the canonical design must retain typed unresolved-resource provenance"
    );

    let reimported = mfd::import(&roundtrip)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert_eq!(reimported.project.runtime_dependencies().len(), 1);
    assert_eq!(
        reimported
            .project
            .source_options
            .edi_config_reference
            .as_ref()
            .and_then(mapping::EdiConfigDependency::reference),
        Some("Unavailable/Envelope.Config")
    );
    assert_eq!(source_fields(&reimported.project), expected_source_fields);
    assert_eq!(
        target_bindings(&reimported.project.root),
        BTreeSet::from(["Code".to_string(), "Quantity".to_string()])
    );
    assert!(
        mfd::export(
            &reimported.project,
            &directory.0.join("second-roundtrip.mfd")
        )?
        .is_empty()
    );
    Ok(())
}

#[test]
fn malformed_unresolved_config_metadata_warns_and_resolves_normally() -> Result<(), Box<dyn Error>>
{
    let directory = TempDir::new()?;
    write_fixture(&directory.0)?;
    let path = directory.0.join("mapping.mfd");
    let design = std::fs::read_to_string(&path)?.replace(
        "config=\"Unavailable/Envelope.Config\"",
        "config=\"Unavailable/Envelope.Config\" ferrule-unresolved-config=\"maybe\"",
    );
    std::fs::write(&path, design)?;

    let imported = mfd::import(&path)?;
    assert_eq!(imported.warnings.len(), 2, "{:?}", imported.warnings);
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid ferrule-unresolved-config metadata"))
    );
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("could not compile external configuration"))
    );
    assert_eq!(imported.project.runtime_dependencies().len(), 1);
    Ok(())
}

#[test]
fn missing_edi_configuration_metadata_roundtrips_as_a_typed_dependency()
-> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    write_fixture(&directory.0)?;
    let design_path = directory.0.join("mapping.mfd");
    let design = std::fs::read_to_string(&design_path)?
        .replace(r#" config="Unavailable/Envelope.Config""#, "");
    std::fs::write(&design_path, design)?;

    let imported = mfd::import(&design_path)?;
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("entry-tree schema inferred"));
    assert!(matches!(
        imported
            .project
            .source_options
            .edi_config_reference
            .as_ref(),
        Some(mapping::EdiConfigDependency::MissingConfiguration)
    ));
    assert_eq!(imported.project.runtime_dependencies().len(), 1);
    assert!(engine::validate(&imported.project).is_empty());

    let roundtrip = directory.0.join("missing-roundtrip.mfd");
    assert!(mfd::export(&imported.project, &roundtrip)?.is_empty());
    let exported = std::fs::read_to_string(&roundtrip)?;
    let document = roxmltree::Document::parse(&exported)?;
    let text = document
        .descendants()
        .find(|node| node.has_tag_name("text") && node.attribute("type") == Some("edi"))
        .ok_or("exported mapping has no EDI component")?;
    assert_eq!(text.attribute("config"), None);
    assert_eq!(text.attribute("ferrule-missing-config"), Some("1"));
    assert_eq!(text.attribute("ferrule-unresolved-config"), None);

    let reimported = mfd::import(&roundtrip)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(matches!(
        reimported
            .project
            .source_options
            .edi_config_reference
            .as_ref(),
        Some(mapping::EdiConfigDependency::MissingConfiguration)
    ));
    assert_eq!(reimported.project.runtime_dependencies().len(), 1);
    assert!(engine::validate(&reimported.project).is_empty());
    Ok(())
}

fn write_fixture(directory: &Path) -> Result<(), std::io::Error> {
    std::fs::write(
        directory.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Target"><xs:complexType><xs:sequence>
    <xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Code" type="xs:string"/>
      <xs:element name="Quantity" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        directory.join("mapping.mfd"),
        r#"<mapping version="26"><resources/><component name="map"><structure><children>
  <component name="orders" library="text" kind="16"><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="Envelope">
      <entry name="Batch"><entry name="Line" outkey="10"><entry name="Detail"/></entry></entry>
    </entry></entry></entry></root>
    <text type="edi" kind="EDIFACT" config="Unavailable/Envelope.Config" inputinstance="orders.edi"/>
  </data></component>
  <component name="ProjectLine" library="user" kind="19"><data>
    <root><entry name="Line" inpkey="30" componentid="101"/></root>
    <root rootindex="1"><entry name="Row" outkey="40" componentid="102">
      <entry name="Code" outkey="41"/><entry name="Quantity" outkey="42"/>
    </entry></root>
  </data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Target"><entry name="Row" inpkey="20">
      <entry name="Code" inpkey="21"/><entry name="Quantity" inpkey="22"/>
    </entry></entry></root>
    <document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="40"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="41"><edges><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="42"><edges><edge vertexkey="22"/></edges></vertex>
</vertices></graph></structure></component>
<component name="ProjectLine" library="user" editable="1"><structure><children>
  <component name="Line" library="text" uid="101" kind="16"><properties UsageKind="input"/><data>
    <root><entry name="document"><entry name="Line" outkey="201"><entry name="Detail">
      <entry name="Code" outkey="202"/><entry name="Quantity" outkey="203"/>
    </entry></entry></entry></root>
    <text type="edi" kind="EDIFACT" config="Unavailable/Envelope.Config"/>
    <parameter usageKind="input" name="Lines" sequence="1"><root>
      <entry name="Envelope"/><entry name="Batch"/><entry name="Line"/>
    </root></parameter>
  </data></component>
  <component name="Row" library="xml" uid="102" kind="14"><properties UsageKind="output"/><data>
    <root><entry name="Row" inpkey="212">
      <entry name="Code" inpkey="213"/><entry name="Quantity" inpkey="214"/>
    </entry></root>
    <document schema="target.xsd" instanceroot="{}Target/{}Row"/>
    <parameter usageKind="output" name="Rows" sequence="1"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="201"><edges><edge vertexkey="212"/></edges></vertex>
  <vertex vertexkey="202"><edges><edge vertexkey="213"/></edges></vertex>
  <vertex vertexkey="203"><edges><edge vertexkey="214"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )
}

fn schema_at<'a>(schema: &'a ir::SchemaNode, path: &[&str]) -> Option<&'a ir::SchemaNode> {
    path.iter()
        .try_fold(schema, |node, segment| node.child(segment))
}

fn source_fields(project: &Project) -> BTreeSet<(Option<Vec<String>>, Vec<String>)> {
    project
        .graph
        .nodes
        .values()
        .filter_map(|node| match node {
            Node::SourceField { path, frame } => Some((frame.clone(), path.clone())),
            _ => None,
        })
        .collect()
}

fn target_bindings(scope: &Scope) -> BTreeSet<String> {
    let mut fields = scope
        .bindings
        .iter()
        .map(|binding| binding.target_field.clone())
        .collect::<BTreeSet<_>>();
    for child in &scope.children {
        fields.extend(target_bindings(child));
    }
    fields
}
