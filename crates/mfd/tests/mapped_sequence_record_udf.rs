use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};
use mapping::IterationOutput;

struct TempDir(PathBuf);

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_mapped_record_udf_{}_{}",
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

fn mapping() -> &'static str {
    r#"<mapping version="26"><component name="map"><structure><children>
  <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Item" outkey="10"><entry name="Meta"><entry name="Code" outkey="11"/></entry><entry name="Qty" outkey="12"/></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
  <component name="MapItems" library="user" kind="19"><data>
    <root><entry name="Item" inpkey="30" componentid="101"/></root>
    <root rootindex="1"><entry name="Line" outkey="40" componentid="102"><entry name="Label" outkey="41"/><entry name="Doubled" outkey="42"/></entry></root>
  </data></component>
  <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Line" inpkey="20"><entry name="Label" inpkey="21"/><entry name="Doubled" inpkey="22"/></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>
  <vertex vertexkey="40"><edges><edge vertexkey="20"/></edges></vertex>
  <vertex vertexkey="41"><edges><edge vertexkey="21"/></edges></vertex>
  <vertex vertexkey="42"><edges><edge vertexkey="22"/></edges></vertex>
</vertices></graph></structure></component>
<component name="MapItems" library="user" editable="1"><structure><children>
  <component name="Item" library="xml" uid="101" kind="14"><properties UsageKind="input"/><data><root><entry name="Item" outkey="201"><entry name="Meta"><entry name="Code" outkey="202"/></entry><entry name="Qty" outkey="203"/></entry></root><document schema="source.xsd" instanceroot="{}Source/{}Item"/><parameter usageKind="input" name="Items" sequence="1"/></data></component>
  <component name="concat" library="core" uid="103" kind="5"><sources><datapoint key="204"/><datapoint key="205"/></sources><targets><datapoint key="206"/></targets></component>
  <component name="constant" library="core" uid="104" kind="2"><targets><datapoint key="207"/></targets><data><constant value="!" datatype="string"/></data></component>
  <component name="multiply" library="core" uid="105" kind="5"><sources><datapoint key="208"/><datapoint key="209"/></sources><targets><datapoint key="210"/></targets></component>
  <component name="constant" library="core" uid="106" kind="2"><targets><datapoint key="211"/></targets><data><constant value="2" datatype="integer"/></data></component>
  <component name="Line" library="xml" uid="102" kind="14"><properties UsageKind="output"/><data><root><entry name="Line" inpkey="212"><entry name="Label" inpkey="213"/><entry name="Doubled" inpkey="214"/></entry></root><document schema="target.xsd" instanceroot="{}Target/{}Line"/><parameter usageKind="output" name="Lines" sequence="1"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="201"><edges><edge vertexkey="212"/></edges></vertex>
  <vertex vertexkey="202"><edges><edge vertexkey="204"/></edges></vertex>
  <vertex vertexkey="207"><edges><edge vertexkey="205"/></edges></vertex>
  <vertex vertexkey="206"><edges><edge vertexkey="213"/></edges></vertex>
  <vertex vertexkey="203"><edges><edge vertexkey="208"/></edges></vertex>
  <vertex vertexkey="211"><edges><edge vertexkey="209"/></edges></vertex>
  <vertex vertexkey="210"><edges><edge vertexkey="214"/></edges></vertex>
</vertices></graph></structure></component></mapping>"#
}

fn setup(design: &str) -> TempDir {
    let dir = TempDir::new();
    write(
        &dir.0.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Meta"><xs:complexType><xs:sequence><xs:element name="Code" type="xs:string"/></xs:sequence></xs:complexType></xs:element><xs:element name="Qty" type="xs:integer"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.0.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Line" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Label" type="xs:string"/><xs:element name="Doubled" type="xs:integer"/></xs:sequence></xs:complexType></xs:element><xs:element name="Spare" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(&dir.0.join("mapping.mfd"), design);
    dir
}

fn add_optional_extra_field(dir: &TempDir) {
    let path = dir.0.join("target.xsd");
    let schema = std::fs::read_to_string(&path).unwrap().replace(
        r#"<xs:element name="Doubled" type="xs:integer"/>"#,
        r#"<xs:element name="Doubled" type="xs:integer"/><xs:element name="Extra" type="xs:string" minOccurs="0"/>"#,
    );
    write(&path, &schema);
}

fn target_with_extra(design: &str, prefix: &str) -> String {
    design.replacen(
        r#"<component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Line" inpkey="20"><entry name="Label" inpkey="21"/><entry name="Doubled" inpkey="22"/></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>"#,
        &format!(
            r#"{prefix}<component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Line" inpkey="20"><entry name="Label" inpkey="21"/><entry name="Doubled" inpkey="22"/><entry name="Extra" inpkey="23"/></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{{}}Target"/></data></component>"#,
        ),
        1,
    )
}

fn item(code: &str, qty: i64) -> Instance {
    Instance::Group(vec![
        (
            "Meta".to_string(),
            Instance::Group(vec![(
                "Code".to_string(),
                Instance::Scalar(Value::String(code.to_string())),
            )]),
        ),
        ("Qty".to_string(), Instance::Scalar(Value::Int(qty))),
    ])
}

fn source(items: Vec<Instance>) -> Instance {
    Instance::Group(vec![("Item".to_string(), Instance::Repeated(items))])
}

fn scalar<'a>(instance: &'a Instance, field: &str) -> &'a Value {
    instance.field(field).and_then(Instance::as_scalar).unwrap()
}

#[test]
fn mapped_sequence_record_udf_preserves_items_and_executes_scalar_expressions() {
    let dir = setup(mapping());
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let line = &imported.project.root.children[0];
    assert_eq!(line.target_field, "Line");
    assert_eq!(line.source(), Some(["Item".to_string()].as_slice()));
    assert_eq!(line.iteration_output, IterationOutput::Repeated);

    let input = source(vec![item("A", 2), item("A", 5), item("B", 1)]);
    let output = engine::run(&imported.project, &input).unwrap();
    let lines = output
        .field("Line")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(lines.len(), 3);
    assert_eq!(scalar(&lines[0], "Label"), &Value::String("A!".into()));
    assert_eq!(scalar(&lines[0], "Doubled"), &Value::Int(4));
    assert_eq!(scalar(&lines[1], "Label"), &Value::String("A!".into()));
    assert_eq!(scalar(&lines[1], "Doubled"), &Value::Int(10));
    assert_eq!(scalar(&lines[2], "Label"), &Value::String("B!".into()));

    let empty = engine::run(&imported.project, &source(Vec::new())).unwrap();
    assert!(
        empty
            .field("Line")
            .and_then(Instance::as_repeated)
            .is_some_and(<[Instance]>::is_empty)
    );

    let exported = dir.0.join("roundtrip.mfd");
    assert!(
        mfd::export(&imported.project, &exported)
            .unwrap()
            .is_empty()
    );
    let reimported = mfd::import(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    let rerun = engine::run(&reimported.project, &input).unwrap();
    let rerun_lines = rerun.field("Line").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rerun_lines.len(), lines.len());
    for (actual, expected) in rerun_lines.iter().zip(lines) {
        assert_eq!(scalar(actual, "Label"), scalar(expected, "Label"));
        assert_eq!(scalar(actual, "Doubled"), scalar(expected, "Doubled"));
    }
}

#[test]
fn mapped_sequence_record_preserves_an_ordinary_extra_scalar_binding() {
    let design = target_with_extra(
        mapping(),
        r#"<component name="constant" library="core" kind="2"><targets><datapoint key="13"/></targets><data><constant value="extra" datatype="string"/></data></component>"#,
    )
    .replacen(
        r#"<vertex vertexkey="42"><edges><edge vertexkey="22"/></edges></vertex>"#,
        r#"<vertex vertexkey="42"><edges><edge vertexkey="22"/></edges></vertex><vertex vertexkey="13"><edges><edge vertexkey="23"/></edges></vertex>"#,
        1,
    );
    let dir = setup(&design);
    add_optional_extra_field(&dir);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let output = engine::run(&imported.project, &source(vec![item("A", 2)])).unwrap();
    let lines = output
        .field("Line")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(scalar(&lines[0], "Extra"), &Value::String("extra".into()));
}

#[test]
fn mapped_sequence_record_rejects_a_descendant_owned_by_another_call() {
    let extra_call = r#"<component name="MapExtra" library="user" kind="19"><data>
    <root><entry name="Item" inpkey="31" componentid="301"/></root>
    <root rootindex="1"><entry name="Line" outkey="50" componentid="302"><entry name="Extra" outkey="51"/></entry></root>
  </data></component>"#;
    let extra_definition = r#"<component name="MapExtra" library="user" editable="1"><structure><children>
  <component name="Item" library="xml" uid="301" kind="14"><properties UsageKind="input"/><data><root><entry name="Item" outkey="401"><entry name="Qty" outkey="402"/></entry></root><document schema="source.xsd" instanceroot="{}Source/{}Item"/><parameter usageKind="input" name="Items" sequence="1"/></data></component>
  <component name="Line" library="xml" uid="302" kind="14"><properties UsageKind="output"/><data><root><entry name="Line" inpkey="403"><entry name="Extra" inpkey="404"/></entry></root><document schema="target.xsd" instanceroot="{}Target/{}Line"/><parameter usageKind="output" name="Lines" sequence="1"/></data></component>
</children><graph><vertices>
  <vertex vertexkey="401"><edges><edge vertexkey="403"/></edges></vertex>
  <vertex vertexkey="402"><edges><edge vertexkey="404"/></edges></vertex>
</vertices></graph></structure></component>"#;
    let design = target_with_extra(mapping(), extra_call)
        .replacen(
            r#"<vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>"#,
            r#"<vertex vertexkey="10"><edges><edge vertexkey="30"/><edge vertexkey="31"/></edges></vertex>"#,
            1,
        )
        .replacen(
            r#"<vertex vertexkey="42"><edges><edge vertexkey="22"/></edges></vertex>"#,
            r#"<vertex vertexkey="42"><edges><edge vertexkey="22"/></edges></vertex><vertex vertexkey="51"><edges><edge vertexkey="23"/></edges></vertex>"#,
            1,
        )
        .replacen(
            "</mapping>",
            &format!("{extra_definition}</mapping>"),
            1,
        );
    let dir = setup(&design);
    add_optional_extra_field(&dir);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();

    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("iteration into `Line` comes from an unsupported feed")),
        "{:?}",
        imported.warnings
    );
    assert!(imported.project.root.children.is_empty());
}

#[test]
fn mapped_sequence_record_requires_its_structural_edge() {
    let design = mapping().replace(
        "<vertex vertexkey=\"201\"><edges><edge vertexkey=\"212\"/></edges></vertex>",
        "",
    );
    let dir = setup(&design);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(
        imported.warnings.iter().any(|warning| warning.contains(
            "mapped sequence record requires exactly one direct collection-to-record edge"
        )),
        "{:?}",
        imported.warnings
    );
    assert!(imported.project.root.children.is_empty());
}

#[test]
fn mapped_sequence_record_rejects_sequence_operations() {
    let dir = setup(&mapping().replace("name=\"multiply\"", "name=\"sum\""));
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(
        imported.warnings.iter().any(|warning| warning
            .contains("mapped sequence record uses unsupported sequence operation `sum`")),
        "{:?}",
        imported.warnings
    );
    assert!(imported.project.root.children.is_empty());
}

#[test]
fn mapped_sequence_record_rejects_multiple_output_records() {
    let design = mapping()
        .replace(
            r#"<root><entry name="Line" inpkey="212"><entry name="Label" inpkey="213"/><entry name="Doubled" inpkey="214"/></entry></root>"#,
            r#"<root><entry name="Target"><entry name="Line" inpkey="212"><entry name="Label" inpkey="213"/><entry name="Doubled" inpkey="214"/></entry><entry name="Spare" inpkey="215"><entry name="Value" inpkey="216"/></entry></entry></root>"#,
        )
        .replace(
            r#"<document schema="target.xsd" instanceroot="{}Target/{}Line"/>"#,
            r#"<document schema="target.xsd" instanceroot="{}Target"/>"#,
        );
    let dir = setup(&design);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(
        imported.warnings.iter().any(|warning| warning.contains(
            "mapped sequence record requires one input collection and one output record"
        )),
        "{:?}",
        imported.warnings
    );
    assert!(imported.project.root.children.is_empty());
}

#[test]
fn mapped_sequence_record_rejects_nested_source_repetition() {
    let dir = setup(mapping());
    let source_schema = std::fs::read_to_string(dir.0.join("source.xsd"))
        .unwrap()
        .replace(
            r#"<xs:element name="Meta">"#,
            r#"<xs:element name="Meta" maxOccurs="unbounded">"#,
        );
    write(&dir.0.join("source.xsd"), &source_schema);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(
        imported.warnings.iter().any(|warning| warning.contains(
            "structured sequence expressions must read scalar fields without crossing a nested repetition"
        )),
        "{:?}",
        imported.warnings
    );
    assert!(imported.project.root.children.is_empty());
}

#[test]
fn mapped_sequence_record_requires_one_caller_collection_input() {
    let design = mapping()
        .replace(
            r#"<root><entry name="Item" inpkey="30" componentid="101"/></root>"#,
            r#"<root><entry name="Item" inpkey="30" componentid="101"><entry name="Duplicate" inpkey="31"/></entry></root>"#,
        )
        .replace(
            r#"<vertex vertexkey="10"><edges><edge vertexkey="30"/></edges></vertex>"#,
            r#"<vertex vertexkey="10"><edges><edge vertexkey="30"/><edge vertexkey="31"/></edges></vertex>"#,
        );
    let dir = setup(&design);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(
        imported.warnings.iter().any(|warning| warning
            .contains("its mapped sequence parameter must have one collection input")),
        "{:?}",
        imported.warnings
    );
    assert!(imported.project.root.children.is_empty());
}

#[test]
fn mapped_sequence_record_rejects_a_descendant_as_its_public_input() {
    let design = mapping().replace(
        r#"<root><entry name="Item" inpkey="30" componentid="101"/></root>"#,
        r#"<root><entry name="Item" componentid="101"><entry name="Wrong" inpkey="30"/></entry></root>"#,
    );
    let dir = setup(&design);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("input is not the public collection port")),
        "{:?}",
        imported.warnings
    );
    assert!(imported.project.root.children.is_empty());
}

#[test]
fn mapped_sequence_record_rejects_a_descendant_as_its_public_output() {
    let design = mapping().replace(
        r#"<root rootindex="1"><entry name="Line" outkey="40" componentid="102"><entry name="Label" outkey="41"/><entry name="Doubled" outkey="42"/></entry></root>"#,
        r#"<root rootindex="1"><entry name="Line" componentid="102"><entry name="Wrong" outkey="40"/><entry name="Label" outkey="41"/><entry name="Doubled" outkey="42"/></entry></root>"#,
    );
    let dir = setup(&design);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("iteration into `Line` comes from an unsupported feed")),
        "{:?}",
        imported.warnings
    );
    assert!(imported.project.root.children.is_empty());
}

#[test]
fn mapped_sequence_record_executes_into_a_database_root_scope() {
    let design = mapping().replace(
        r#"<component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Line" inpkey="20"><entry name="Label" inpkey="21"/><entry name="Doubled" inpkey="22"/></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>"#,
        r#"<component name="target" library="db" kind="15"><data><root><entry name="document"><entry name="Line" type="table" inpkey="20"><entry name="Label" inpkey="21"/><entry name="Doubled" inpkey="22"/></entry></entry></root><database ref="missing"/></data></component>"#,
    );
    let dir = setup(&design);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(imported.warnings[0].contains("has no resolvable connection"));
    assert!(engine::validate(&imported.project).is_empty());
    assert!(imported.project.target.repeating);
    assert_eq!(
        imported.project.root.source(),
        Some(["Item".to_string()].as_slice())
    );
    assert_eq!(
        imported.project.root.iteration_output,
        IterationOutput::Repeated
    );

    let output = engine::run(&imported.project, &source(vec![item("C", 3)])).unwrap();
    let rows = output.as_repeated().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(scalar(&rows[0], "Label"), &Value::String("C!".into()));
    assert_eq!(scalar(&rows[0], "Doubled"), &Value::Int(6));
}
