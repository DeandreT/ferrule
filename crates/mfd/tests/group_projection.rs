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
        </children><graph><edges><edge edgekey="90"><data><dataconnection type="2"/></data></edge></edges><vertices><vertex vertexkey="10"><edges><edge vertexkey="20" edgekey="90"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
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
fn group_copy_below_repeated_owner_projects_the_connected_descendant() {
    let dir = TempDir::new();
    let source_xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Address"><xs:complexType><xs:sequence><xs:element name="City" type="xs:string"/><xs:element name="PostalCode" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#;
    write(&dir.0.join("source.xsd"), source_xsd);
    write(
        &dir.0.join("target.xsd"),
        &source_xsd.replace("name=\"Source\"", "name=\"Target\""),
    );
    write(
        &dir.0.join("source.xml"),
        "<Source><Row><Address><City>Seattle</City><PostalCode>98101</PostalCode></Address></Row><Row><Address><City>Portland</City><PostalCode>97205</PostalCode></Address></Row></Source>",
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Row" outkey="10"><entry name="Address" outkey="11"/></entry></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Row" inpkey="20"><entry name="Address" inpkey="21"/></entry></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><edges><edge edgekey="90"><data><dataconnection type="2"/></data></edge></edges><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="21" edgekey="90"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let source = format_xml::read(&dir.0.join("source.xml"), &imported.project.source).unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    let rows = target.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        scalar(rows[0].field("Address").unwrap(), "City"),
        &Value::String("Seattle".into())
    );
    assert_eq!(
        scalar(rows[1].field("Address").unwrap(), "PostalCode"),
        &Value::String("97205".into())
    );
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
        </children><graph><edges><edge edgekey="90"><data><dataconnection type="2"/></data></edge><edge edgekey="91"><data><dataconnection type="2"/></data></edge></edges><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="21" edgekey="90"/><edge vertexkey="22" edgekey="91"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let source = format_xml::read(&dir.0.join("source.xml"), &imported.project.source).unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    let headers = target
        .field("Header")
        .and_then(Instance::as_mapped_sequence)
        .unwrap();
    assert_eq!(headers.len(), 2);
    assert_eq!(scalar(&headers[0], "Name"), &Value::String("Ada".into()));
    assert_eq!(scalar(&headers[1], "Name"), &Value::String("Grace".into()));
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
fn exact_repeating_group_descendants_are_copied_as_complete_items() {
    let dir = TempDir::new();
    let source_xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Wrapper"><xs:complexType><xs:sequence><xs:element name="Label" type="xs:string"/><xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Name" type="xs:string"/><xs:element name="Quantity" type="xs:integer"/><xs:element name="Tag" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence><xs:attribute name="code" type="xs:string"/></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#;
    write(&dir.0.join("source.xsd"), source_xsd);
    write(
        &dir.0.join("target.xsd"),
        &source_xsd.replace("name=\"Source\"", "name=\"Target\""),
    );
    write(
        &dir.0.join("source.xml"),
        r#"<Source><Wrapper><Label>catalog</Label><Item code="A"><Name>first</Name><Quantity>2</Quantity><Tag><Value>x</Value></Tag><Tag><Value>y</Value></Tag></Item><Item code="B"><Name>second</Name><Quantity>4</Quantity><Tag><Value>z</Value></Tag></Item></Wrapper></Source>"#,
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Wrapper" outkey="10"/></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Wrapper" inpkey="20"/></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><edges><edge edgekey="90"><data><dataconnection type="2"/></data></edge></edges><vertices><vertex vertexkey="10"><edges><edge vertexkey="20" edgekey="90"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let wrapper_scope = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Wrapper")
        .unwrap();
    let item_scope = wrapper_scope
        .children
        .iter()
        .find(|scope| scope.target_field == "Item")
        .unwrap();
    assert_eq!(
        item_scope.construction,
        mapping::ScopeConstruction::CopyCurrentSource
    );

    let source = format_xml::read(&dir.0.join("source.xml"), &imported.project.source).unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    let wrapper = target.field("Wrapper").unwrap();
    assert_eq!(scalar(wrapper, "Label"), &Value::String("catalog".into()));
    let items = wrapper
        .field("Item")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(scalar(&items[0], "code"), &Value::String("A".into()));
    assert_eq!(scalar(&items[1], "Name"), &Value::String("second".into()));
    let tags = items[0]
        .field("Tag")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(tags.len(), 2);
    assert_eq!(scalar(&tags[1], "Value"), &Value::String("y".into()));

    let design = dir.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported = std::fs::read_to_string(&design).unwrap();
    assert!(exported.contains(r#"<dataconnection type="2"/>"#));

    let reimported = mfd::import(&design).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    let roundtrip_wrapper = reimported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Wrapper")
        .unwrap();
    let roundtrip_item = roundtrip_wrapper
        .children
        .iter()
        .find(|scope| scope.target_field == "Item")
        .unwrap();
    assert_eq!(
        roundtrip_item.construction,
        mapping::ScopeConstruction::CopyCurrentSource
    );
    let roundtrip = engine::run(&reimported.project, &source).unwrap();
    assert_eq!(roundtrip, target);
}

#[test]
fn mismatched_repeating_group_descendants_are_not_copied_lossily() {
    let dir = TempDir::new();
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Wrapper"><xs:complexType><xs:sequence><xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Wrapper"><xs:complexType><xs:sequence><xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/><xs:element name="Required" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Wrapper" outkey="10"/></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Wrapper" inpkey="20"/></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><edges><edge edgekey="90"><data><dataconnection type="2"/></data></edge></edges><vertices><vertex vertexkey="10"><edges><edge vertexkey="20" edgekey="90"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    );

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("only repeating"));
    assert!(imported.project.root.children.is_empty());
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
        </children><graph><edges><edge edgekey="90"><data><dataconnection type="2"/></data></edge></edges><vertices><vertex vertexkey="10"><edges><edge vertexkey="20" edgekey="90"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
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
        </children><graph><edges><edge edgekey="90"><data><dataconnection type="2"/></data></edge></edges><vertices><vertex vertexkey="10"><edges><edge vertexkey="20" edgekey="90"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
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
