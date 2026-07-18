use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value, XML_TEXT_FIELD};
use mapping::Node;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_dynamic_xml_lookup_{}_{}",
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

fn write_fixture(dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    std::fs::write(
        dir.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Rows"><xs:complexType><xs:sequence>
    <xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Entry" maxOccurs="unbounded"><xs:complexType><xs:simpleContent>
        <xs:extension base="xs:string"><xs:attribute name="Code" type="xs:string" use="required"/></xs:extension>
      </xs:simpleContent></xs:complexType></xs:element>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.join("profile.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Profiles"><xs:complexType><xs:sequence>
    <xs:element name="Profile" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Given" type="xs:string"/><xs:element name="Family" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Summary"><xs:complexType><xs:sequence>
    <xs:element name="Names" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;

    let design = dir.join("mapping.mfd");
    std::fs::write(
        &design,
        r##"<mapping version="29"><resources/><component name="map"><structure><children>
  <component name="Rows" library="xml" kind="14"><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="Rows">
      <entry name="Row" outkey="1"><entry name="Entry" outkey="2">
        <entry name="Code" type="attribute" outkey="3"/>
      </entry></entry>
    </entry></entry></entry></root>
    <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Rows"/>
  </data></component>
  <component name="profile" library="xml" kind="14"><data>
    <root><entry name="document"><entry name="Profile" inpkey="11" use-generic-elements="1">
      <entry name="element()" inpkey="12"><entry name="LocalName" inpkey="13"/>
        <entry name="#text" type="xml-type" inpkey="14"/>
      </entry>
      <entry name="Given" outkey="15"/><entry name="Family" outkey="16"/>
    </entry></entry></root>
    <document schema="profile.xsd"/>
    <parameter usageKind="variable"><root><entry name="Profiles"/><entry name="Profile"/></root></parameter>
  </data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="50"/></targets>
    <data><constant value=" " datatype="string"/></data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="51"/></targets>
    <data><constant value=", " datatype="string"/></data></component>
  <component name="concat" library="core" kind="5" growable="1">
    <sources><datapoint pos="0" key="60"/><datapoint pos="1" key="61"/><datapoint pos="2" key="62"/></sources>
    <targets><datapoint pos="0" key="63"/></targets>
  </component>
  <component name="string-join" library="core" kind="5">
    <sources><datapoint/><datapoint pos="1" key="70"/><datapoint pos="2" key="71"/></sources>
    <targets><datapoint pos="0" key="72"/></targets>
  </component>
  <component name="Summary" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="Summary"><entry name="Names" inpkey="80"/></entry></entry></entry></root>
    <document schema="target.xsd" instanceroot="{}Summary"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="1"><edges><edge vertexkey="11"/></edges></vertex>
  <vertex vertexkey="2"><edges><edge vertexkey="12"/><edge vertexkey="14"/></edges></vertex>
  <vertex vertexkey="3"><edges><edge vertexkey="13"/></edges></vertex>
  <vertex vertexkey="15"><edges><edge vertexkey="60"/></edges></vertex>
  <vertex vertexkey="50"><edges><edge vertexkey="61"/></edges></vertex>
  <vertex vertexkey="16"><edges><edge vertexkey="62"/></edges></vertex>
  <vertex vertexkey="63"><edges><edge vertexkey="70"/></edges></vertex>
  <vertex vertexkey="51"><edges><edge vertexkey="71"/></edges></vertex>
  <vertex vertexkey="72"><edges><edge vertexkey="80"/></edges></vertex>
</vertices></graph></structure></component></mapping>"##,
    )?;
    Ok(design)
}

#[test]
fn generic_elements_pivot_key_value_rows_into_variable_fields() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let mut matches = imported
        .project
        .graph
        .nodes
        .values()
        .filter_map(|node| {
            let Node::Lookup {
                collection,
                key,
                matches,
                value,
            } = node
            else {
                return None;
            };
            assert_eq!(collection, &["Entry"]);
            assert_eq!(key, &["Code"]);
            assert_eq!(value, &[XML_TEXT_FIELD]);
            let Node::Const {
                value: Value::String(name),
            } = imported.project.graph.nodes.get(matches)?
            else {
                return None;
            };
            Some(name.as_str())
        })
        .collect::<Vec<_>>();
    matches.sort_unstable();
    assert_eq!(matches, ["Family", "Given"]);

    let source = format_xml::from_str(
        r#"<Rows>
          <Row><Entry Code="Family">Lovelace</Entry><Entry Code="Given">Ada</Entry></Row>
          <Row><Entry Code="Given">Grace</Entry><Entry Code="Family">Hopper</Entry></Row>
        </Rows>"#,
        &imported.project.source,
    )?;
    let target = engine::run(&imported.project, &source)?;
    assert_eq!(
        target.field("Names").and_then(Instance::as_scalar),
        Some(&Value::String("Ada Lovelace, Grace Hopper".into()))
    );

    let exported_path = dir.0.join("exported.mfd");
    let export_warnings = mfd::export(&imported.project, &exported_path)?;
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    let roundtripped = mfd::import(&exported_path)?;
    assert!(
        roundtripped.warnings.is_empty(),
        "{:?}",
        roundtripped.warnings
    );
    let validation = engine::validate(&roundtripped.project);
    assert!(validation.is_empty(), "{validation:?}");
    assert_eq!(
        roundtripped
            .project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(node, Node::Lookup { .. }))
            .count(),
        2
    );
    assert!(roundtripped.project.graph.nodes.values().any(|node| {
        matches!(
            node,
            Node::Aggregate { collection, .. } if collection == &["Row"]
        )
    }));
    assert_eq!(engine::run(&roundtripped.project, &source)?, target);
    Ok(())
}
