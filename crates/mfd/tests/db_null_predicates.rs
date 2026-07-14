use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value};
use mapping::Node;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_db_null_predicates_{}_{}",
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
  <xs:element name="Source"><xs:complexType><xs:sequence>
    <xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Name" type="xs:string"/>
      <xs:element name="Value" type="xs:string" minOccurs="0"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    std::fs::write(
        dir.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="Target"><xs:complexType><xs:sequence>
    <xs:element name="NullRow" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Name" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
    <xs:element name="PresentRow" maxOccurs="unbounded"><xs:complexType><xs:sequence>
      <xs:element name="Name" type="xs:string"/>
    </xs:sequence></xs:complexType></xs:element>
  </xs:sequence></xs:complexType></xs:element>
</xs:schema>"#,
    )?;
    let design = dir.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="Source">
      <entry name="Row" outkey="10"><entry name="Name" outkey="11"/><entry name="Value" outkey="12"/></entry>
    </entry></entry></entry></root>
    <document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/>
  </data></component>
  <component name="is-null" library="db" kind="5">
    <sources><datapoint pos="0" key="20"/></sources>
    <targets><datapoint pos="0" key="21"/></targets>
  </component>
  <component name="is-not-null" library="db" kind="5">
    <sources><datapoint pos="0" key="22"/></sources>
    <targets><datapoint pos="0" key="23"/></targets>
  </component>
  <component name="null rows" library="core" kind="3">
    <sources><datapoint pos="0" key="24"/><datapoint pos="1" key="25"/></sources>
    <targets><datapoint pos="0" key="26"/></targets>
  </component>
  <component name="present rows" library="core" kind="3">
    <sources><datapoint pos="0" key="27"/><datapoint pos="1" key="28"/></sources>
    <targets><datapoint pos="0" key="29"/></targets>
  </component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
    <root><entry name="FileInstance"><entry name="document"><entry name="Target">
      <entry name="NullRow" inpkey="30"><entry name="Name" inpkey="31"/></entry>
      <entry name="PresentRow" inpkey="32"><entry name="Name" inpkey="33"/></entry>
    </entry></entry></entry></root>
    <document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/>
  </data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="24"/><edge vertexkey="27"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="31"/><edge vertexkey="33"/></edges></vertex>
  <vertex vertexkey="12"><edges><edge vertexkey="20"/><edge vertexkey="22"/></edges></vertex>
  <vertex vertexkey="21"><edges><edge vertexkey="25"/></edges></vertex>
  <vertex vertexkey="23"><edges><edge vertexkey="28"/></edges></vertex>
  <vertex vertexkey="26"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="29"><edges><edge vertexkey="32"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    )?;
    Ok(design)
}

fn child_scope<'a>(project: &'a mapping::Project, target: &str) -> Option<&'a mapping::Scope> {
    project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == target)
}

fn repeated_names<'a>(output: &'a Instance, field: &str) -> Option<Vec<&'a str>> {
    output
        .field(field)?
        .as_repeated()?
        .iter()
        .map(|item| match item.field("Name")?.as_scalar()? {
            Value::String(name) => Some(name.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn db_null_predicates_import_as_filtered_scalar_expressions() -> TestResult {
    let dir = TempDir::new()?;
    let design = write_fixture(&dir.0)?;
    let imported = mfd::import(&design)?;

    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let validation = engine::validate(&imported.project);
    assert!(validation.is_empty(), "{validation:?}");

    let Some(null_scope) = child_scope(&imported.project, "NullRow") else {
        panic!("missing NullRow scope");
    };
    let Some(null_filter) = null_scope
        .filter
        .and_then(|node| imported.project.graph.nodes.get(&node))
    else {
        panic!("missing NullRow filter");
    };
    let Node::Call {
        function: null_function,
        args: null_args,
    } = null_filter
    else {
        panic!("is-null did not lower to a call");
    };
    assert_eq!(null_function, "not");
    let [exists_id] = null_args.as_slice() else {
        panic!("is-null wrapper must have one argument");
    };
    assert!(matches!(
        imported.project.graph.nodes.get(exists_id),
        Some(Node::Call { function, args }) if function == "exists" && args.len() == 1
    ));

    let Some(present_scope) = child_scope(&imported.project, "PresentRow") else {
        panic!("missing PresentRow scope");
    };
    assert!(matches!(
        present_scope
            .filter
            .and_then(|node| imported.project.graph.nodes.get(&node)),
        Some(Node::Call { function, args }) if function == "exists" && args.len() == 1
    ));

    let source = format_xml::from_str(
        "<Source><Row><Name>absent</Name></Row><Row><Name>empty</Name><Value/></Row><Row><Name>full</Name><Value>set</Value></Row></Source>",
        &imported.project.source,
    )?;
    let output = engine::run(&imported.project, &source)?;
    assert_eq!(repeated_names(&output, "NullRow"), Some(vec!["absent"]));
    assert_eq!(
        repeated_names(&output, "PresentRow"),
        Some(vec!["empty", "full"])
    );
    assert_eq!(
        source
            .field("Row")
            .and_then(Instance::as_repeated)
            .and_then(|rows| rows.get(1))
            .and_then(|row| row.field("Value"))
            .and_then(Instance::as_scalar),
        Some(&Value::String(String::new()))
    );
    Ok(())
}
