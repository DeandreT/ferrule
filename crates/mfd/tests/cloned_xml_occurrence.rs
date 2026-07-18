use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value, XML_TEXT_FIELD};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_cloned_xml_occurrence_{}_{}",
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

fn write_fixture(dir: &Path) -> Result<PathBuf, std::io::Error> {
    std::fs::write(
        dir.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:complexType name="Balance"><xs:sequence>
    <xs:element name="Amt"><xs:complexType><xs:simpleContent>
      <xs:extension base="xs:decimal"><xs:attribute name="Ccy" type="xs:string"/></xs:extension>
    </xs:simpleContent></xs:complexType></xs:element>
    <xs:element name="Kind" type="xs:string"/>
  </xs:sequence></xs:complexType>
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Debit" type="Balance" maxOccurs="unbounded"/>
    <xs:element name="Credit" type="Balance" maxOccurs="unbounded"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Statement"><xs:complexType><xs:sequence>
    <xs:element name="Header" type="xs:string"/>
    <xs:element name="Bal" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Amt"><xs:complexType><xs:simpleContent>
        <xs:extension base="xs:decimal"><xs:attribute name="Ccy" type="xs:string"/></xs:extension>
      </xs:simpleContent></xs:complexType></xs:element>
      <xs:element name="Kind" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;

    let design = dir.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data>
    <root><entry name="Source">
      <entry name="Debit" outkey="10"><entry name="Amt" outkey="11"><entry name="Ccy" type="attribute" outkey="12"/></entry><entry name="Kind" outkey="13"/></entry>
      <entry name="Credit" outkey="20"><entry name="Amt" outkey="21"><entry name="Ccy" type="attribute" outkey="22"/></entry><entry name="Kind" outkey="23"/></entry>
    </entry></root>
    <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
  </data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Statement">
      <entry name="Header" inpkey="50"/>
      <entry name="Bal" inpkey="30"><entry name="Amt" inpkey="31"><entry name="Ccy" type="attribute" inpkey="32"/></entry><entry name="Kind" inpkey="33"/></entry>
      <entry name="Bal" inpkey="40"><entry name="Amt" inpkey="41"><entry name="Ccy" type="attribute" inpkey="42"/></entry><entry name="Kind" inpkey="43"/></entry>
    </entry></root>
    <document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Statement"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="31"/></edges></vertex>
  <vertex vertexkey="12"><edges><edge vertexkey="32"/></edges></vertex>
  <vertex vertexkey="13"><edges><edge vertexkey="33"/><edge vertexkey="50"/></edges></vertex>
  <vertex vertexkey="20"><edges><edge vertexkey="40"/></edges></vertex>
  <vertex vertexkey="21"><edges><edge vertexkey="41"/></edges></vertex>
  <vertex vertexkey="22"><edges><edge vertexkey="42"/></edges></vertex>
  <vertex vertexkey="23"><edges><edge vertexkey="43"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

#[test]
fn cloned_occurrence_branches_keep_their_own_descendant_bindings()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let balance_scope = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Bal")
        .ok_or("missing balance scope")?;
    let segments = balance_scope
        .concatenated()
        .ok_or("balance branches were not concatenated")?
        .iter()
        .collect::<Vec<_>>();
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].source(), Some(["Debit".to_string()].as_slice()));
    assert_eq!(
        segments[1].source(),
        Some(["Credit".to_string()].as_slice())
    );

    let source = format_xml::from_str(
        r#"<Source><Debit><Amt Ccy="USD">10.5</Amt><Kind>D1</Kind></Debit><Debit><Amt Ccy="EUR">20</Amt><Kind>D2</Kind></Debit><Credit><Amt Ccy="GBP">7</Amt><Kind>C1</Kind></Credit></Source>"#,
        &imported.project.source,
    )?;
    let target = engine::run(&imported.project, &source)?;
    assert_eq!(
        target.field("Header").and_then(Instance::as_scalar),
        Some(&Value::String("D1".into()))
    );
    let balances = target
        .field("Bal")
        .and_then(Instance::as_repeated)
        .ok_or("target balances are not repeated")?;
    assert_eq!(balances.len(), 3);
    assert_eq!(
        balances[0].field("Kind").and_then(Instance::as_scalar),
        Some(&Value::String("D1".into()))
    );
    assert_eq!(
        balances[2].field("Kind").and_then(Instance::as_scalar),
        Some(&Value::String("C1".into()))
    );
    let credit_amount = balances[2].field("Amt").ok_or("missing credit amount")?;
    assert_eq!(
        credit_amount
            .field(XML_TEXT_FIELD)
            .and_then(Instance::as_scalar),
        Some(&Value::Float(7.0))
    );
    assert_eq!(
        credit_amount.field("Ccy").and_then(Instance::as_scalar),
        Some(&Value::String("GBP".into()))
    );
    Ok(())
}

#[test]
fn structural_branch_without_content_is_not_silently_concatenated()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let design = write_fixture(&dir.0)?;
    let mut mapping = std::fs::read_to_string(&design)?;
    for vertex in [
        r#"<vertex vertexkey="21"><edges><edge vertexkey="41"/></edges></vertex>"#,
        r#"<vertex vertexkey="22"><edges><edge vertexkey="42"/></edges></vertex>"#,
        r#"<vertex vertexkey="23"><edges><edge vertexkey="43"/></edges></vertex>"#,
    ] {
        mapping = mapping.replace(vertex, "");
    }
    std::fs::write(&design, mapping)?;

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("target group `Bal` has multiple connected structural sequence feeds")
    }));
    assert!(
        imported
            .project
            .root
            .children
            .iter()
            .find(|scope| scope.target_field == "Bal")
            .is_none_or(|scope| scope.concatenated().is_none())
    );
    Ok(())
}

#[test]
fn absent_edi_composites_do_not_emit_empty_cloned_occurrences()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    std::fs::write(
        dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Result"><xs:complexType><xs:sequence>
            <xs:element name="Station" maxOccurs="unbounded"><xs:complexType><xs:attribute name="code" type="xs:string"/></xs:complexType></xs:element>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )?;
    let design = dir.0.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="edi" library="text" kind="16"><data>
            <root><entry name="FileInstance"><entry name="document">
              <entry name="Envelope" ferrule-repeating="0"><entry name="Row" ferrule-repeating="1">
                <entry name="C1" ferrule-repeating="0" outkey="10"><entry name="Code" ferrule-repeating="0" datatype="string" outkey="11"/></entry>
                <entry name="C2" ferrule-repeating="0" outkey="20"><entry name="Code" ferrule-repeating="0" datatype="string" outkey="21"/></entry>
              </entry></entry>
            </entry></entry></root><text type="edi" kind="EDIFACT" inputinstance="source.edi"/>
          </data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
            <root><entry name="Result">
              <entry name="Station" inpkey="30"><entry name="code" type="attribute" inpkey="31"/></entry>
              <entry name="Station" inpkey="40" clone="1"><entry name="code" type="attribute" inpkey="41"/></entry>
            </entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Result"/>
          </data></component>
        </children><graph><vertices>
          <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
          <vertex vertexkey="11"><edges><edge vertexkey="31"/></edges></vertex>
          <vertex vertexkey="20"><edges><edge vertexkey="40"/></edges></vertex>
          <vertex vertexkey="21"><edges><edge vertexkey="41"/></edges></vertex>
        </vertices></graph></structure></component></mapping>"#,
    )?;

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let source = Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(vec![
            Instance::Group(vec![
                (
                    "C1".into(),
                    Instance::Group(vec![(
                        "Code".into(),
                        Instance::Scalar(Value::String("A".into())),
                    )]),
                ),
                (
                    "C2".into(),
                    Instance::Group(vec![("Code".into(), Instance::Scalar(Value::Null))]),
                ),
            ]),
            Instance::Group(vec![
                (
                    "C1".into(),
                    Instance::Group(vec![("Code".into(), Instance::Scalar(Value::Null))]),
                ),
                (
                    "C2".into(),
                    Instance::Group(vec![(
                        "Code".into(),
                        Instance::Scalar(Value::String("B".into())),
                    )]),
                ),
            ]),
        ]),
    )]);

    let target = engine::run(&imported.project, &source)?;
    let stations = target
        .field("Station")
        .and_then(Instance::as_repeated)
        .ok_or("target stations are not repeated")?;
    assert_eq!(stations.len(), 2);
    assert_eq!(
        stations
            .iter()
            .filter_map(|station| station.field("code").and_then(Instance::as_scalar))
            .collect::<Vec<_>>(),
        [&Value::String("A".into()), &Value::String("B".into())]
    );
    Ok(())
}

#[test]
fn scalar_only_cloned_occurrences_keep_document_order_around_sequences()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let design = write_fixture(&dir.0)?;
    let mut mapping = std::fs::read_to_string(&design)?;
    mapping = mapping.replace(
        r#"<entry name="Header" inpkey="50"/>"#,
        r#"<entry name="Header" inpkey="50"/>
      <entry name="Bal"><entry name="Kind" inpkey="34"/></entry>"#,
    );
    mapping = mapping.replace(
        r#"<entry name="Bal" inpkey="40"><entry name="Amt" inpkey="41"><entry name="Ccy" type="attribute" inpkey="42"/></entry><entry name="Kind" inpkey="43"/></entry>"#,
        r#"<entry name="Bal" inpkey="40"><entry name="Amt" inpkey="41"><entry name="Ccy" type="attribute" inpkey="42"/></entry><entry name="Kind" inpkey="43"/></entry>
      <entry name="Bal"><entry name="Kind" inpkey="44"/></entry>"#,
    );
    mapping = mapping.replace(
        r#"</data></component>
</children><graph>"#,
        r#"</data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint key="70"/></targets><data><constant value="S1" datatype="string"/></data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint key="71"/></targets><data><constant value="S4" datatype="string"/></data></component>
</children><graph>"#,
    );
    mapping = mapping.replace(
        r#"<graph><vertices>"#,
        r#"<graph><vertices>
  <vertex vertexkey="70"><edges><edge vertexkey="34"/></edges></vertex>
  <vertex vertexkey="71"><edges><edge vertexkey="44"/></edges></vertex>"#,
    );
    std::fs::write(&design, mapping)?;

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let validation = engine::validate(&imported.project);
    assert!(validation.is_empty(), "{validation:?}");
    let balance_scope = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Bal")
        .ok_or("missing balance scope")?;
    assert_eq!(
        balance_scope
            .concatenated()
            .ok_or("balance branches were not concatenated")?
            .len(),
        4
    );

    let source = format_xml::from_str(
        r#"<Source><Debit><Amt Ccy="USD">10.5</Amt><Kind>D1</Kind></Debit><Debit><Amt Ccy="EUR">20</Amt><Kind>D2</Kind></Debit><Credit><Amt Ccy="GBP">7</Amt><Kind>C1</Kind></Credit></Source>"#,
        &imported.project.source,
    )?;
    let target = engine::run(&imported.project, &source)?;
    let balances = target
        .field("Bal")
        .and_then(Instance::as_repeated)
        .ok_or("target balances are not repeated")?;
    let kinds = balances
        .iter()
        .filter_map(|balance| balance.field("Kind"))
        .filter_map(Instance::as_scalar)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            &Value::String("S1".into()),
            &Value::String("D1".into()),
            &Value::String("D2".into()),
            &Value::String("C1".into()),
            &Value::String("S4".into()),
        ]
    );
    Ok(())
}

#[test]
fn cloned_repeating_scalars_use_entry_order_instead_of_pin_order()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    std::fs::write(
        dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Street" type="xs:string"/>
    <xs:element name="Locality" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Target"><xs:complexType><xs:sequence>
    <xs:element name="Address"><xs:complexType><xs:sequence>
      <xs:element name="Line" type="xs:string" maxOccurs="4"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let mapping = dir.0.join("mapping.mfd");
    std::fs::write(
        &mapping,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data>
    <root><entry name="Source"><entry name="Street" outkey="10"/><entry name="Locality" outkey="20"/></entry></root>
    <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
  </data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Target"><entry name="Address">
      <entry name="Line" inpkey="60"/><entry name="Line" inpkey="30" clone="1"/>
    </entry></entry></root>
    <document schema="target.xsd" instanceroot="{}Target"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="60"/></edges></vertex>
  <vertex vertexkey="20"><edges><edge vertexkey="30"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;

    let imported = mfd::import(&mapping)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let source = format_xml::from_str(
        "<Source><Street>8 Oak Avenue</Street><Locality>Old Town</Locality></Source>",
        &imported.project.source,
    )?;
    let target = engine::run(&imported.project, &source)?;
    let lines = target
        .field("Address")
        .and_then(|address| address.field("Line"))
        .and_then(Instance::as_repeated)
        .ok_or("missing address lines")?;
    assert_eq!(
        lines
            .iter()
            .filter_map(Instance::as_scalar)
            .collect::<Vec<_>>(),
        [
            &Value::String("8 Oak Avenue".into()),
            &Value::String("Old Town".into())
        ]
    );
    Ok(())
}
