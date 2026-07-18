use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::Node;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_mixed_content_{}_{}",
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

fn write_fixture(dir: &Path) -> Result<PathBuf, std::io::Error> {
    std::fs::write(
        dir.join("library.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:complexType name="SectionType" mixed="true"><xs:choice minOccurs="0" maxOccurs="unbounded">
    <xs:element name="Strong" type="xs:string"/>
    <xs:element name="Em" type="xs:string"/>
    <xs:element name="Subsection" type="SectionType"/>
  </xs:choice></xs:complexType>
  <xs:element name="Library"><xs:complexType><xs:sequence>
    <xs:element name="Article" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Title" type="xs:string"/>
      <xs:element name="Body" type="SectionType"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.join("fragment.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Fragment"><xs:complexType mixed="true"/></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.join("digest.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Digest"><xs:complexType><xs:sequence>
    <xs:element name="Row" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Title" type="xs:string"/>
      <xs:element name="Markup" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.join("library.xml"),
        "<Library><Article><Title>First</Title><Body>Read <Strong>carefully</Strong>, then <Em>compare</Em> and <Em>revise</Em>.</Body></Article><Article><Title>Second</Title><Body><Strong>Ship</Strong> only when <Em>ready</Em>.</Body></Article></Library>",
    )?;

    let design = dir.join("mixed-content.mfd");
    std::fs::write(
        &design,
        r##"<mapping version="26"><component name="map"><structure><children>
  <component name="library" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Library"><entry name="Article" outkey="1"><entry name="Title" outkey="2"/><entry name="Body" outkey="3"><entry name="#text" outkey="4"/><entry name="Strong" outkey="5"/><entry name="Em" outkey="6"/></entry></entry></entry></entry></entry></root><document schema="library.xsd" inputinstance="library.xml" instanceroot="{}Library"/></data></component>
  <component name="fragment" library="xml" kind="14"><data><root><entry name="document"><entry name="Fragment" inpkey="10" outkey="11"><entry name="#text" inpkey="12"/><entry name="#text" inpkey="13" clone="1"/><entry name="#text" inpkey="14" clone="1"/></entry></entry></root><document schema="fragment.xsd" instanceroot="{}Fragment"/><parameter usageKind="variable"/></data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="20"/></targets><data><constant value="&lt;strong&gt;" datatype="string"/></data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="21"/></targets><data><constant value="&lt;/strong&gt;" datatype="string"/></data></component>
  <component name="concat" library="core" kind="5"><sources><datapoint pos="0" key="30"/><datapoint pos="1" key="31"/><datapoint pos="2" key="32"/></sources><targets><datapoint pos="0" key="33"/></targets></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="22"/></targets><data><constant value="&lt;em&gt;" datatype="string"/></data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="23"/></targets><data><constant value="&lt;/em&gt;" datatype="string"/></data></component>
  <component name="concat" library="core" kind="5"><sources><datapoint pos="0" key="34"/><datapoint pos="1" key="35"/><datapoint pos="2" key="36"/></sources><targets><datapoint pos="0" key="37"/></targets></component>
  <component name="digest" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Digest"><entry name="Row" inpkey="50"><entry name="Title" inpkey="51"/><entry name="Markup" inpkey="52"/></entry></entry></entry></entry></root><document schema="digest.xsd" outputinstance="digest.xml" instanceroot="{}Digest"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="1"><edges><edge vertexkey="50"/></edges></vertex>
  <vertex vertexkey="2"><edges><edge vertexkey="51"/></edges></vertex>
  <vertex vertexkey="3"><edges><edge vertexkey="10"/></edges></vertex>
  <vertex vertexkey="4"><edges><edge vertexkey="12"/></edges></vertex>
  <vertex vertexkey="20"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="5"><edges><edge vertexkey="31"/></edges></vertex>
  <vertex vertexkey="21"><edges><edge vertexkey="32"/></edges></vertex>
  <vertex vertexkey="33"><edges><edge vertexkey="13"/></edges></vertex>
  <vertex vertexkey="22"><edges><edge vertexkey="34"/></edges></vertex>
  <vertex vertexkey="6"><edges><edge vertexkey="35"/></edges></vertex>
  <vertex vertexkey="23"><edges><edge vertexkey="36"/></edges></vertex>
  <vertex vertexkey="37"><edges><edge vertexkey="14"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="52"/></edges></vertex>
</vertices></graph></structure></component></mapping>"##,
    )?;
    Ok(design)
}

#[test]
fn recursive_mixed_content_replacements_preserve_document_order_and_values()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let design = write_fixture(&dir.0)?;
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(imported.project.graph.nodes.values().any(|node| matches!(
        node,
        Node::XmlMixedContent { replacements, .. } if replacements.len() == 2
    )));

    let source = format_xml::read(&dir.0.join("library.xml"), &imported.project.source)?;
    assert_eq!(
        source
            .field("Article")
            .and_then(Instance::as_repeated)
            .and_then(|articles| articles.first())
            .and_then(|article| article.field("Body"))
            .and_then(|body| body.field(ir::XML_TEXT_FIELD))
            .and_then(Instance::as_scalar),
        Some(&Value::String("Read , then  and .".into()))
    );
    let output = engine::run(&imported.project, &source)?;
    let rows = output
        .field("Row")
        .and_then(Instance::as_repeated)
        .ok_or_else(|| std::io::Error::other("digest rows were not produced"))?;
    let markup = rows
        .iter()
        .map(|row| row.field("Markup").and_then(Instance::as_scalar))
        .collect::<Vec<_>>();
    assert_eq!(
        markup,
        vec![
            Some(&Value::String(
                "Read <strong>carefully</strong>, then <em>compare</em> and <em>revise</em>."
                    .into()
            )),
            Some(&Value::String(
                "<strong>Ship</strong> only when <em>ready</em>.".into()
            )),
        ]
    );

    let exported = dir.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &exported)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let roundtrip = mfd::import(&exported)?;
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(
        roundtrip.project.graph.nodes.values().any(|node| matches!(
            node,
            Node::XmlMixedContent { replacements, .. } if replacements.len() == 2
        )),
        "{:?}",
        roundtrip.project.graph.nodes
    );
    Ok(())
}
