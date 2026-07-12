use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_group_projection_{}_{}",
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

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

fn scalar<'a>(instance: &'a Instance, field: &str) -> &'a Value {
    instance.field(field).and_then(Instance::as_scalar).unwrap()
}

#[test]
fn root_group_projects_nested_scalars_simple_content_and_attributes() {
    let dir = TempDir::new();
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Info"><xs:complexType><xs:sequence><xs:element name="Description"><xs:complexType><xs:simpleContent><xs:extension base="xs:string"><xs:attribute name="code" type="xs:string"/></xs:extension></xs:simpleContent></xs:complexType></xs:element><xs:element name="Name" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("target.xsd"),
        &std::fs::read_to_string(dir.0.join("source.xsd"))
            .unwrap()
            .replace("name=\"Source\"", "name=\"Target\""),
    );
    write(
        &dir.0.join("source.xml"),
        r#"<Source><Info><Description code="A">projected text</Description><Name>source name</Name></Info></Source>"#,
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source" outkey="10"><entry name="Info"/></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target" inpkey="20"><entry name="Info"/></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let source = format_xml::read(&dir.0.join("source.xml"), &imported.project.source).unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    let info = target.field("Info").unwrap();
    assert_eq!(scalar(info, "Name"), &Value::String("source name".into()));
    let description = info.field("Description").unwrap();
    assert_eq!(
        scalar(description, ir::XML_TEXT_FIELD),
        &Value::String("projected text".into())
    );
    assert_eq!(scalar(description, "code"), &Value::String("A".into()));
}

#[test]
fn connected_leaf_makes_its_parent_group_port_redundant() {
    let dir = TempDir::new();
    let source_xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Group"><xs:complexType><xs:sequence><xs:element name="A" type="xs:string"/><xs:element name="B" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#;
    write(&dir.0.join("source.xsd"), source_xsd);
    write(
        &dir.0.join("target.xsd"),
        &source_xsd.replace("name=\"Source\"", "name=\"Target\""),
    );
    write(
        &dir.0.join("source.xml"),
        "<Source><Group><A>source a</A><B>source b</B></Group></Source>",
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Group" outkey="10"/></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="constant" library="core" kind="2"><targets><datapoint key="11"/></targets><data><constant value="override" datatype="string"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Group" inpkey="20"><entry name="A" inpkey="21"/></entry></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let source = format_xml::read(&dir.0.join("source.xml"), &imported.project.source).unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    let group = target.field("Group").unwrap();
    assert_eq!(scalar(group, "A"), &Value::String("override".into()));
    assert!(group.field("B").is_none());
}

#[test]
fn controlled_non_repeating_group_feed_is_not_projected() {
    let dir = TempDir::new();
    let source_xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Group"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#;
    write(&dir.0.join("source.xsd"), source_xsd);
    write(
        &dir.0.join("target.xsd"),
        &source_xsd.replace("name=\"Source\"", "name=\"Target\""),
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Group" outkey="10"/></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="constant" library="core" kind="2"><targets><datapoint key="11"/></targets><data><constant value="true" datatype="boolean"/></data></component>
          <component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="12"/><datapoint pos="1" key="13"/></sources><targets><datapoint pos="0" key="14"/><datapoint/></targets></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Group" inpkey="20"/></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="12"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="13"/></edges></vertex><vertex vertexkey="14"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("group `Group` ignored"));
    assert!(imported.project.root.bindings.is_empty());
}

#[test]
fn group_projection_uses_only_its_owning_iteration_frame() {
    let dir = TempDir::new();
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Order" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Customer"><xs:complexType><xs:sequence><xs:element name="Number" type="xs:string"/><xs:element name="Name" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Header"><xs:complexType><xs:sequence><xs:element name="Number" type="xs:string"/><xs:element name="Name" type="xs:string"/></xs:sequence></xs:complexType></xs:element><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Customer"><xs:complexType><xs:sequence><xs:element name="Number" type="xs:string"/><xs:element name="Name" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("source.xml"),
        r#"<Source><Order><Customer><Number>1</Number><Name>Ada</Name></Customer></Order><Order><Customer><Number>2</Number><Name>Grace</Name></Customer></Order></Source>"#,
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Order" outkey="10"><entry name="Customer" outkey="11"/></entry></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Header" inpkey="22"/><entry name="Row" inpkey="20"><entry name="Customer" inpkey="21"/></entry></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="21"/><edge vertexkey="22"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let source = format_xml::read(&dir.0.join("source.xml"), &imported.project.source).unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    assert_eq!(
        scalar(target.field("Header").unwrap(), "Name"),
        &Value::String("Ada".into())
    );
    let rows = target.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        scalar(rows[0].field("Customer").unwrap(), "Name"),
        &Value::String("Ada".into())
    );
    assert_eq!(
        scalar(rows[1].field("Customer").unwrap(), "Number"),
        &Value::String("2".into())
    );
}

#[test]
fn repeating_only_group_projection_warns_once_and_adds_no_bindings() {
    let dir = TempDir::new();
    let source_xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Wrapper"><xs:complexType><xs:sequence><xs:element name="Item" type="xs:string" maxOccurs="unbounded"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#;
    write(&dir.0.join("source.xsd"), source_xsd);
    write(
        &dir.0.join("target.xsd"),
        &source_xsd.replace("name=\"Source\"", "name=\"Target\""),
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Wrapper" outkey="10"/></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Wrapper" inpkey="20"/></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("only repeating"));
    assert!(imported.project.root.bindings.is_empty());
}

#[test]
fn mixed_group_projection_copies_scalars_and_warns_about_repeating_children() {
    let dir = TempDir::new();
    let source_xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Wrapper"><xs:complexType><xs:sequence><xs:element name="Label" type="xs:string"/><xs:element name="Item" type="xs:string" maxOccurs="unbounded"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#;
    write(&dir.0.join("source.xsd"), source_xsd);
    write(
        &dir.0.join("target.xsd"),
        &source_xsd.replace("name=\"Source\"", "name=\"Target\""),
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Wrapper" outkey="10"/></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Wrapper" inpkey="20"/></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("were not copied"));
    let wrapper = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Wrapper")
        .unwrap();
    assert_eq!(wrapper.bindings.len(), 1);
    assert_eq!(wrapper.bindings[0].target_field, "Label");
}

#[test]
fn scalar_group_port_populates_xml_text_alongside_an_explicit_attribute() {
    let dir = TempDir::new();
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Code" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Description"><xs:complexType mixed="true"><xs:sequence><xs:element name="Bold" type="xs:string" minOccurs="0"/></xs:sequence><xs:attribute name="code" type="xs:string"/></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(&dir.0.join("source.xml"), "<Source><Code>A</Code></Source>");
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Code" outkey="11"/></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="constant" library="core" kind="2"><targets><datapoint key="10"/></targets><data><constant value="promoted" datatype="string"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Description" inpkey="20"><entry name="code" type="attribute" inpkey="21"/></entry></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="21"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let source = format_xml::read(&dir.0.join("source.xml"), &imported.project.source).unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    let description = target.field("Description").unwrap();
    assert_eq!(
        scalar(description, ir::XML_TEXT_FIELD),
        &Value::String("promoted".into())
    );
    assert_eq!(scalar(description, "code"), &Value::String("A".into()));
    let output = format_xml::to_string(&imported.project.target, &target).unwrap();
    assert!(
        output.contains("<Description code=\"A\">promoted</Description>"),
        "{output}"
    );
}

#[test]
fn structured_group_feed_is_not_lowered_to_xml_text() {
    let dir = TempDir::new();
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Record" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Description"><xs:complexType mixed="true"><xs:sequence><xs:element name="Bold" type="xs:string" minOccurs="0"/></xs:sequence><xs:attribute name="code" type="xs:string"/></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Record" outkey="10"><entry name="Value"/></entry></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Description" inpkey="20"/></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("group `Description` ignored"));
    assert!(imported.project.root.children.is_empty());
}
