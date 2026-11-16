use std::error::Error;

use ir::{Instance, Value};

use test_support::TempDir;

#[test]
fn imports_executes_and_roundtrips_external_xslt_aggregate() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new("external_xslt")?;
    dir.write(
        "source.xsd",
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Catalog"><xs:complexType><xs:sequence>
    <xs:element name="Items"><xs:complexType><xs:sequence>
      <xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence>
        <xs:element name="Cost" type="xs:integer"/>
      </xs:sequence></xs:complexType></xs:element>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    dir.write(
        "target.xsd",
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Report"><xs:complexType><xs:sequence>
    <xs:element name="Total" type="xs:integer"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    dir.write(
        "pricing.xslt",
        r#"<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0">
  <xsl:template name="GrandTotal">
    <xsl:param name="catalog"/>
    <xsl:value-of select="sum($catalog/Item/Cost)"/>
  </xsl:template>
</xsl:stylesheet>"#,
    )?;
    dir.write("mapping.mfd", r#"<mapping><component name="map"><structure><children>
  <component name="catalog" library="xml" kind="14"><data><root>
    <entry name="Catalog"><entry name="Items" outkey="10"><entry name="Item"><entry name="Cost"/></entry></entry></entry>
  </root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Catalog"/></data></component>
  <component name="GrandTotal" library="pricing" kind="5">
    <sources><datapoint pos="0" key="20"/></sources>
    <targets><datapoint pos="0" key="21"/></targets>
  </component>
  <component name="report" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root>
    <entry name="Report"><entry name="Total" inpkey="30"/></entry>
  </root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Report"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="21"><edges><edge vertexkey="30"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#)?;

    let imported = mfd::import(&dir.path().join("mapping.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let input = Instance::Group(
        (vec![(
            "Items".into(),
            Instance::Group(
                (vec![(
                    "Item".into(),
                    Instance::Repeated(vec![
                        Instance::Group(
                            (vec![("Cost".into(), Instance::Scalar(Value::Int(5)))]).into(),
                        ),
                        Instance::Group(
                            (vec![("Cost".into(), Instance::Scalar(Value::Int(7)))]).into(),
                        ),
                    ]),
                )])
                .into(),
            ),
        )])
        .into(),
    );
    assert_total(&engine::run(&imported.project, &input)?, 12);

    let export_path = dir.path().join("roundtrip.mfd");
    let export_warnings = mfd::export(&imported.project, &export_path)?;
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    let roundtripped = mfd::import(&export_path)?;
    assert!(
        roundtripped.warnings.is_empty(),
        "{:?}",
        roundtripped.warnings
    );
    assert!(engine::validate(&roundtripped.project).is_empty());
    assert_total(&engine::run(&roundtripped.project, &input)?, 12);
    Ok(())
}

fn assert_total(output: &Instance, expected: i64) {
    assert_eq!(
        output.field("Total").and_then(Instance::as_scalar),
        Some(&Value::Int(expected))
    );
}
