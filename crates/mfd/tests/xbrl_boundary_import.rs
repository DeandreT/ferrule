use std::error::Error;

use mapping::{Node, XbrlBoundaryMode};

use test_support::TempDir;

fn missing(description: &str) -> std::io::Error {
    std::io::Error::other(description)
}

#[test]
fn xbrl_source_boundary_retains_paths_and_xml_binding() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new("mfd_xbrl_import")?;
    dir.write(
        "target.xsd",
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Target"><xs:complexType><xs:sequence>
    <xs:element name="Amount" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    dir.write(
        "source-boundary.mfd",
        r#"<mapping version="26"><resources/>
  <component name="map"><structure><children>
    <component name="Facts" library="xbrl" kind="27"><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="xbrl">
        <entry name="Report"><entry name="Amount" outkey="10"/></entry>
      </entry></entry></entry></root>
      <xbrl schema="taxonomy/source.xsd" inputinstance="input/facts.xbrl"/>
    </data></component>
    <component name="Target" library="xml" kind="14">
      <properties XSLTDefaultOutput="1"/>
      <data>
        <root><entry name="FileInstance"><entry name="document"><entry name="Target">
          <entry name="Amount" inpkey="20"/>
        </entry></entry></entry></root>
        <document schema="target.xsd" outputinstance="out.xml" instanceroot="{}Target"/>
      </data>
    </component>
  </children></structure><connections><edge from="10" to="20"/></connections></component>
</mapping>"#,
    )?;

    let imported = mfd::import(&dir.path().join("source-boundary.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    assert_eq!(
        imported.project.source_path.as_deref(),
        Some("input/facts.xbrl")
    );
    assert_eq!(imported.project.target_path.as_deref(), Some("out.xml"));
    let boundary = imported
        .project
        .source_options
        .xbrl
        .as_ref()
        .ok_or_else(|| missing("missing XBRL source boundary options"))?;
    assert_eq!(boundary.mode(), XbrlBoundaryMode::ExternalSource);
    assert_eq!(boundary.taxonomy(), "taxonomy/source.xsd");
    assert_eq!(boundary.presentation(), None);
    assert!(imported.project.target_options.xbrl.is_none());

    let amount_schema = imported
        .project
        .source
        .child("Report")
        .and_then(|report| report.child("Amount"))
        .ok_or_else(|| missing("missing XBRL source Report/Amount path"))?;
    assert!(!amount_schema.repeating);
    let binding = imported
        .project
        .root
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Amount")
        .ok_or_else(|| missing("missing XML target Amount binding"))?;
    assert!(matches!(
        imported.project.graph.nodes.get(&binding.node),
        Some(Node::SourceField { frame: None, path })
            if path == &["Report".to_string(), "Amount".to_string()]
    ));
    Ok(())
}

#[test]
fn xbrl_target_boundary_retains_presentation_and_nested_binding() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new("mfd_xbrl_import")?;
    std::fs::create_dir_all(dir.path().join("presentation"))?;
    dir.write("presentation/table.sps", r#"<structure xmlns:ex="urn:example"><template subtype="xbrl-concept-aspect" match="ex:Amount"><children><calltemplate subtype="named" match="monetaryItemType"/></children></template></structure>"#)?;
    dir.write(
        "source.xsd",
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Value" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    dir.write(
        "target-boundary.mfd",
        r#"<mapping version="26"><resources/>
  <component name="map"><structure><children>
    <component name="Source" library="xml" kind="14"><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Source">
        <entry name="Value" outkey="10"/>
      </entry></entry></entry></root>
      <document schema="source.xsd" inputinstance="input.xml" instanceroot="{}Source"/>
    </data></component>
    <component name="Filing" library="xbrl" kind="27">
      <properties XSLTDefaultOutput="1"/>
      <data>
        <root><entry name="FileInstance"><entry name="document"><entry name="xbrl">
          <entry name="Report"><entry name="Amount" inpkey="20"/></entry>
        </entry></entry></entry></root>
        <xbrl schema="taxonomy/target.xsd" sps="presentation/table.sps"/>
      </data>
    </component>
  </children></structure><connections><edge from="10" to="20"/></connections></component>
</mapping>"#,
    )?;

    let imported = mfd::import(&dir.path().join("target-boundary.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    assert_eq!(imported.project.source_path.as_deref(), Some("input.xml"));
    assert_eq!(imported.project.target_path, None);
    assert!(imported.project.source_options.xbrl.is_none());
    let boundary = imported
        .project
        .target_options
        .xbrl
        .as_ref()
        .ok_or_else(|| missing("missing XBRL target boundary options"))?;
    assert_eq!(boundary.mode(), XbrlBoundaryMode::ExternalTarget);
    assert_eq!(boundary.taxonomy(), "taxonomy/target.xsd");
    assert_eq!(boundary.presentation(), Some("presentation/table.sps"));

    let amount_schema = imported
        .project
        .target
        .child("Report")
        .and_then(|report| report.child("Amount"))
        .ok_or_else(|| missing("missing XBRL target Report/Amount path"))?;
    assert!(!amount_schema.repeating);
    let report_scope = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Report")
        .ok_or_else(|| missing("missing XBRL Report target scope"))?;
    let binding = report_scope
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Amount")
        .ok_or_else(|| missing("missing XBRL target Amount binding"))?;
    assert!(matches!(
        imported.project.graph.nodes.get(&binding.node),
        Some(Node::SourceField { frame: None, path }) if path == &["Value".to_string()]
    ));
    Ok(())
}
