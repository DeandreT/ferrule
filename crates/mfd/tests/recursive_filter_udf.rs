use std::fs;
use std::path::PathBuf;

use mapping::ScopeConstruction;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-mfd-recursive-filter-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn recursive_udf_filters_each_level_and_preserves_the_group_shape() {
    let dir = TempDir::new();
    fs::write(dir.0.join("folder.xsd"), schema()).unwrap();
    fs::write(dir.0.join("mapping.mfd"), mapping()).unwrap();

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(matches!(
        imported.project.root.construction,
        ScopeConstruction::RecursiveFilter { .. }
    ));
    let issues = engine::validate(&imported.project);
    assert!(issues.is_empty(), "{issues:?}");
    let source = format_xml::from_str(
        r#"<Folder label="root"><Item code="root.keep"/><Item code="drop.txt"/><Folder label="child"><Item code="nested.keep"/><Item code="other"/></Folder></Folder>"#,
        &imported.project.source,
    )
    .unwrap();
    let expected = format_xml::from_str(
        r#"<Folder label="root"><Item code="root.keep"/><Folder label="child"><Item code="nested.keep"/></Folder></Folder>"#,
        &imported.project.target,
    )
    .unwrap();

    let initial = engine::run(&imported.project, &source).unwrap();
    assert_eq!(initial, expected);

    let export_path = dir.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &export_path).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&export_path).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(matches!(
        reimported.project.root.construction,
        ScopeConstruction::RecursiveFilter { .. }
    ));
    assert!(engine::validate(&reimported.project).is_empty());
    assert_eq!(engine::run(&reimported.project, &source), Ok(initial));
}

fn schema() -> &'static str {
    r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Folder">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="Item" minOccurs="0" maxOccurs="unbounded">
          <xs:complexType><xs:attribute name="code" type="xs:string"/></xs:complexType>
        </xs:element>
        <xs:element ref="Folder" minOccurs="0" maxOccurs="unbounded"/>
      </xs:sequence>
      <xs:attribute name="label" type="xs:string"/>
    </xs:complexType>
  </xs:element>
</xs:schema>"#
}

fn mapping() -> &'static str {
    r#"<mapping version="26">
  <component name="main"><structure><children>
    <component name="source" library="xml" kind="14"><data><root>
      <entry name="Folder" outkey="100"><entry name="label" type="attribute" outkey="101"/><entry name="Item" outkey="102"/><entry name="Folder" outkey="103"/></entry>
    </root><document schema="folder.xsd" instanceroot="{}Folder"/></data></component>
    <component name="constant" library="core" uid="2" kind="2"><targets><datapoint key="110"/></targets><data><constant value=".keep" datatype="string"/></data></component>
    <component name="KeepItems" library="custom" uid="3" kind="19"><data>
      <root><entry name="input" componentid="10"><entry name="Folder" inpkey="200"/></entry><entry name="suffix" componentid="14" inpkey="201"/></root>
      <root rootindex="1"><entry name="output" componentid="11"><entry name="Folder" outkey="300"/></entry></root>
    </data></component>
    <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root>
      <entry name="Folder" inpkey="400"><entry name="label" type="attribute" inpkey="401"/><entry name="Item" inpkey="402"/><entry name="Folder" inpkey="403"/></entry>
    </root><document schema="folder.xsd" instanceroot="{}Folder"/></data></component>
  </children><graph><vertices>
    <vertex vertexkey="100"><edges><edge vertexkey="200"/></edges></vertex>
    <vertex vertexkey="110"><edges><edge vertexkey="201"/></edges></vertex>
    <vertex vertexkey="300"><edges><edge vertexkey="400"/></edges></vertex>
  </vertices></graph></structure></component>

  <component name="KeepItems" library="custom"><structure><children>
    <component name="output" library="xml" uid="11" kind="14"><properties UsageKind="output"/><data><root>
      <entry name="Folder" inpkey="600"><entry name="label" type="attribute" inpkey="601"/><entry name="Item" inpkey="602"/><entry name="Folder" inpkey="603"/></entry>
    </root><document schema="folder.xsd" instanceroot="{}Folder"/><parameter usageKind="output" name="output"/></data></component>
    <component name="input" library="xml" uid="10" kind="14"><properties UsageKind="input"/><data><root>
      <entry name="Folder" outkey="500"><entry name="label" type="attribute" outkey="501"/><entry name="Item" outkey="502"><entry name="code" type="attribute" outkey="503"/></entry><entry name="Folder" outkey="504"/></entry>
    </root><document schema="folder.xsd" instanceroot="{}Folder"/><parameter usageKind="input" name="input"/></data></component>
    <component name="select" library="core" uid="12" kind="3"><sources><datapoint pos="0" key="610"/><datapoint pos="1" key="611"/></sources><targets><datapoint pos="0" key="612"/><datapoint/></targets></component>
    <component name="contains" library="core" uid="13" kind="5"><sources><datapoint pos="0" key="620"/><datapoint pos="1" key="621"/></sources><targets><datapoint pos="0" key="622"/></targets></component>
    <component name="suffix" library="core" uid="14" kind="6"><targets><datapoint pos="0" key="630"/></targets><data><parameter usageKind="input" name="suffix"/></data></component>
    <component name="KeepItems" library="custom" uid="15" kind="19"><data>
      <root><entry name="input" componentid="10"><entry name="Folder"><entry name="Folder" inpkey="640"/></entry></entry><entry name="suffix" componentid="14" inpkey="641"/></root>
      <root rootindex="1"><entry name="output" componentid="11"><entry name="Folder"><entry name="Folder" outkey="650"/></entry></entry></root>
    </data></component>
  </children><graph><vertices>
    <vertex vertexkey="500"><edges><edge vertexkey="600"/></edges></vertex>
    <vertex vertexkey="501"><edges><edge vertexkey="601"/></edges></vertex>
    <vertex vertexkey="502"><edges><edge vertexkey="610"/></edges></vertex>
    <vertex vertexkey="503"><edges><edge vertexkey="620"/></edges></vertex>
    <vertex vertexkey="630"><edges><edge vertexkey="621"/><edge vertexkey="641"/></edges></vertex>
    <vertex vertexkey="622"><edges><edge vertexkey="611"/></edges></vertex>
    <vertex vertexkey="612"><edges><edge vertexkey="602"/></edges></vertex>
    <vertex vertexkey="504"><edges><edge vertexkey="640"/></edges></vertex>
    <vertex vertexkey="650"><edges><edge vertexkey="603"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#
}
