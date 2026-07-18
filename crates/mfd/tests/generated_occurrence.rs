use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, Value, XML_TEXT_FIELD};
use mapping::IterationOutput;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_generated_occurrence_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
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

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

fn write_fixture(dir: &Path) {
    write(
        &dir.join("source.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Source"><xs:complexType><xs:sequence><xs:element name="Record" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Label" type="xs:string"/><xs:element name="Base" type="xs:decimal"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.join("target.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Label" type="xs:string"/><xs:element name="Literal" type="xs:string"/><xs:element name="Price"><xs:complexType><xs:simpleContent><xs:extension base="xs:decimal"><xs:attribute name="discount" type="xs:decimal"/></xs:extension></xs:simpleContent></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    );
    write(
        &dir.join("mapping.mfd"),
        r#"<mapping version="26"><component name="map"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Source"><entry name="Record" outkey="10"><entry name="Label" outkey="11"/><entry name="Base" outkey="12"/></entry></entry></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
          <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="20"/></targets><data><constant value="0" datatype="decimal"/></data></component>
          <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="21"/></targets><data><constant value="3" datatype="decimal"/></data></component>
          <component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="22"/></targets><data><constant value="10" datatype="string"/></data></component>
          <component name="generate-sequence" library="core" kind="5"><sources><datapoint pos="0" key="23"/><datapoint pos="1" key="24"/></sources><targets><datapoint pos="0" key="30"/></targets></component>
          <component name="add" library="core" kind="5"><sources><datapoint pos="0" key="31"/><datapoint pos="1" key="32"/></sources><targets><datapoint pos="0" key="33"/></targets></component>
          <component name="multiply" library="core" kind="5"><sources><datapoint pos="0" key="34"/><datapoint pos="1" key="35"/></sources><targets><datapoint pos="0" key="36"/></targets></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Target"><entry name="Item" inpkey="40"><entry name="Label" inpkey="41"/><entry name="Literal" inpkey="44"/><entry name="Price" inpkey="42"><entry name="discount" type="attribute" inpkey="43"/></entry></entry></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
        </children><graph><edges/><vertices>
          <vertex vertexkey="10"><edges><edge vertexkey="40"/></edges></vertex>
          <vertex vertexkey="11"><edges><edge vertexkey="41"/></edges></vertex>
          <vertex vertexkey="12"><edges><edge vertexkey="32"/></edges></vertex>
          <vertex vertexkey="20"><edges><edge vertexkey="23"/></edges></vertex>
          <vertex vertexkey="21"><edges><edge vertexkey="24"/></edges></vertex>
          <vertex vertexkey="22"><edges><edge vertexkey="35"/><edge vertexkey="44"/></edges></vertex>
          <vertex vertexkey="30"><edges><edge vertexkey="31"/><edge vertexkey="34"/></edges></vertex>
          <vertex vertexkey="33"><edges><edge vertexkey="42"/></edges></vertex>
          <vertex vertexkey="36"><edges><edge vertexkey="43"/></edges></vertex>
        </vertices></graph></structure></component></mapping>"#,
    );
}

fn rewrite_mapping(dir: &Path, rewrite: impl FnOnce(String) -> String) {
    let path = dir.join("mapping.mfd");
    let mapping = std::fs::read_to_string(&path).unwrap();
    write(&path, &rewrite(mapping));
}

fn price_values(item: &Instance) -> Vec<(f64, f64)> {
    item.field("Price")
        .and_then(Instance::as_mapped_sequence)
        .unwrap()
        .iter()
        .map(|price| {
            let text = price
                .field(XML_TEXT_FIELD)
                .and_then(Instance::as_scalar)
                .and_then(number)
                .unwrap();
            let discount = price
                .field("discount")
                .and_then(Instance::as_scalar)
                .and_then(number)
                .unwrap();
            (text, discount)
        })
        .collect()
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Int(value) => Some(*value as f64),
        Value::Float(value) => Some(*value),
        _ => None,
    }
}

#[test]
fn computed_descendants_generate_mapped_text_occurrences_per_outer_item() {
    let dir = TempDir::new();
    write_fixture(&dir.0);
    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        engine::validate(&imported.project).is_empty(),
        "{:?}",
        engine::validate(&imported.project)
    );

    let item_scope = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Item")
        .unwrap();
    let price_scope = item_scope
        .children
        .iter()
        .find(|scope| scope.target_field == "Price")
        .unwrap();
    assert_eq!(
        price_scope.iteration_output,
        IterationOutput::MappedSequence
    );
    assert!(price_scope.sequence().is_some());

    let source = format_xml::from_str(
        "<Source><Record><Label>A</Label><Base>10</Base></Record><Record><Label>B</Label><Base>20</Base></Record></Source>",
        &imported.project.source,
    )
    .unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    let items = output
        .field("Item")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(items.len(), 2);
    for item in items {
        assert_eq!(
            item.field("Literal").and_then(Instance::as_scalar),
            Some(&Value::String("10".into()))
        );
    }
    assert_eq!(
        price_values(&items[0]),
        vec![(10.0, 0.0), (11.0, 10.0), (12.0, 20.0), (13.0, 30.0)]
    );
    assert_eq!(
        price_values(&items[1]),
        vec![(20.0, 0.0), (21.0, 10.0), (22.0, 20.0), (23.0, 30.0)]
    );
    let xml = format_xml::to_string(&imported.project.target, &output).unwrap();
    assert_eq!(xml.matches("<Price discount=").count(), 8);
    assert!(xml.contains("<Label>A</Label>"));
    assert!(xml.contains("<Label>B</Label>"));

    let exported = dir.0.join("roundtrip.mfd");
    assert!(
        mfd::export(&imported.project, &exported)
            .unwrap()
            .is_empty()
    );
    let design = std::fs::read_to_string(&exported).unwrap();
    assert!(design.contains("name=\"#text\""), "{design}");
    let reimported = mfd::import(&exported).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(
        engine::validate(&reimported.project).is_empty(),
        "{:?}",
        engine::validate(&reimported.project)
    );
    assert_eq!(output, engine::run(&reimported.project, &source).unwrap());
}

#[test]
fn scalar_reducer_owns_its_sequence_and_does_not_create_an_occurrence_scope() {
    let dir = TempDir::new();
    write_fixture(&dir.0);
    rewrite_mapping(&dir.0, |mapping| {
        mapping
            .replace(
                r#"<component name="add" library="core" kind="5"><sources><datapoint pos="0" key="31"/><datapoint pos="1" key="32"/></sources><targets><datapoint pos="0" key="33"/></targets></component>"#,
                r#"<component name="exists" library="core" kind="5"><sources><datapoint pos="0" key="31"/></sources><targets><datapoint pos="0" key="33"/></targets></component>"#,
            )
            .replace(
                r#"<vertex vertexkey="12"><edges><edge vertexkey="32"/></edges></vertex>"#,
                "",
            )
            .replace(
                r#"<vertex vertexkey="36"><edges><edge vertexkey="43"/></edges></vertex>"#,
                "",
            )
    });

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains(
            "sequence function `generate-sequence` is not connected to a repeating target",
        )
    }));
    let item_scope = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Item")
        .unwrap();
    assert!(item_scope.children.iter().all(|scope| {
        scope.target_field != "Price" || scope.iteration_output != IterationOutput::MappedSequence
    }));
}

#[test]
fn multiple_generated_dependencies_warn_and_do_not_guess_sequence_alignment() {
    let dir = TempDir::new();
    write_fixture(&dir.0);
    rewrite_mapping(&dir.0, |mapping| {
        mapping
            .replace(
                r#"<component name="add" library="core" kind="5"><sources><datapoint pos="0" key="31"/><datapoint pos="1" key="32"/></sources>"#,
                r#"<component name="add" library="core" kind="5"><sources><datapoint pos="0" key="31"/><datapoint pos="1" key="32"/><datapoint pos="2" key="51"/></sources>"#,
            )
            .replace(
                r#"<component name="target" library="xml" kind="14">"#,
                r#"<component name="generate-sequence" library="core" kind="5"><sources><datapoint pos="0" key="53"/><datapoint pos="1" key="54"/></sources><targets><datapoint pos="0" key="50"/></targets></component><component name="target" library="xml" kind="14">"#,
            )
            .replace(
                r#"<vertex vertexkey="30"><edges><edge vertexkey="31"/><edge vertexkey="34"/></edges></vertex>"#,
                r#"<vertex vertexkey="30"><edges><edge vertexkey="31"/><edge vertexkey="34"/></edges></vertex><vertex vertexkey="20"><edges><edge vertexkey="23"/><edge vertexkey="53"/></edges></vertex><vertex vertexkey="21"><edges><edge vertexkey="24"/><edge vertexkey="54"/></edges></vertex><vertex vertexkey="50"><edges><edge vertexkey="51"/></edges></vertex>"#,
            )
            .replace(
                r#"<vertex vertexkey="20"><edges><edge vertexkey="23"/></edges></vertex>"#,
                "",
            )
            .replace(
                r#"<vertex vertexkey="21"><edges><edge vertexkey="24"/></edges></vertex>"#,
                "",
            )
    });

    let imported = mfd::import(&dir.0.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.iter().any(|warning| {
        warning.contains("depends on multiple generated sequences; occurrence inference skipped")
    }));
    let item_scope = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Item")
        .unwrap();
    assert!(item_scope.children.iter().all(|scope| {
        scope.target_field != "Price" || scope.iteration_output != IterationOutput::MappedSequence
    }));
}
