use ir::{Instance, Value, XML_MIXED_CONTENT_FIELD, XML_NODE_NAME_FIELD};
use mapping::ScopeConstruction;

use test_support::TempDir;

#[test]
fn mapped_mixed_children_retain_source_document_order() {
    let directory = TempDir::new("mfd_target_mixed_content").unwrap();
    std::fs::write(
        directory.path().join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input"><xs:complexType><xs:sequence><xs:element name="Body" maxOccurs="unbounded"><xs:complexType mixed="true"><xs:choice minOccurs="0" maxOccurs="unbounded"><xs:element name="Strong" type="xs:string"/><xs:element name="Em" type="xs:string"/></xs:choice></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        directory.path().join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Output"><xs:complexType><xs:sequence><xs:element name="Description"><xs:complexType mixed="true"><xs:choice minOccurs="0" maxOccurs="unbounded"><xs:element name="Bold" type="xs:string"/><xs:element name="Italic" type="xs:string"/></xs:choice></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )
    .unwrap();
    let design = directory.path().join("mapping.mfd");
    std::fs::write(
        &design,
        r##"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Input"><entry name="Body" outkey="1"><entry name="#text" outkey="2"/><entry name="Strong" outkey="3"/><entry name="Em" outkey="4"/></entry></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Input"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Output"><entry name="Description" inpkey="11"><entry name="#text" inpkey="12"/><entry name="Bold" inpkey="13"/><entry name="Italic" inpkey="14"/></entry></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Output"/></data></component>
        </children><graph><vertices>
          <vertex vertexkey="1"><edges><edge vertexkey="11"/></edges></vertex>
          <vertex vertexkey="2"><edges><edge vertexkey="12"/></edges></vertex>
          <vertex vertexkey="3"><edges><edge vertexkey="13"/></edges></vertex>
          <vertex vertexkey="4"><edges><edge vertexkey="14"/></edges></vertex>
        </vertices></graph></structure></component></mapping>"##,
    )
    .unwrap();

    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let description = &imported.project.root.children[0];
    assert!(matches!(
        &description.construction,
        ScopeConstruction::XmlMixedContent { elements }
            if elements.len() == 2
    ));

    let source = format_xml::from_str(
        "<Input><Body>Hello <Strong>world</Strong>, use <Em>care</Em> and <Em>precision</Em>.</Body></Input>",
        &imported.project.source,
    )
    .unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    let description = output.field("Description").unwrap();
    let ordered = description
        .field(XML_MIXED_CONTENT_FIELD)
        .and_then(Instance::as_repeated)
        .unwrap();
    let names = ordered
        .iter()
        .map(|item| {
            item.field(XML_NODE_NAME_FIELD)
                .and_then(Instance::as_scalar)
                .cloned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            Some(Value::String(String::new())),
            Some(Value::String("Bold".into())),
            Some(Value::String(String::new())),
            Some(Value::String("Italic".into())),
            Some(Value::String("".into())),
            Some(Value::String("Italic".into())),
            Some(Value::String("".into())),
        ]
    );
    let xml = format_xml::to_string(&imported.project.target, &output).unwrap();
    assert!(
        xml.contains(
            "<Description>Hello <Bold>world</Bold>, use <Italic>care</Italic> and <Italic>precision</Italic>.</Description>"
        ),
        "{xml}"
    );

    let exported = directory.path().join("roundtrip.mfd");
    assert!(
        mfd::export(&imported.project, &exported)
            .unwrap()
            .is_empty()
    );
    let roundtrip = mfd::import(&exported).unwrap();
    assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
    assert!(engine::validate(&roundtrip.project).is_empty());
    assert_eq!(output, engine::run(&roundtrip.project, &source).unwrap());
}
