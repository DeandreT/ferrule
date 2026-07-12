use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};
use mapping::Node;

struct TempDir(PathBuf);

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_aggregate_udf_{}_{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
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

fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
}

fn setup() -> TempDir {
    let dir = TempDir::new();
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Readings"><xs:complexType><xs:sequence><xs:element name="Reading" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Bucket" type="xs:string"/><xs:element name="Value" type="xs:decimal"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Report"><xs:complexType><xs:sequence><xs:element name="Summary" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Label" type="xs:string"/><xs:element name="Minimum" type="xs:decimal"/><xs:element name="Maximum" type="xs:decimal"/><xs:element name="Average" type="xs:decimal"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data><root><entry name="Readings"><entry name="Reading" outkey="10"><entry name="Bucket" outkey="11"/><entry name="Value" outkey="12"/></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Readings"/></data></component>
  <component name="constant" library="core" kind="2"><targets><datapoint key="60"/></targets><data><constant value="A" datatype="string"/></data></component>
  <component name="equal" library="core" kind="5"><sources><datapoint key="52"/><datapoint key="53"/></sources><targets><datapoint key="61"/></targets></component>
  <component name="filter" library="core" kind="3"><sources><datapoint key="50"/><datapoint key="51"/></sources><targets><datapoint key="54"/><datapoint/></targets></component>
  <component name="Summarize" library="user" kind="19"><data>
    <root><entry name="Readings" componentid="101"><entry name="Reading" inpkey="30"/></entry></root>
    <root rootindex="1"><entry name="Summary" componentid="102" outkey="40"><entry name="Minimum" outkey="41"/><entry name="Maximum" outkey="42"/><entry name="Average" outkey="43"/></entry></root>
  </data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Report"><entry name="Summary" inpkey="20"><entry name="Label" inpkey="24"/><entry name="Minimum" inpkey="21"/><entry name="Maximum" inpkey="22"/><entry name="Average" inpkey="23"/></entry></entry></root><document schema="target.xsd" outputinstance="out.xml" instanceroot="{}Report"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="50"/></edges></vertex>
  <vertex vertexkey="11"><edges><edge vertexkey="52"/></edges></vertex>
  <vertex vertexkey="60"><edges><edge vertexkey="53"/><edge vertexkey="24"/></edges></vertex>
  <vertex vertexkey="61"><edges><edge vertexkey="51"/></edges></vertex>
  <vertex vertexkey="54"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="40"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="41"><edges><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="42"><edges><edge vertexkey="22"/></edges></vertex>
  <vertex vertexkey="43"><edges><edge vertexkey="23"/></edges></vertex>
</vertices></graph></structure></component>
<component name="Summarize" library="user" editable="1"><structure><children>
  <component name="min" library="core" uid="103" kind="5"><sources><datapoint/><datapoint pos="1" key="70"/></sources><targets><datapoint key="71"/></targets></component>
  <component name="max" library="core" uid="104" kind="5"><sources><datapoint/><datapoint pos="1" key="72"/></sources><targets><datapoint key="73"/></targets></component>
  <component name="avg" library="core" uid="105" kind="5"><sources><datapoint/><datapoint pos="1" key="74"/></sources><targets><datapoint key="75"/></targets></component>
  <component name="Reading" library="xml" uid="101" kind="14"><properties UsageKind="input"/><data><root><entry name="Reading"><entry name="Value" outkey="76"/></entry></root><document schema="source.xsd" instanceroot="{}Readings/{}Reading"/><parameter usageKind="input" name="Readings" sequence="1"/></data></component>
  <component name="Summary" library="xml" uid="102" kind="14"><properties UsageKind="output"/><data><root><entry name="Summary"><entry name="Minimum" inpkey="77"/><entry name="Maximum" inpkey="78"/><entry name="Average" inpkey="79"/></entry></root><document schema="target.xsd" instanceroot="{}Report/{}Summary"/><parameter usageKind="output" name="Summary"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="76"><edges><edge vertexkey="70"/><edge vertexkey="72"/><edge vertexkey="74"/></edges></vertex>
  <vertex vertexkey="71"><edges><edge vertexkey="77"/></edges></vertex>
  <vertex vertexkey="73"><edges><edge vertexkey="78"/></edges></vertex>
  <vertex vertexkey="75"><edges><edge vertexkey="79"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#,
    );
    dir
}

fn reading(bucket: &str, value: f64) -> Instance {
    Instance::Group(vec![
        (
            "Bucket".into(),
            Instance::Scalar(Value::String(bucket.into())),
        ),
        ("Value".into(), Instance::Scalar(Value::Float(value))),
    ])
}

fn source(items: Vec<Instance>) -> Instance {
    Instance::Group(vec![("Reading".into(), Instance::Repeated(items))])
}

fn scalar<'a>(instance: &'a Instance, field: &str) -> &'a Value {
    instance.field(field).and_then(Instance::as_scalar).unwrap()
}

#[test]
fn aggregate_record_udf_imports_filters_executes_and_roundtrips() {
    let dir = setup();
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let summary = &imported.project.root.children[0];
    assert_eq!(summary.target_field, "Summary");
    assert!(summary.source().is_none());
    assert!(matches!(
        summary.sequence(),
        Some(mapping::SequenceExpr::Generate { .. })
    ));
    assert_eq!(summary.bindings.len(), 4);
    assert_eq!(
        imported
            .project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(
                node,
                Node::Aggregate {
                    expression: Some(_),
                    ..
                }
            ))
            .count(),
        3
    );

    let input = source(vec![
        reading("A", 2.0),
        reading("B", 100.0),
        reading("A", 8.0),
    ]);
    let output = engine::run(&imported.project, &input).unwrap();
    let summaries = output
        .field("Summary")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(scalar(&summaries[0], "Label"), &Value::String("A".into()));
    assert_eq!(scalar(&summaries[0], "Minimum"), &Value::Float(2.0));
    assert_eq!(scalar(&summaries[0], "Maximum"), &Value::Float(8.0));
    assert_eq!(scalar(&summaries[0], "Average"), &Value::Float(5.0));

    let empty = engine::run(&imported.project, &source(Vec::new())).unwrap();
    let empty_summary = &empty
        .field("Summary")
        .and_then(Instance::as_repeated)
        .unwrap()[0];
    for field in ["Minimum", "Maximum", "Average"] {
        assert_eq!(scalar(empty_summary, field), &Value::Null);
    }

    let exported = dir.0.join("roundtrip.mfd");
    let warnings = mfd::export(&imported.project, &exported).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let reimported = mfd::import(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let rerun = engine::run(&reimported.project, &input).unwrap();
    let rerun_summary = &rerun
        .field("Summary")
        .and_then(Instance::as_repeated)
        .unwrap()[0];
    assert_eq!(scalar(rerun_summary, "Label"), &Value::String("A".into()));
    assert_eq!(scalar(rerun_summary, "Minimum"), &Value::Float(2.0));
    assert_eq!(scalar(rerun_summary, "Maximum"), &Value::Float(8.0));
    assert_eq!(scalar(rerun_summary, "Average"), &Value::Float(5.0));
}

#[test]
fn unsupported_aggregate_record_shape_keeps_one_actionable_udf_warning() {
    let dir = setup();
    let path = dir.0.join("mapping.mfd");
    let design = std::fs::read_to_string(&path)
        .unwrap()
        .replace("name=\"min\"", "name=\"count\"");
    write(&path, &design);

    let imported = mfd::import(&path).unwrap();
    assert!(
        imported.warnings.iter().any(|warning| {
            warning.contains("skipped user-defined function `Summarize`")
                && warning.contains("unsupported component `count`")
        }),
        "{:?}",
        imported.warnings
    );
    assert!(
        imported
            .project
            .graph
            .nodes
            .values()
            .all(|node| !matches!(node, Node::Aggregate { .. }))
    );
}
