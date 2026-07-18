use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};
use mapping::Node;

struct TempDir(PathBuf);

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_target_node_functions_{}_{}",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
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

fn setup() -> TempDir {
    let directory = TempDir::new();
    std::fs::write(
        directory.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:element name="Source"><xs:complexType><xs:sequence>
            <xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence>
              <xs:element name="Raw" type="xs:decimal"/>
            </xs:sequence></xs:complexType></xs:element>
          </xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        directory.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:simpleType name="Amount"><xs:restriction base="xs:decimal"><xs:fractionDigits value="2"/></xs:restriction></xs:simpleType>
          <xs:complexType name="ResultItem"><xs:sequence><xs:element name="Amount" type="Amount"/></xs:sequence></xs:complexType>
          <xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Item" type="ResultItem" maxOccurs="unbounded"/></xs:sequence></xs:complexType></xs:element>
        </xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        directory.0.join("mapping.mfd"),
        r#"<mapping version="31">
          <component name="map"><structure><children>
            <component name="source" library="xml" kind="14"><data>
              <root><entry name="Source"><entry name="Item" outkey="1"><entry name="Raw" outkey="2"/></entry></entry></root>
              <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
            </data></component>
            <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
              <root><entry name="Target"><inputnodefunctions><rule applyto="descendants"><function name="round-target"/><filter datatype="numeric"/></rule></inputnodefunctions><entry name="Item" inpkey="11"><entry name="Amount" inpkey="12"/></entry></entry></root>
              <document schema="target.xsd" instanceroot="{}Target"/>
            </data></component>
          </children><graph><vertices>
            <vertex vertexkey="1"><edges><edge vertexkey="11"/></edges></vertex>
            <vertex vertexkey="2"><edges><edge vertexkey="12"/></edges></vertex>
          </vertices></graph></structure></component>
          <component name="round-target" library="mapforce_nodefunction"><structure><children>
            <component name="zero" library="core" uid="200" kind="2"><targets><datapoint pos="0" key="20"/></targets><data><constant value="0" datatype="integer"/></data></component>
            <component name="substitute-missing" library="core" uid="201" kind="5"><sources><datapoint pos="0" key="21"/><datapoint pos="1" key="22"/></sources><targets><datapoint pos="0" key="23"/></targets></component>
            <component name="round-helper" library="user" uid="202" kind="19"><data>
              <root><entry name="raw" inpkey="24" componentid="101"/><entry name="precision" inpkey="25" componentid="102"/></root>
              <root rootindex="1"><entry name="result" outkey="26" componentid="103"/></root>
            </data></component>
            <component name="result" library="core" uid="203" kind="7"><sources><datapoint pos="0" key="27"/></sources><data><output datatype="anySimpleType"/><parameter usageKind="output" name="result"/></data></component>
            <component name="node_fractionDigits" library="core" uid="204" kind="6"><targets><datapoint pos="0" key="28"/></targets><data><input datatype="int"/><parameter usageKind="input" name="node_fractionDigits"/></data></component>
            <component name="raw_value" library="core" uid="205" kind="6"><targets><datapoint pos="0" key="29"/></targets><data><input datatype="anySimpleType"/><parameter usageKind="input" name="raw_value"/></data></component>
          </children></structure><connections>
            <edge from="26" to="27"/><edge from="29" to="24"/><edge from="23" to="25"/><edge from="28" to="21"/><edge from="20" to="22"/>
          </connections></component>
          <component name="round-helper" library="user"><structure><children>
            <component name="raw" library="core" uid="101" kind="6"><targets><datapoint pos="0" key="40"/></targets><data><input datatype="decimal"/><parameter usageKind="input" name="raw"/></data></component>
            <component name="precision" library="core" uid="102" kind="6"><targets><datapoint pos="0" key="41"/></targets><data><input datatype="integer"/><parameter usageKind="input" name="precision"/></data></component>
            <component name="round-precision" library="core" uid="104" kind="5"><sources><datapoint pos="0" key="42"/><datapoint pos="1" key="43"/></sources><targets><datapoint pos="0" key="44"/></targets></component>
            <component name="result" library="core" uid="103" kind="7"><sources><datapoint pos="0" key="45"/></sources><data><output datatype="decimal"/><parameter usageKind="output" name="result"/></data></component>
          </children></structure><connections>
            <edge from="40" to="42"/><edge from="41" to="43"/><edge from="44" to="45"/>
          </connections></component>
        </mapping>"#,
    )
    .unwrap();
    directory
}

#[test]
fn target_descendant_rule_inlines_nested_scalar_udf_and_uses_fraction_digits() {
    let directory = setup();
    let imported = mfd::import(&directory.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let item = &imported.project.root.children[0];
    let amount = item
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Amount")
        .unwrap();
    assert!(
        matches!(
            imported.project.graph.nodes.get(&amount.node),
            Some(Node::Call { function, .. }) if function == "round"
        ),
        "{:?}",
        imported.project.graph.nodes.get(&amount.node)
    );

    let source = Instance::Group(vec![(
        "Item".into(),
        Instance::Repeated(vec![Instance::Group(vec![(
            "Raw".into(),
            Instance::Scalar(Value::Float(1.235)),
        )])]),
    )]);
    let output = engine::run(&imported.project, &source).unwrap();
    let items = output.field("Item").unwrap().as_repeated().unwrap();
    assert_eq!(
        items[0].field("Amount").and_then(Instance::as_scalar),
        Some(&Value::Float(1.24))
    );
}
