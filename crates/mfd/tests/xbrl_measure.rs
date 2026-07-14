use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};
use mapping::{Node, NodeId, Scope};

struct TempDir(PathBuf);

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_xbrl_measure_{}_{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
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

fn write(path: &Path, contents: &str) -> std::io::Result<()> {
    std::fs::write(path, contents)
}

fn missing(description: &str) -> std::io::Error {
    std::io::Error::other(description)
}

fn binding_node(scope: &Scope, target: &str) -> Option<NodeId> {
    scope
        .bindings
        .iter()
        .find(|binding| binding.target_field == target)
        .map(|binding| binding.node)
        .or_else(|| {
            scope
                .children
                .iter()
                .find_map(|child| binding_node(child, target))
        })
}

fn scalar_field<'a>(instance: &'a Instance, wanted: &str) -> Option<&'a Value> {
    match instance {
        Instance::Scalar(_) => None,
        Instance::Group(fields) => fields.iter().find_map(|(name, value)| {
            if name == wanted {
                value.as_scalar()
            } else {
                scalar_field(value, wanted)
            }
        }),
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            items.iter().find_map(|item| scalar_field(item, wanted))
        }
    }
}

#[test]
fn xbrl_measure_helpers_retain_qname_bindings() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Currency" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    write(
        &dir.0.join("measure.mfd"),
        r#"<mapping version="26"><resources/>
  <component name="map"><structure><children>
    <component name="Source" library="xml" kind="14"><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Source">
        <entry name="Currency" outkey="10"/>
      </entry></entry></entry></root>
      <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
    </data></component>
    <component name="xbrl-measure-currency" library="xbrl" kind="5">
      <sources><datapoint pos="0" key="20"/></sources>
      <targets><datapoint pos="0" key="21"/></targets>
    </component>
    <component name="xbrl-measure-shares" library="xbrl" kind="5">
      <sources/>
      <targets><datapoint pos="0" key="22"/></targets>
    </component>
    <component name="Filing" library="xbrl" kind="27">
      <properties XSLTDefaultOutput="1"/>
      <data>
        <root><entry name="FileInstance"><entry name="document"><entry name="xbrl">
          <entry name="Unit">
            <entry name="CurrencyMeasure" inpkey="30"/>
            <entry name="SharesMeasure" inpkey="31"/>
          </entry>
        </entry></entry></entry></root>
        <xbrl schema="taxonomy.xsd" outputinstance="filing.xbrl"/>
      </data>
    </component>
  </children></structure><connections>
    <edge from="10" to="20"/>
    <edge from="21" to="30"/>
    <edge from="22" to="31"/>
  </connections></component>
</mapping>"#,
    )?;

    let imported = mfd::import(&dir.0.join("measure.mfd"))?;
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("typed external target boundary"));
    assert!(engine::validate(&imported.project).is_empty());

    let currency_node = binding_node(&imported.project.root, "CurrencyMeasure")
        .ok_or_else(|| missing("missing currency measure binding"))?;
    let shares_node = binding_node(&imported.project.root, "SharesMeasure")
        .ok_or_else(|| missing("missing shares measure binding"))?;
    assert_ne!(currency_node, shares_node);
    assert!(imported.project.graph.nodes.contains_key(&currency_node));
    assert!(imported.project.graph.nodes.contains_key(&shares_node));

    let source = Instance::Group(vec![(
        "Currency".to_string(),
        Instance::Scalar(Value::String("USD".to_string())),
    )]);
    let output = engine::run(&imported.project, &source)?;
    assert_eq!(
        scalar_field(&output, "CurrencyMeasure"),
        Some(&Value::String(
            "{http://www.xbrl.org/2003/iso4217}iso4217:USD".to_string()
        ))
    );
    assert_eq!(
        scalar_field(&output, "SharesMeasure"),
        Some(&Value::String(
            "{http://www.xbrl.org/2003/instance}xbrli:shares".to_string()
        ))
    );
    Ok(())
}

#[test]
fn malformed_xbrl_measure_pins_retain_a_null_binding() -> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Currency" type="xs:string"/>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    write(
        &dir.0.join("malformed.mfd"),
        r#"<mapping version="26"><resources/>
  <component name="map"><structure><children>
    <component name="Source" library="xml" kind="14"><data>
      <root><entry name="FileInstance"><entry name="document"><entry name="Source">
        <entry name="Currency" outkey="10"/>
      </entry></entry></entry></root>
      <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
    </data></component>
    <component name="xbrl-measure-shares" library="xbrl" kind="5">
      <sources><datapoint pos="0" key="20"/></sources>
      <targets><datapoint pos="0" key="21"/></targets>
    </component>
    <component name="Filing" library="xbrl" kind="27">
      <properties XSLTDefaultOutput="1"/>
      <data>
        <root><entry name="FileInstance"><entry name="document"><entry name="xbrl">
          <entry name="Unit"><entry name="SharesMeasure" inpkey="30"/></entry>
        </entry></entry></entry></root>
        <xbrl schema="taxonomy.xsd" outputinstance="filing.xbrl"/>
      </data>
    </component>
  </children></structure><connections>
    <edge from="10" to="20"/>
    <edge from="21" to="30"/>
  </connections></component>
</mapping>"#,
    )?;

    let imported = mfd::import(&dir.0.join("malformed.mfd"))?;
    assert_eq!(imported.warnings.len(), 2, "{:?}", imported.warnings);
    assert_eq!(
        imported
            .warnings
            .iter()
            .filter(|warning| warning.contains("typed external target boundary"))
            .count(),
        1
    );
    assert_eq!(
        imported
            .warnings
            .iter()
            .filter(|warning| warning.contains("has malformed pins; imported as Null"))
            .count(),
        1
    );
    assert!(engine::validate(&imported.project).is_empty());

    let shares_node = binding_node(&imported.project.root, "SharesMeasure")
        .ok_or_else(|| missing("missing malformed shares measure binding"))?;
    assert!(matches!(
        imported.project.graph.nodes.get(&shares_node),
        Some(Node::Const { value: Value::Null })
    ));
    let source = Instance::Group(vec![(
        "Currency".to_string(),
        Instance::Scalar(Value::String("USD".to_string())),
    )]);
    let output = engine::run(&imported.project, &source)?;
    assert_eq!(scalar_field(&output, "SharesMeasure"), Some(&Value::Null));
    Ok(())
}
