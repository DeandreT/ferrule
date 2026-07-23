use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};
use mapping::{JoinSourceCardinality, Scope, ScopeIteration};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_correlated_scalar_join_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) -> Result<(), std::io::Error> {
    std::fs::write(path, contents)
}

fn write_fixture(directory: &Path) -> Result<PathBuf, std::io::Error> {
    write(
        &directory.join("orders.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Orders"><xs:complexType><xs:sequence>
    <xs:element name="Order" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Id" type="xs:string"/>
      <xs:element name="CustomerNumber" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    write(
        &directory.join("customers.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Customers"><xs:complexType><xs:sequence>
    <xs:element name="Customer" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Number" type="xs:string"/>
      <xs:element name="Name" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    write(
        &directory.join("report.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Report"><xs:complexType><xs:sequence>
    <xs:element name="Order" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Id" type="xs:string"/>
      <xs:element name="Match" maxOccurs="unbounded"><xs:complexType><xs:sequence>
        <xs:element name="Number" type="xs:string"/>
        <xs:element name="Name" type="xs:string"/>
      </xs:sequence></xs:complexType></xs:element>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let design = directory.join("mapping.mfd");
    write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="orders" library="xml" kind="14"><data>
    <root><entry name="Orders"><entry name="Order" outkey="1"><entry name="Id" outkey="2"/><entry name="CustomerNumber" outkey="3"/></entry></entry></root>
    <document schema="orders.xsd" inputinstance="orders.xml" instanceroot="{}Orders"/>
  </data></component>
  <component name="customers" library="xml" kind="14"><data>
    <root><entry name="Customers"><entry name="Customer" outkey="4"><entry name="Number" outkey="5"/><entry name="Name" outkey="6"/></entry></entry></root>
    <document schema="customers.xsd" inputinstance="customers.xml" instanceroot="{}Customers"/>
  </data></component>
  <component name="join" library="core" uid="32" kind="32"><data>
    <root><entry name="document"><entry name="tuple">
      <entry name="dynamic_tree_node0"><entry name="CustomerNumber" inpkey="10"/></entry>
      <entry name="dynamic_tree_node1"><entry name="Customer" inpkey="20" outkey="21"><entry name="Number" outkey="22"/><entry name="Name" outkey="23"/></entry></entry>
    </entry></entry></root>
    <join><joinkeys><keypair><first-key path-id="101"/><second-key path-id="102"/></keypair></joinkeys>
      <keypaths><entry outkey="101"><condition/><entry name="Number" outkey="102"><condition/></entry></entry></keypaths>
    </join>
  </data></component>
  <component name="report" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="Report"><entry name="Order" inpkey="40"><entry name="Id" inpkey="41"/><entry name="Match" inpkey="42"><entry name="Number" inpkey="43"/><entry name="Name" inpkey="44"/></entry></entry></entry></root>
    <document schema="report.xsd" outputinstance="report.xml" instanceroot="{}Report"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="1"><edges><edge vertexkey="40"/></edges></vertex>
  <vertex vertexkey="2"><edges><edge vertexkey="41"/></edges></vertex>
  <vertex vertexkey="3"><edges><edge vertexkey="10"/></edges></vertex>
  <vertex vertexkey="4"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="21"><edges><edge vertexkey="42"/></edges></vertex>
  <vertex vertexkey="22"><edges><edge vertexkey="43"/></edges></vertex>
  <vertex vertexkey="23"><edges><edge vertexkey="44"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

fn child<'a>(scope: &'a Scope, field: &str) -> Option<&'a Scope> {
    scope
        .children
        .iter()
        .find(|child| child.target_field == field)
}

fn run_fixture(project: &mapping::Project) -> Result<Instance, Box<dyn Error>> {
    let source = format_xml::from_str(
        "<Orders><Order><Id>O-1</Id><CustomerNumber>B</CustomerNumber></Order><Order><Id>O-2</Id><CustomerNumber>A</CustomerNumber></Order><Order><Id>O-3</Id><CustomerNumber>Z</CustomerNumber></Order></Orders>",
        &project.source,
    )?;
    let extra = project
        .extra_sources
        .first()
        .ok_or("missing customer source")?;
    let customers = format_xml::from_str(
        "<Customers><Customer><Number>B</Number><Name>Bee-1</Name></Customer><Customer><Number>A</Number><Name>Ay</Name></Customer><Customer><Number>B</Number><Name>Bee-2</Name></Customer></Customers>",
        &extra.schema,
    )?;
    Ok(engine::run_with_sources(
        project,
        &source,
        vec![(extra.name.clone(), customers)],
    )?)
}

fn scalar<'a>(instance: &'a Instance, field: &str) -> Option<&'a Value> {
    instance.field(field).and_then(Instance::as_scalar)
}

fn repeated<'a>(instance: &'a Instance, field: &str) -> &'a [Instance] {
    instance
        .field(field)
        .and_then(Instance::as_repeated)
        .unwrap_or_default()
}

fn assert_execution(output: &Instance) -> Result<(), Box<dyn Error>> {
    let orders = output
        .field("Order")
        .and_then(Instance::as_repeated)
        .ok_or("missing repeated Order output")?;
    assert_eq!(orders.len(), 3);
    assert_eq!(scalar(&orders[0], "Id"), Some(&Value::String("O-1".into())));
    assert_eq!(scalar(&orders[1], "Id"), Some(&Value::String("O-2".into())));
    assert_eq!(scalar(&orders[2], "Id"), Some(&Value::String("O-3".into())));

    let first = repeated(&orders[0], "Match");
    assert_eq!(first.len(), 2);
    assert_eq!(
        scalar(&first[0], "Name"),
        Some(&Value::String("Bee-1".into()))
    );
    assert_eq!(
        scalar(&first[1], "Name"),
        Some(&Value::String("Bee-2".into()))
    );
    let second = repeated(&orders[1], "Match");
    assert_eq!(second.len(), 1);
    assert_eq!(
        scalar(&second[0], "Name"),
        Some(&Value::String("Ay".into()))
    );
    assert!(repeated(&orders[2], "Match").is_empty());
    Ok(())
}

#[test]
fn nested_scalar_join_correlates_and_roundtrips() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&directory.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let orders = child(&imported.project.root, "Order").ok_or("missing Order scope")?;
    assert_eq!(orders.source(), Some(["Order".to_string()].as_slice()));
    let matches = child(orders, "Match").ok_or("missing Match scope")?;
    let ScopeIteration::InnerJoin { plan, .. } = &matches.iteration else {
        return Err("Match does not use an inner join".into());
    };
    let sources = plan.sources().collect::<Vec<_>>();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].cardinality(), JoinSourceCardinality::Singleton);
    assert_eq!(sources[0].collection(), ["CustomerNumber"]);
    assert_eq!(sources[1].cardinality(), JoinSourceCardinality::Repeating);
    assert!(sources[1].collection().ends_with(&["Customer".to_string()]));

    let output = run_fixture(&imported.project)?;
    assert_execution(&output)?;

    let roundtrip = directory.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &roundtrip)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&roundtrip)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    let roundtrip_output = run_fixture(&reimported.project)?;
    assert_eq!(roundtrip_output, output);
    Ok(())
}
