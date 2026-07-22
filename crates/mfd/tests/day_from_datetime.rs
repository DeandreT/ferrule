use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};
use mapping::Node;

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_day_from_datetime_{}_{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        if let Err(error) = std::fs::create_dir_all(&path) {
            panic!("failed to create test directory: {error}");
        }
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) {
    if let Err(error) = std::fs::write(path, contents) {
        panic!("failed to write {}: {error}", path.display());
    }
}

fn setup() -> TempDir {
    let dir = TempDir::new();
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Dates"><xs:complexType><xs:sequence>
    <xs:element name="Date" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="At" type="xs:dateTime"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Checks"><xs:complexType><xs:sequence>
    <xs:element name="Check" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="BeginningOfFinancialYear" type="xs:boolean"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26">
  <component name="main"><structure><children>
    <component name="Dates" library="xml" uid="1" kind="14"><data>
      <root><entry name="Dates"><entry name="Date" outkey="10"><entry name="At" outkey="11"/></entry></entry></root>
      <document schema="source.xsd" instanceroot="{}Dates"/>
    </data></component>
    <component name="IsBeginningOfFinancialYear" library="user" uid="2" kind="19"><data>
      <root><entry name="At" inpkey="20" componentid="100"/></root>
      <root rootindex="1"><entry name="Result" outkey="21" componentid="101"/></root>
    </data></component>
    <component name="Checks" library="xml" uid="3" kind="14"><data>
      <root><entry name="Checks"><entry name="Check" inpkey="30"><entry name="BeginningOfFinancialYear" inpkey="31"/></entry></entry></root>
      <document schema="target.xsd" instanceroot="{}Checks"/>
    </data></component>
  </children><graph><vertices>
    <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
    <vertex vertexkey="11"><edges><edge vertexkey="20"/></edges></vertex>
    <vertex vertexkey="21"><edges><edge vertexkey="31"/></edges></vertex>
  </vertices></graph></structure></component>
  <component name="IsBeginningOfFinancialYear" library="user" inline="1"><structure><children>
    <component name="At" library="core" uid="100" kind="6">
      <targets><datapoint pos="0" key="1000"/></targets><data><input datatype="dateTime"/></data>
    </component>
    <component name="month-from-datetime" library="lang" uid="102" kind="5">
      <sources><datapoint pos="0" key="1001"/></sources><targets><datapoint pos="0" key="1002"/></targets>
    </component>
    <component name="month" library="core" uid="103" kind="2">
      <targets><datapoint pos="0" key="1003"/></targets><data><constant value="12" datatype="integer"/></data>
    </component>
    <component name="equal" library="core" uid="104" kind="5">
      <sources><datapoint pos="0" key="1004"/><datapoint pos="1" key="1005"/></sources><targets><datapoint pos="0" key="1006"/></targets>
    </component>
    <component name="day-from-datetime" library="lang" uid="105" kind="5">
      <sources><datapoint pos="0" key="1007"/></sources><targets><datapoint pos="0" key="1008"/></targets>
    </component>
    <component name="day" library="core" uid="106" kind="2">
      <targets><datapoint pos="0" key="1009"/></targets><data><constant value="1" datatype="integer"/></data>
    </component>
    <component name="equal" library="core" uid="107" kind="5">
      <sources><datapoint pos="0" key="1010"/><datapoint pos="1" key="1011"/></sources><targets><datapoint pos="0" key="1012"/></targets>
    </component>
    <component name="logical-and" library="core" uid="108" kind="5">
      <sources><datapoint pos="0" key="1013"/><datapoint pos="1" key="1014"/></sources><targets><datapoint pos="0" key="1015"/></targets>
    </component>
    <component name="Result" library="core" uid="101" kind="7">
      <sources><datapoint pos="0" key="1016"/></sources><data><output datatype="boolean"/></data>
    </component>
  </children><graph><vertices>
    <vertex vertexkey="1000"><edges><edge vertexkey="1001"/><edge vertexkey="1007"/></edges></vertex>
    <vertex vertexkey="1002"><edges><edge vertexkey="1004"/></edges></vertex>
    <vertex vertexkey="1003"><edges><edge vertexkey="1005"/></edges></vertex>
    <vertex vertexkey="1006"><edges><edge vertexkey="1013"/></edges></vertex>
    <vertex vertexkey="1008"><edges><edge vertexkey="1010"/></edges></vertex>
    <vertex vertexkey="1009"><edges><edge vertexkey="1011"/></edges></vertex>
    <vertex vertexkey="1012"><edges><edge vertexkey="1014"/></edges></vertex>
    <vertex vertexkey="1015"><edges><edge vertexkey="1016"/></edges></vertex>
  </vertices></graph></structure></component>
</mapping>"#,
    );
    dir
}

fn result(instance: &Instance) -> &Value {
    let Some(value) = instance
        .field("BeginningOfFinancialYear")
        .and_then(Instance::as_scalar)
    else {
        panic!("missing boolean result field");
    };
    value
}

#[test]
fn scalar_datetime_component_udf_imports_and_executes() {
    let dir = setup();
    let imported = match mfd::import(&dir.0.join("mapping.mfd")) {
        Ok(imported) => imported,
        Err(error) => panic!("failed to import self-authored mapping: {error}"),
    };
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let validation = engine::validate(&imported.project);
    assert!(validation.is_empty(), "{validation:?}");
    assert!(
        imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::UserFunctionCall { .. }))
    );
    for function in ["month_from_datetime", "day_from_datetime"] {
        assert!(imported.project.user_functions.values().any(|definition| {
            definition.body.nodes.values().any(|node| {
                matches!(node, Node::Call { function: imported, .. } if imported == function)
            })
        }));
    }

    let source = match format_xml::from_str(
        "<Dates><Date><At>2024-12-01T23:30:00-05:00</At></Date><Date><At>2024-12-02T00:30:00+14:00</At></Date><Date><At>2024-11-01T00:00:00Z</At></Date></Dates>",
        &imported.project.source,
    ) {
        Ok(source) => source,
        Err(error) => panic!("failed to parse test source: {error}"),
    };
    let target = match engine::run(&imported.project, &source) {
        Ok(target) => target,
        Err(error) => panic!("failed to execute imported mapping: {error}"),
    };
    let Some(checks) = target.field("Check").and_then(Instance::as_repeated) else {
        panic!("missing repeated Check output");
    };
    assert_eq!(checks.len(), 3);
    assert_eq!(result(&checks[0]), &Value::Bool(true));
    assert_eq!(result(&checks[1]), &Value::Bool(false));
    assert_eq!(result(&checks[2]), &Value::Bool(false));
}
