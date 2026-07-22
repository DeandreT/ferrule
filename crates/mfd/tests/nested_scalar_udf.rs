use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::Node;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_nested_scalar_udf_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
}

fn setup() -> TempDir {
    let dir = TempDir::new();
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Pairs"><xs:complexType><xs:sequence>
    <xs:element name="Pair" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Left" type="xs:string"/><xs:element name="Right" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Results"><xs:complexType><xs:sequence>
    <xs:element name="Result" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Text" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26">
  <component name="main"><structure><children>
    <component name="Pairs" library="xml" uid="1" kind="14"><data>
      <root><entry name="Pairs"><entry name="Pair" outkey="10"><entry name="Left" outkey="11"/><entry name="Right" outkey="12"/></entry></entry></root>
      <document schema="source.xsd" instanceroot="{}Pairs"/>
    </data></component>
    <component name="CombineWrapped" library="helpers" uid="2" kind="19"><data>
      <root><entry name="Left" inpkey="20" componentid="100"/><entry name="Right" inpkey="21" componentid="101"/></root>
      <root rootindex="1"><entry name="Result" outkey="22" componentid="102"/></root>
    </data></component>
    <component name="Results" library="xml" uid="3" kind="14"><properties XSLTDefaultOutput="1"/><data>
      <root><entry name="Results"><entry name="Result" inpkey="30"><entry name="Text" inpkey="31"/></entry></entry></root>
      <document schema="target.xsd" instanceroot="{}Results"/>
    </data></component>
  </children><graph><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
    <vertex vertexkey="11"><edges><edge vertexkey="20"/></edges></vertex>
    <vertex vertexkey="12"><edges><edge vertexkey="21"/></edges></vertex>
    <vertex vertexkey="22"><edges><edge vertexkey="31"/></edges></vertex>
  </vertices></graph></structure></component>

  <component name="CombineWrapped" library="helpers" inline="1"><structure><children>
    <component name="Left" library="core" uid="100" kind="6"><targets><datapoint pos="0" key="1000"/></targets><data><input datatype="string"/></data></component>
    <component name="Right" library="core" uid="101" kind="6"><targets><datapoint pos="0" key="1001"/></targets><data><input datatype="string"/></data></component>
    <component name="Wrap" library="helpers" uid="103" kind="19"><data>
      <root><entry name="Value" inpkey="1100" componentid="200"/></root>
      <root rootindex="1"><entry name="Result" outkey="1101" componentid="201"/></root>
    </data></component>
    <component name="Wrap" library="helpers" uid="104" kind="19"><data>
      <root><entry name="Value" inpkey="1102" componentid="200"/></root>
      <root rootindex="1"><entry name="Result" outkey="1103" componentid="201"/></root>
    </data></component>
    <component name="concat" library="core" uid="105" kind="5"><sources><datapoint pos="0" key="1200"/><datapoint pos="1" key="1201"/></sources><targets><datapoint pos="0" key="1202"/></targets></component>
    <component name="Result" library="core" uid="102" kind="7"><sources><datapoint pos="0" key="1300"/></sources><data><output datatype="string"/></data></component>
  </children><graph><vertices>
    <vertex vertexkey="1000"><edges><edge vertexkey="1100"/></edges></vertex>
    <vertex vertexkey="1001"><edges><edge vertexkey="1102"/></edges></vertex>
    <vertex vertexkey="1101"><edges><edge vertexkey="1200"/></edges></vertex>
    <vertex vertexkey="1103"><edges><edge vertexkey="1201"/></edges></vertex>
    <vertex vertexkey="1202"><edges><edge vertexkey="1300"/></edges></vertex>
  </vertices></graph></structure></component>

  <component name="Wrap" library="helpers" inline="1"><structure><children>
    <component name="Value" library="core" uid="200" kind="6"><targets><datapoint pos="0" key="2000"/></targets><data><input datatype="string"/></data></component>
    <component name="open" library="core" uid="202" kind="2"><targets><datapoint pos="0" key="2001"/></targets><data><constant value="[" datatype="string"/></data></component>
    <component name="close" library="core" uid="203" kind="2"><targets><datapoint pos="0" key="2002"/></targets><data><constant value="]" datatype="string"/></data></component>
    <component name="concat" library="core" uid="204" kind="5"><sources><datapoint pos="0" key="2003"/><datapoint pos="1" key="2004"/><datapoint pos="2" key="2005"/></sources><targets><datapoint pos="0" key="2006"/></targets></component>
    <component name="uppercase" library="lang" uid="205" kind="5"><sources><datapoint pos="0" key="2008"/></sources><targets><datapoint pos="0" key="2009"/></targets></component>
    <component name="Result" library="core" uid="201" kind="7"><sources><datapoint pos="0" key="2007"/></sources><data><output datatype="string"/></data></component>
  </children><graph><vertices>
    <vertex vertexkey="2000"><edges><edge vertexkey="2008"/></edges></vertex>
    <vertex vertexkey="2001"><edges><edge vertexkey="2003"/></edges></vertex>
    <vertex vertexkey="2002"><edges><edge vertexkey="2005"/></edges></vertex>
    <vertex vertexkey="2009"><edges><edge vertexkey="2004"/></edges></vertex>
    <vertex vertexkey="2006"><edges><edge vertexkey="2007"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#,
    );
    dir
}

#[test]
fn forward_declared_nested_scalar_udfs_preserve_callable_definitions() {
    let dir = setup();
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_eq!(imported.project.user_functions.len(), 2);
    assert_eq!(
        imported
            .project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(node, Node::UserFunctionCall { .. }))
            .count(),
        1
    );
    let combined = imported
        .project
        .user_functions
        .values()
        .find(|function| function.name == "CombineWrapped")
        .unwrap();
    assert_eq!(
        combined
            .body
            .nodes
            .values()
            .filter(|node| matches!(node, Node::Call { function, .. } if function == "concat"))
            .count(),
        3
    );

    let source = format_xml::from_str(
        "<Pairs><Pair><Left>a</Left><Right>b</Right></Pair><Pair><Left>c</Left><Right>d</Right></Pair></Pairs>",
        &imported.project.source,
    )
    .unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    let results = output
        .field("Result")
        .and_then(Instance::as_repeated)
        .unwrap();
    let texts = results
        .iter()
        .map(|result| result.field("Text").and_then(Instance::as_scalar).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        texts,
        [
            &Value::String("[A][B]".to_string()),
            &Value::String("[C][D]".to_string()),
        ]
    );
}

#[test]
fn omitted_scalar_udf_inputs_use_definition_defaults_only_when_unconnected() {
    let dir = TempDir::new();
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Input"><xs:complexType><xs:sequence>
    <xs:element name="Value" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Results"><xs:complexType><xs:sequence>
    <xs:element name="Defaulted" type="xs:boolean"/>
    <xs:element name="Explicit" type="xs:boolean"/>
    <xs:element name="ExplicitNull" type="xs:boolean" minOccurs="0"/>
    <xs:element name="Echo" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26">
  <component name="main"><structure><children>
    <component name="Input" library="xml" uid="1" kind="14"><data>
      <root><entry name="Input"><entry name="Value" outkey="10"/></entry></root>
      <document schema="source.xsd" instanceroot="{}Input"/>
    </data></component>
    <component name="DefaultBoolean" library="helpers" uid="2" kind="19"><data>
      <root><entry name="value" inpkey="19" componentid="100"/></root>
      <root rootindex="1"><entry name="result" outkey="20" componentid="101"/></root>
    </data></component>
    <component name="DefaultBoolean" library="helpers" uid="3" kind="19"><data>
      <root><entry name="value" inpkey="30" componentid="100"/></root>
      <root rootindex="1"><entry name="result" outkey="31" componentid="101"/></root>
    </data></component>
    <component name="constant" library="core" uid="4" kind="2"><targets><datapoint key="40"/></targets><data><constant value="false" datatype="boolean"/></data></component>
    <component name="DefaultBoolean" library="helpers" uid="5" kind="19"><data>
      <root><entry name="value" inpkey="32" componentid="100"/></root>
      <root rootindex="1"><entry name="result" outkey="33" componentid="101"/></root>
    </data></component>
    <component name="constant" library="core" uid="6" kind="2"><targets><datapoint key="42"/></targets><data><constant value="invalid" datatype="boolean"/></data></component>
    <component name="Results" library="xml" uid="7" kind="14"><properties XSLTDefaultOutput="1"/><data>
      <root><entry name="Results"><entry name="Defaulted" inpkey="50"/><entry name="Explicit" inpkey="51"/><entry name="ExplicitNull" inpkey="53"/><entry name="Echo" inpkey="52"/></entry></root>
      <document schema="target.xsd" instanceroot="{}Results"/>
    </data></component>
  </children><graph><vertices>
    <vertex vertexkey="20"><edges><edge vertexkey="50"/></edges></vertex>
    <vertex vertexkey="40"><edges><edge vertexkey="30"/></edges></vertex>
    <vertex vertexkey="31"><edges><edge vertexkey="51"/></edges></vertex>
    <vertex vertexkey="42"><edges><edge vertexkey="32"/></edges></vertex>
    <vertex vertexkey="33"><edges><edge vertexkey="53"/></edges></vertex>
    <vertex vertexkey="10"><edges><edge vertexkey="52"/></edges></vertex>
  </vertices></graph></structure></component>

  <component name="DefaultBoolean" library="helpers" inline="1"><structure><children>
    <component name="value" library="core" uid="100" kind="6">
      <sources><datapoint pos="0" key="1000"/></sources>
      <targets><datapoint pos="0" key="1001"/></targets>
      <data><input datatype="boolean"/><parameter usageKind="input" name="value" optional="1"/></data>
    </component>
    <component name="true" library="core" uid="102" kind="2"><targets><datapoint key="1002"/></targets><data><constant value="true" datatype="anySimpleType"/></data></component>
    <component name="result" library="core" uid="101" kind="7"><sources><datapoint key="1003"/></sources><data><output datatype="boolean"/></data></component>
  </children><graph><vertices>
    <vertex vertexkey="1002"><edges><edge vertexkey="1000"/></edges></vertex>
    <vertex vertexkey="1001"><edges><edge vertexkey="1003"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let source = format_xml::from_str(
        "<Input><Value>kept</Value></Input>",
        &imported.project.source,
    )
    .unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    assert_eq!(
        output.field("Defaulted").and_then(Instance::as_scalar),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        output.field("Explicit").and_then(Instance::as_scalar),
        Some(&Value::Bool(false))
    );
    assert_eq!(
        output.field("ExplicitNull").and_then(Instance::as_scalar),
        Some(&Value::Null)
    );
}
