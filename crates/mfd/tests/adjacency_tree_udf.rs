use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::ScopeConstruction;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_adjacency_tree_{}_{}",
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

fn write(path: &Path, value: &str) -> Result<(), std::io::Error> {
    std::fs::write(path, value)
}

#[test]
fn imports_and_executes_recursive_adjacency_udf() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    write(
        &dir.0.join("catalog.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="catalog"><xs:complexType><xs:sequence><xs:element name="type" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:attribute name="name" type="xs:string" use="required"/><xs:attribute name="base" type="xs:string"/></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    write(
        &dir.0.join("tree.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="type"><xs:complexType><xs:sequence><xs:element ref="type" minOccurs="0" maxOccurs="unbounded"/></xs:sequence><xs:attribute name="name" type="xs:string" use="required"/></xs:complexType></xs:element></xs:schema>"#,
    )?;
    write(
        &dir.0.join("catalog.xml"),
        r#"<catalog><type name="Root"/><type name="Beta" base="Root"/><type name="Alpha" base="Root"/><type name="Leaf" base="Beta"/><type name="Detached" base="Detached"/></catalog>"#,
    )?;
    write(&dir.0.join("mapping.mfd"), MAPPING)?;

    let imported = mfd::import(&dir.0.join("mapping.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(matches!(
        imported.project.root.construction,
        ScopeConstruction::AdjacencyTree { .. }
    ));
    assert!(engine::validate(&imported.project).is_empty());

    let input = format_xml::read(&dir.0.join("catalog.xml"), &imported.project.source)?;
    let output = engine::run(&imported.project, &input)?;
    assert_eq!(string(&output, "name"), Some("Root"));
    let children = repeated(&output, "type").ok_or("missing root children")?;
    assert_eq!(children.len(), 2);
    assert_eq!(string(&children[0], "name"), Some("Beta"));
    assert_eq!(string(&children[1], "name"), Some("Alpha"));
    let grandchildren = repeated(&children[0], "type").ok_or("missing Beta children")?;
    assert_eq!(grandchildren.len(), 1);
    assert_eq!(string(&grandchildren[0], "name"), Some("Leaf"));

    let export = dir.0.join("roundtrip.mfd");
    assert!(mfd::export(&imported.project, &export)?.is_empty());
    let reimported = mfd::import(&export)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(matches!(
        reimported.project.root.construction,
        ScopeConstruction::AdjacencyTree { .. }
    ));
    assert!(engine::validate(&reimported.project).is_empty());
    assert_eq!(engine::run(&reimported.project, &input)?, output);
    Ok(())
}

fn string<'a>(instance: &'a Instance, field: &str) -> Option<&'a str> {
    match instance.field(field) {
        Some(Instance::Scalar(Value::String(value))) => Some(value),
        _ => None,
    }
}

fn repeated<'a>(instance: &'a Instance, field: &str) -> Option<&'a [Instance]> {
    instance.field(field).and_then(Instance::as_repeated)
}

const MAPPING: &str = r#"<mapping version="26">
<component name="map"><structure><children>
  <component name="source" library="xml" uid="1" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="catalog" outkey="101"/></entry></entry></root><document schema="catalog.xsd" inputinstance="catalog.xml" instanceroot="{}catalog"/></data></component>
  <component name="target" library="xml" uid="2" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="type" inpkey="201"/></entry></entry></root><document schema="tree.xsd" instanceroot="{}type"/></data></component>
  <component name="BuildTree" library="user" uid="30" kind="19"><data>
    <root><entry name="catalog" componentid="10" inpkey="301"/></root>
    <root><entry name="base" componentid="12"/></root>
    <root rootindex="2"><entry name="type" componentid="11" outkey="302"/></root>
  </data></component>
</children><graph><edges><edge edgekey="1"><data><dataconnection type="2"/></data></edge><edge edgekey="2"><data><dataconnection type="2"/></data></edge></edges><vertices>
  <vertex vertexkey="101"><edges><edge vertexkey="301" edgekey="1"/></edges></vertex>
  <vertex vertexkey="302"><edges><edge vertexkey="201" edgekey="2"/></edges></vertex>
</vertices></graph></structure></component>
<component name="BuildTree" library="user" uid="30"><structure><children>
  <component name="catalog" library="xml" uid="10" kind="14"><data><root><entry name="catalog" outkey="401"><entry name="type" outkey="402"><entry name="name" type="attribute" outkey="403"/><entry name="base" type="attribute" outkey="404"/></entry></entry></root><document schema="catalog.xsd" instanceroot="{}catalog"/><parameter usageKind="input"/></data></component>
  <component name="type" library="xml" uid="11" kind="14"><data><root><entry name="type" inpkey="501"><entry name="name" type="attribute" inpkey="502"/><entry name="type" inpkey="503"/></entry></root><document schema="tree.xsd" instanceroot="{}type"/><parameter usageKind="output"/></data></component>
  <component name="base" library="core" uid="12" kind="6"><targets><datapoint pos="0" key="601"/></targets></component>
  <component name="BuildTree" library="user" uid="31" kind="19"><data><root><entry name="catalog" componentid="10" inpkey="701"/></root><root><entry name="base" componentid="12" inpkey="702"/></root><root rootindex="2"><entry name="type" componentid="11" outkey="703"/></root></data></component>
  <component name="exists" library="core" uid="20" kind="5"><sources><datapoint pos="0" key="711"/></sources><targets><datapoint pos="0" key="712"/></targets></component>
  <component name="equal" library="core" uid="21" kind="5"><sources><datapoint pos="0" key="721"/><datapoint pos="1" key="722"/></sources><targets><datapoint pos="0" key="723"/></targets></component>
  <component name="not-exists" library="core" uid="22" kind="5"><sources><datapoint pos="0" key="731"/></sources><targets><datapoint pos="0" key="732"/></targets></component>
  <component name="if-else" library="core" uid="23" kind="4"><sources><datapoint pos="0" key="741"/><datapoint pos="1" key="742"/><datapoint pos="2" key="743"/></sources><targets><datapoint pos="0" key="744"/></targets></component>
  <component name="type" library="core" uid="24" kind="3"><sources><datapoint pos="0" key="751"/><datapoint pos="1" key="752"/></sources><targets><datapoint pos="0" key="753"/></targets></component>
</children><graph><edges><edge edgekey="3"><data><dataconnection type="2"/></data></edge><edge edgekey="4"><data><dataconnection type="2"/></data></edge><edge edgekey="5"><data><dataconnection type="2"/></data></edge></edges><vertices>
  <vertex vertexkey="401"><edges><edge vertexkey="701" edgekey="3"/></edges></vertex>
  <vertex vertexkey="403"><edges><edge vertexkey="702"/><edge vertexkey="502"/></edges></vertex>
  <vertex vertexkey="703"><edges><edge vertexkey="503" edgekey="4"/></edges></vertex>
  <vertex vertexkey="402"><edges><edge vertexkey="751"/></edges></vertex>
  <vertex vertexkey="601"><edges><edge vertexkey="711"/><edge vertexkey="721"/></edges></vertex>
  <vertex vertexkey="404"><edges><edge vertexkey="722"/><edge vertexkey="731"/></edges></vertex>
  <vertex vertexkey="712"><edges><edge vertexkey="741"/></edges></vertex>
  <vertex vertexkey="723"><edges><edge vertexkey="742"/></edges></vertex>
  <vertex vertexkey="732"><edges><edge vertexkey="743"/></edges></vertex>
  <vertex vertexkey="744"><edges><edge vertexkey="752"/></edges></vertex>
  <vertex vertexkey="753"><edges><edge vertexkey="501" edgekey="5"/></edges></vertex>
</vertices></graph></structure></component>
</mapping>"#;
