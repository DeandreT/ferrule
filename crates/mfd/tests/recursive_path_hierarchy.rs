use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::ScopeConstruction;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_recursive_path_hierarchy_{}_{}",
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

fn scalar<'a>(instance: &'a Instance, field: &str) -> Option<&'a str> {
    match instance.field(field) {
        Some(Instance::Scalar(Value::String(value))) => Some(value),
        _ => None,
    }
}

#[test]
fn imports_and_executes_bounded_recursive_path_grouping() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = TempDir::new()?;
    write(
        &dir.0.join("paths.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Paths"><xs:complexType><xs:sequence><xs:element name="Path" type="xs:string" minOccurs="0" maxOccurs="unbounded"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    write(
        &dir.0.join("directory.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="directory"><xs:complexType><xs:choice minOccurs="0" maxOccurs="unbounded"><xs:element name="file"><xs:complexType><xs:attribute name="name" type="xs:string"/></xs:complexType></xs:element><xs:element ref="directory"/></xs:choice><xs:attribute name="name" type="xs:string"/></xs:complexType></xs:element></xs:schema>"#,
    )?;
    write(
        &dir.0.join("paths.xml"),
        "<Paths><Path>Project\\README.md</Path><Path>Project\\src\\main.rs</Path><Path>Project\\src\\lib.rs</Path><Path>Project\\tests\\smoke.rs</Path></Paths>",
    )?;
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26">
<component name="map"><structure><children>
  <component name="source" library="xml" uid="1" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Paths" outkey="101"/></entry></entry></root><document schema="paths.xsd" inputinstance="paths.xml" instanceroot="{}Paths"/></data></component>
  <component name="target" library="xml" uid="2" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="directory" inpkey="201"/></entry></entry></root><document schema="directory.xsd" outputinstance="directory.xml" instanceroot="{}directory"/></data></component>
  <component name="BuildTree" library="user" uid="30" kind="19"><data>
    <root><entry name="Paths" componentid="10"><entry name="Paths" inpkey="301"/></entry></root>
    <root rootindex="1"><entry name="directory" componentid="11"><entry name="directory"><entry name="directory" outkey="302"/></entry></entry></root>
  </data></component>
</children><graph><edges><edge edgekey="901"><data><dataconnection type="2"/></data></edge><edge edgekey="902"><data><dataconnection type="2"/></data></edge></edges><vertices>
  <vertex vertexkey="101"><edges><edge vertexkey="301" edgekey="901"/></edges></vertex>
  <vertex vertexkey="302"><edges><edge vertexkey="201" edgekey="902"/></edges></vertex>
</vertices></graph></structure></component>
<component name="BuildTree" library="user" uid="30"><structure><children>
  <component name="paths" library="xml" uid="10" kind="14"><data><root><entry name="Paths"><entry name="Path" outkey="401"/></entry></root><document schema="paths.xsd" instanceroot="{}Paths"/><parameter usageKind="input"/></data></component>
  <component name="directory" library="xml" uid="11" kind="14"><data><root><entry name="directory"><entry name="file" inpkey="501"><entry name="name" type="attribute" inpkey="502"/></entry><entry name="directory" inpkey="503"><entry name="name" type="attribute" inpkey="504"/><entry name="file" inpkey="505"/><entry name="directory" inpkey="506"/></entry></entry></root><document schema="directory.xsd" instanceroot="{}directory"/><parameter usageKind="output"/></data></component>
  <component name="contains" library="core" uid="12" kind="5"><sources><datapoint pos="0" key="601"/><datapoint pos="1" key="602"/></sources><targets><datapoint pos="0" key="603"/></targets></component>
  <component name="constant" library="core" uid="13" kind="2"><targets><datapoint pos="0" key="604"/></targets><data><constant value="\" datatype="string"/></data></component>
  <component name="Path" library="core" uid="14" kind="3"><sources><datapoint pos="0" key="605"/><datapoint pos="1" key="606"/></sources><targets><datapoint pos="0" key="607"/><datapoint pos="1" key="608"/></targets></component>
  <component name="substring-before" library="core" uid="15" kind="5"><sources><datapoint pos="0" key="609"/><datapoint pos="1" key="610"/></sources><targets><datapoint pos="0" key="611"/></targets></component>
  <component name="group-by" library="core" uid="16" kind="5"><sources><datapoint pos="0" key="612"/><datapoint pos="1" key="613"/></sources><targets><datapoint pos="0" key="614"/><datapoint pos="1" key="615"/></targets></component>
  <component name="substring-after" library="core" uid="17" kind="5"><sources><datapoint pos="0" key="616"/><datapoint pos="1" key="617"/></sources><targets><datapoint pos="0" key="618"/></targets></component>
  <component name="BuildTree" library="user" uid="31" kind="19"><data><root><entry name="Paths" componentid="10"><entry name="Paths"><entry name="Path" inpkey="619"/></entry></entry></root><root rootindex="1"><entry name="directory" componentid="11"><entry name="directory"><entry name="file" outkey="620"/><entry name="directory" outkey="621"/></entry></entry></root></data></component>
</children><graph><edges><edge edgekey="903"><data><dataconnection type="2"/></data></edge><edge edgekey="904"><data><dataconnection type="2"/></data></edge><edge edgekey="905"><data><dataconnection type="2"/></data></edge></edges><vertices>
  <vertex vertexkey="401"><edges><edge vertexkey="601"/><edge vertexkey="605"/><edge vertexkey="609"/><edge vertexkey="616"/><edge vertexkey="502"/></edges></vertex>
  <vertex vertexkey="604"><edges><edge vertexkey="602"/><edge vertexkey="610"/><edge vertexkey="617"/></edges></vertex>
  <vertex vertexkey="603"><edges><edge vertexkey="606"/></edges></vertex>
  <vertex vertexkey="607"><edges><edge vertexkey="612"/></edges></vertex>
  <vertex vertexkey="608"><edges><edge vertexkey="501" edgekey="903"/></edges></vertex>
  <vertex vertexkey="611"><edges><edge vertexkey="613"/></edges></vertex>
  <vertex vertexkey="614"><edges><edge vertexkey="503"/></edges></vertex>
  <vertex vertexkey="615"><edges><edge vertexkey="504"/></edges></vertex>
  <vertex vertexkey="618"><edges><edge vertexkey="619"/></edges></vertex>
  <vertex vertexkey="620"><edges><edge vertexkey="505" edgekey="904"/></edges></vertex>
  <vertex vertexkey="621"><edges><edge vertexkey="506" edgekey="905"/></edges></vertex>
</vertices></graph></structure></component>
</mapping>"#,
    )?;

    let imported = mfd::import(&dir.0.join("mapping.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(matches!(
        imported.project.root.construction,
        ScopeConstruction::PathHierarchy { .. }
    ));
    assert!(engine::validate(&imported.project).is_empty());
    let input = format_xml::read(&dir.0.join("paths.xml"), &imported.project.source)?;
    let output = engine::run(&imported.project, &input)?;
    assert_eq!(scalar(&output, "name"), Some("Project"));
    let files = output
        .field("file")
        .and_then(Instance::as_repeated)
        .ok_or("missing files")?;
    assert_eq!(scalar(&files[0], "name"), Some("README.md"));
    let directories = output
        .field("directory")
        .and_then(Instance::as_repeated)
        .ok_or("missing directories")?;
    assert_eq!(scalar(&directories[0], "name"), Some("src"));
    assert_eq!(scalar(&directories[1], "name"), Some("tests"));

    let export = dir.0.join("roundtrip.mfd");
    assert!(mfd::export(&imported.project, &export)?.is_empty());
    let reimported = mfd::import(&export)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(matches!(
        reimported.project.root.construction,
        ScopeConstruction::PathHierarchy { .. }
    ));
    assert!(engine::validate(&reimported.project).is_empty());
    assert_eq!(engine::run(&reimported.project, &input)?, output);
    Ok(())
}
