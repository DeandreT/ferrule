use ir::{Instance, Value};

use test_support::TempDir;

#[test]
fn descendant_binding_constructs_one_repeating_xml_occurrence() {
    let dir = TempDir::new("mfd_static_repeating_output").unwrap();
    dir.write("source.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Label" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#).unwrap();
    dir.write("target.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Entry" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Label" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#).unwrap();
    dir.write("source.xml", "<Source><Label>static label</Label></Source>")
        .unwrap();
    dir.write("mapping.mfd", r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Label" outkey="10"/></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Entry"><entry name="Label" inpkey="20"/></entry></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#).unwrap();

    let imported = mfd::import(&dir.path().join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );
    let entry_scope = &imported.project.root.children[0];
    assert_eq!(entry_scope.target_field, "Entry");
    assert!(!entry_scope.iterates());

    let source =
        format_xml::read(&dir.path().join("source.xml"), &imported.project.source).unwrap();
    let target = engine::run(&imported.project, &source).unwrap();
    let entries = target
        .field("Entry")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].field("Label").and_then(Instance::as_scalar),
        Some(&Value::String("static label".into()))
    );

    let output_path = dir.path().join("written.xml");
    format_xml::write(&output_path, &imported.project.target, &target).unwrap();
    let written = std::fs::read_to_string(output_path).unwrap();
    assert_eq!(written.matches("<Entry>").count(), 1);
    assert!(written.contains("<Label>static label</Label>"));
}
