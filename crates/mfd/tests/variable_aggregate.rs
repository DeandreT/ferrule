use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::{AggregateOp, Node};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_variable_aggregate_{}_{}",
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

fn write_fixture(dir: &Path) -> Result<PathBuf, std::io::Error> {
    std::fs::write(
        dir.join("catalog.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Catalog"><xs:complexType><xs:sequence>
    <xs:element name="Entry" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Given" type="xs:string"/>
      <xs:element name="Family" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.join("entry.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Entry"><xs:complexType><xs:sequence>
    <xs:element name="Given" type="xs:string"/>
    <xs:element name="Family" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.join("summary.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Summary"><xs:complexType><xs:sequence>
    <xs:element name="Names" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.join("catalog.xml"),
        "<Catalog><Entry><Given>Ada</Given><Family>Lovelace</Family></Entry><Entry><Given>Grace</Given><Family>Hopper</Family></Entry></Catalog>",
    )?;

    let design = dir.join("variable-aggregate.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="catalog" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Catalog"><entry name="Entry" outkey="1"><entry name="Given" outkey="2"/><entry name="Family" outkey="3"/></entry></entry></entry></entry></root><document schema="catalog.xsd" inputinstance="catalog.xml" instanceroot="{}Catalog"/></data></component>
  <component name="entry-variable" library="xml" kind="14"><data><parameter usageKind="variable"/><root><entry name="document"><entry name="Entry" inpkey="10"><entry name="Given" outkey="11"/><entry name="Family" outkey="12"/></entry></entry></root><document schema="entry.xsd" instanceroot="{}Entry"/></data></component>
  <component name="concat" library="core" kind="5"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/><datapoint pos="2" key="22"/></sources><targets><datapoint pos="0" key="23"/></targets></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="27"/></targets><data><constant value=" " datatype="string"/></data></component>
  <component name="string-join" library="core" kind="5"><sources><datapoint/><datapoint pos="1" key="24"/><datapoint pos="2" key="25"/></sources><targets><datapoint pos="0" key="26"/></targets></component>
  <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="28"/></targets><data><constant value=" | " datatype="string"/></data></component>
  <component name="summary" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Summary"><entry name="Names" inpkey="30"/></entry></entry></entry></root><document schema="summary.xsd" instanceroot="{}Summary"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="1"><edges><edge vertexkey="10"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="27"><edges><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="12"><edges><edge vertexkey="22"/></edges></vertex>
  <vertex vertexkey="23"><edges><edge vertexkey="24"/></edges></vertex>
  <vertex vertexkey="28"><edges><edge vertexkey="25"/></edges></vertex>
  <vertex vertexkey="26"><edges><edge vertexkey="30"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

#[test]
fn computed_aggregate_resolves_transparent_variable_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let imported = mfd::import(&write_fixture(&dir.0)?)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);

    let (collection, expression) = imported
        .project
        .graph
        .nodes
        .values()
        .find_map(|node| match node {
            Node::Aggregate {
                function: AggregateOp::Join,
                collection,
                expression: Some(expression),
                ..
            } => Some((collection, expression)),
            _ => None,
        })
        .ok_or_else(|| std::io::Error::other("computed string-join was not imported"))?;
    assert_eq!(collection, &["Entry"]);
    assert!(matches!(
        imported.project.graph.nodes.get(expression),
        Some(Node::Call { function, .. }) if function == "concat"
    ));

    let source = format_xml::read(&dir.0.join("catalog.xml"), &imported.project.source)?;
    let output = engine::run(&imported.project, &source)?;
    assert_eq!(
        output.field("Names").and_then(Instance::as_scalar),
        Some(&Value::String("Ada Lovelace | Grace Hopper".to_string()))
    );
    Ok(())
}

#[test]
fn filtered_cross_source_aggregate_lowers_to_an_inner_join()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    std::fs::write(
        dir.0.join("order.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Order"><xs:complexType><xs:sequence><xs:element name="Line" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Sku" type="xs:string"/><xs:element name="Quantity" type="xs:int"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    std::fs::write(
        dir.0.join("catalog.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Catalog"><xs:complexType><xs:sequence><xs:element name="Product" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Sku" type="xs:string"/><xs:element name="Price" type="xs:double"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    std::fs::write(
        dir.0.join("summary.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Summary"><xs:complexType><xs:sequence><xs:element name="Total" type="xs:double"/><xs:element name="Matches" type="xs:int"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    let design = dir.0.join("filtered-join-aggregate.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="orders" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Order"><entry name="Line" outkey="1"><entry name="Sku" outkey="2"/><entry name="Quantity" outkey="3"/></entry></entry></entry></entry></root><document schema="order.xsd" inputinstance="order.xml" instanceroot="{}Order"/></data></component>
  <component name="catalog" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Catalog"><entry name="Product" outkey="10"><entry name="Sku" outkey="11"/><entry name="Price" outkey="12"/></entry></entry></entry></entry></root><document schema="catalog.xsd" inputinstance="catalog.xml" instanceroot="{}Catalog"/></data></component>
  <component name="multiply" library="core" kind="5"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/></sources><targets><datapoint pos="0" key="22"/></targets></component>
  <component name="equal" library="core" kind="5"><sources><datapoint pos="0" key="23"/><datapoint pos="1" key="24"/></sources><targets><datapoint pos="0" key="25"/></targets></component>
  <component name="matching-products" library="core" kind="3"><sources><datapoint pos="0" key="26"/><datapoint pos="1" key="27"/></sources><targets><datapoint pos="0" key="28"/><datapoint/></targets></component>
  <component name="sum" library="core" kind="5"><sources><datapoint/><datapoint pos="1" key="29"/></sources><targets><datapoint pos="0" key="30"/></targets></component>
  <component name="count" library="core" kind="5"><sources><datapoint/><datapoint pos="1" key="31"/></sources><targets><datapoint pos="0" key="32"/></targets></component>
  <component name="summary" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Summary"><entry name="Total" inpkey="40"/><entry name="Matches" inpkey="41"/></entry></entry></entry></root><document schema="summary.xsd" outputinstance="summary.xml" instanceroot="{}Summary"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="3"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="12"><edges><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="2"><edges><edge vertexkey="23"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="24"/></edges></vertex>
  <vertex vertexkey="22"><edges><edge vertexkey="26"/></edges></vertex><vertex vertexkey="25"><edges><edge vertexkey="27"/></edges></vertex>
  <vertex vertexkey="28"><edges><edge vertexkey="29"/><edge vertexkey="31"/></edges></vertex>
  <vertex vertexkey="30"><edges><edge vertexkey="40"/></edges></vertex><vertex vertexkey="32"><edges><edge vertexkey="41"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported
            .project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(node, Node::JoinAggregate { .. }))
            .count(),
        2
    );
    assert!(engine::validate(&imported.project).is_empty());

    let record = |fields: Vec<(&str, Value)>| {
        Instance::Group(
            fields
                .into_iter()
                .map(|(name, value)| (name.to_string(), Instance::Scalar(value)))
                .collect(),
        )
    };
    let source = Instance::Group(vec![(
        "Line".into(),
        Instance::Repeated(vec![
            record(vec![
                ("Sku", Value::String("A".into())),
                ("Quantity", Value::Int(2)),
            ]),
            record(vec![
                ("Sku", Value::String("B".into())),
                ("Quantity", Value::Int(3)),
            ]),
        ]),
    )]);
    let catalog = Instance::Group(vec![(
        "Product".into(),
        Instance::Repeated(vec![
            record(vec![
                ("Sku", Value::String("A".into())),
                ("Price", Value::Float(10.0)),
            ]),
            record(vec![
                ("Sku", Value::String("B".into())),
                ("Price", Value::Float(5.0)),
            ]),
            record(vec![
                ("Sku", Value::String("C".into())),
                ("Price", Value::Float(7.0)),
            ]),
        ]),
    )]);
    let catalog_name = imported.project.extra_sources[0].name.clone();
    let output = engine::run_with_sources(
        &imported.project,
        &source,
        vec![(catalog_name, catalog.clone())],
    )?;
    assert_eq!(
        output.field("Total").and_then(Instance::as_scalar),
        Some(&Value::Float(35.0))
    );
    assert_eq!(
        output.field("Matches").and_then(Instance::as_scalar),
        Some(&Value::Int(2))
    );

    let exported = dir.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &exported)?;
    assert!(warnings.is_empty(), "{warnings:?}");
    let exported_xml = std::fs::read_to_string(&exported)?;
    assert!(exported_xml.contains("kind=\"32\""));
    let reimported = mfd::import(&exported)?;
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    let catalog_name = reimported.project.extra_sources[0].name.clone();
    let roundtrip =
        engine::run_with_sources(&reimported.project, &source, vec![(catalog_name, catalog)])?;
    assert_eq!(roundtrip, output);
    Ok(())
}

#[test]
fn filtered_cross_source_aggregate_does_not_broaden_an_enclosing_item_frame()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    std::fs::write(
        dir.0.join("order.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Order"><xs:complexType><xs:sequence><xs:element name="Line" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Sku" type="xs:string"/><xs:element name="Quantity" type="xs:int"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    std::fs::write(
        dir.0.join("catalog.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Catalog"><xs:complexType><xs:sequence><xs:element name="Product" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Sku" type="xs:string"/><xs:element name="Price" type="xs:double"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    std::fs::write(
        dir.0.join("results.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Results"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Sku" type="xs:string"/><xs:element name="Total" type="xs:double"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    let design = dir.0.join("correlated-filtered-aggregate.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="orders" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Order"><entry name="Line" outkey="1"><entry name="Sku" outkey="2"/><entry name="Quantity" outkey="3"/></entry></entry></entry></entry></root><document schema="order.xsd" inputinstance="order.xml" instanceroot="{}Order"/></data></component>
  <component name="catalog" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Catalog"><entry name="Product" outkey="10"><entry name="Sku" outkey="11"/><entry name="Price" outkey="12"/></entry></entry></entry></entry></root><document schema="catalog.xsd" inputinstance="catalog.xml" instanceroot="{}Catalog"/></data></component>
  <component name="multiply" library="core" kind="5" growable="1"><sources><datapoint pos="0" key="20"/><datapoint pos="1" key="21"/></sources><targets><datapoint pos="0" key="22"/></targets></component>
  <component name="equal" library="core" kind="5"><sources><datapoint pos="0" key="23"/><datapoint pos="1" key="24"/></sources><targets><datapoint pos="0" key="25"/></targets></component>
  <component name="matching-products" library="core" kind="3"><sources><datapoint pos="0" key="26"/><datapoint pos="1" key="27"/></sources><targets><datapoint pos="0" key="28"/><datapoint/></targets></component>
  <component name="sum" library="core" kind="5"><sources><datapoint/><datapoint pos="1" key="29"/></sources><targets><datapoint pos="0" key="30"/></targets></component>
  <component name="results" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Results"><entry name="Row" inpkey="40"><entry name="Sku" inpkey="41"/><entry name="Total" inpkey="42"/></entry></entry></entry></entry></root><document schema="results.xsd" outputinstance="results.xml" instanceroot="{}Results"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="3"><edges><edge vertexkey="20"/></edges></vertex><vertex vertexkey="12"><edges><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="2"><edges><edge vertexkey="23"/></edges></vertex><vertex vertexkey="11"><edges><edge vertexkey="24"/></edges></vertex>
  <vertex vertexkey="22"><edges><edge vertexkey="26"/></edges></vertex><vertex vertexkey="25"><edges><edge vertexkey="27"/></edges></vertex>
  <vertex vertexkey="28"><edges><edge vertexkey="29"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="42"/></edges></vertex>
  <vertex vertexkey="1"><edges><edge vertexkey="40"/></edges></vertex><vertex vertexkey="2"><edges><edge vertexkey="41"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;

    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert!(
        imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::JoinAggregate { .. }))
    );

    let record = |fields: Vec<(&str, Value)>| {
        Instance::Group(
            fields
                .into_iter()
                .map(|(name, value)| (name.to_string(), Instance::Scalar(value)))
                .collect(),
        )
    };
    let source = Instance::Group(vec![(
        "Line".into(),
        Instance::Repeated(vec![
            record(vec![
                ("Sku", Value::String("A".into())),
                ("Quantity", Value::Int(2)),
            ]),
            record(vec![
                ("Sku", Value::String("B".into())),
                ("Quantity", Value::Int(3)),
            ]),
        ]),
    )]);
    let catalog = Instance::Group(vec![(
        "Product".into(),
        Instance::Repeated(vec![
            record(vec![
                ("Sku", Value::String("A".into())),
                ("Price", Value::Float(10.0)),
            ]),
            record(vec![
                ("Sku", Value::String("B".into())),
                ("Price", Value::Float(5.0)),
            ]),
        ]),
    )]);
    let catalog_name = imported.project.extra_sources[0].name.clone();
    let output =
        engine::run_with_sources(&imported.project, &source, vec![(catalog_name, catalog)])?;
    let rows = output
        .field("Row")
        .and_then(Instance::as_repeated)
        .ok_or("output rows are absent")?;
    assert_eq!(rows.len(), 2);
    let totals = rows
        .iter()
        .filter_map(|row| row.field("Total").and_then(Instance::as_scalar))
        .collect::<Vec<_>>();
    assert_eq!(totals, vec![&Value::Float(20.0), &Value::Float(15.0)]);
    Ok(())
}

#[test]
#[ignore = "needs the local MapForce sample set; informational only"]
fn local_complete_po_keeps_root_join_aggregates_and_nested_article_values()
-> Result<(), Box<dyn std::error::Error>> {
    let samples =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/ReferenceSamples");
    let design = samples.join("CompletePO.mfd");
    if !design.is_file() {
        return Ok(());
    }
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let source_path = imported
        .project
        .source_path
        .as_deref()
        .ok_or("CompletePO has no primary source path")?;
    let source = format_xml::read(&samples.join(source_path), &imported.project.source)?;
    let extras = imported
        .project
        .extra_sources
        .iter()
        .map(|extra| {
            format_xml::read(&samples.join(&extra.path), &extra.schema)
                .map(|instance| (extra.name.clone(), instance))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let output = engine::run_with_sources(&imported.project, &source, extras)?;
    let line_items = output
        .field("LineItems")
        .and_then(|items| items.field("LineItem"))
        .and_then(Instance::as_repeated)
        .ok_or("CompletePO output has no line items")?;
    let single_prices = line_items
        .iter()
        .flat_map(|line| {
            line.field("Article")
                .and_then(Instance::as_mapped_sequence)
                .into_iter()
                .flatten()
        })
        .filter_map(|article| article.field("SinglePrice"))
        .filter_map(Instance::as_scalar)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(single_prices.len(), 2);
    assert_eq!(single_prices[0], Value::Float(34.0));
    assert!(single_prices.iter().all(|value| *value != Value::Null));
    assert!(
        imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::JoinAggregate { .. }))
    );
    Ok(())
}
