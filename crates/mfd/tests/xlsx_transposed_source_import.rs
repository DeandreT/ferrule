use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "f1278-import-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn exported(indexed: bool) -> String {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xlsx-transposed-rows.mfd");
    let mut text = std::fs::read_to_string(fixture)
        .unwrap()
        .replace(
            r#"annotation="Amount" datatype="integer""#,
            r#"annotation="Amount" datatype="double""#,
        )
        .replace(
            r#"<field1 name="Amount" type="integer"/>"#,
            r#"<field1 name="Amount" type="double"/>"#,
        );
    if !indexed {
        text = text
            .replace(r#"<entry name="n" type="attribute" outkey="104"/>"#, "")
            .replace(
                r#"<vertex vertexkey="104"><edges><edge vertexkey="204"/></edges></vertex>"#,
                "",
            );
    }
    let scratch = Scratch::new();
    let input = scratch.0.join("input.mfd");
    let output = scratch.0.join("output.mfd");
    std::fs::write(&input, text).unwrap();
    let imported = mfd::import(&input).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(mfd::export(&imported.project, &output).unwrap().is_empty());
    std::fs::read_to_string(output).unwrap()
}

fn import(text: &str) -> mfd::Imported {
    let scratch = Scratch::new();
    let path = scratch.0.join("mapping.mfd");
    write_serializer_schema(&scratch.0);
    std::fs::write(&path, text).unwrap();
    mfd::import(&path).unwrap()
}

fn write_serializer_schema(directory: &Path) {
    std::fs::write(
        directory.join("payload.schema.json"),
        r#"{"type":"object","properties":{"amount":{"type":"number"},"tag":{"type":"string"}}}"#,
    )
    .unwrap();
}

fn assert_nonexecutable(text: &str, imported: &mfd::Imported, message: &str) {
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains(message)),
        "{:?}",
        imported.warnings
    );
    assert!(!mfd::assess_import(imported).executable);
    let scratch = Scratch::new();
    let path = scratch.0.join("mapping.mfd");
    write_serializer_schema(&scratch.0);
    std::fs::write(&path, text).unwrap();
    assert!(matches!(
        mfd::import_with_profile(&path, &Default::default(), mfd::ImportProfile::Executable),
        Err(mfd::MfdError::IncompatibleImport(_))
    ));
}

fn number(node: roxmltree::Node<'_, '_>, attribute: &str) -> u32 {
    node.attribute(attribute).unwrap().parse().unwrap()
}

fn row<'a, 'i>(doc: &'a roxmltree::Document<'i>, name: &str) -> roxmltree::Node<'a, 'i> {
    doc.descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("annotation") == Some(name))
        .unwrap()
}

fn index(text: &str, name: &str) -> u32 {
    let doc = roxmltree::Document::parse(text).unwrap();
    number(
        row(&doc, name)
            .children()
            .find(|node| node.attribute("name") == Some("n"))
            .unwrap(),
        "outkey",
    )
}

fn target(text: &str, name: &str) -> u32 {
    let doc = roxmltree::Document::parse(text).unwrap();
    number(
        doc.descendants()
            .find(|node| {
                node.has_tag_name("entry")
                    && node.attribute("name") == Some(name)
                    && node.attribute("inpkey").is_some()
            })
            .unwrap(),
        "inpkey",
    )
}

fn feeds(doc: &roxmltree::Document<'_>) -> BTreeMap<u32, u32> {
    let mut result = BTreeMap::new();
    for vertex in doc.descendants().filter(|node| node.has_tag_name("vertex")) {
        let output = number(vertex, "vertexkey");
        for edge in vertex
            .descendants()
            .filter(|node| node.has_tag_name("edge"))
        {
            result.insert(number(edge, "vertexkey"), output);
        }
    }
    result
}

fn pin(component: roxmltree::Node<'_, '_>, side: &str, position: &str) -> u32 {
    number(
        component
            .children()
            .find(|node| node.has_tag_name(side))
            .unwrap()
            .children()
            .find(|node| node.has_tag_name("datapoint") && node.attribute("pos") == Some(position))
            .unwrap(),
        "key",
    )
}

/// The exported row's alignment output and its equality's driver input.
fn alignment(text: &str, name: &str) -> (u32, u32) {
    let doc = roxmltree::Document::parse(text).unwrap();
    let edges = feeds(&doc);
    let cell = number(row(&doc, name), "outkey");
    let filter = doc
        .descendants()
        .find(|node| {
            node.has_tag_name("component")
                && node.attribute("name") == Some("filter")
                && edges.get(&pin(*node, "sources", "0")) == Some(&cell)
        })
        .unwrap();
    let equality = edges[&pin(filter, "sources", "1")];
    let equal = doc
        .descendants()
        .find(|node| {
            node.has_tag_name("component")
                && node.attribute("name") == Some("equal")
                && pin(*node, "targets", "0") == equality
        })
        .unwrap();
    (pin(filter, "targets", "0"), pin(equal, "sources", "1"))
}

fn rewire(text: &mut String, updates: &[(u32, u32)]) {
    let doc = roxmltree::Document::parse(text).unwrap();
    let mut edges = feeds(&doc);
    for &(input, output) in updates {
        edges.insert(input, output);
    }
    let vertices = doc
        .descendants()
        .find(|node| node.has_tag_name("vertices"))
        .unwrap();
    let range = vertices.range();
    let mut outputs = BTreeMap::<u32, Vec<u32>>::new();
    for (input, output) in edges {
        outputs.entry(output).or_default().push(input);
    }
    let mut replacement = String::from("<vertices>");
    for (output, inputs) in outputs {
        replacement.push_str(&format!(r#"<vertex vertexkey="{output}"><edges>"#));
        for input in inputs {
            replacement.push_str(&format!(r#"<edge vertexkey="{input}"/>"#));
        }
        replacement.push_str("</edges></vertex>");
    }
    replacement.push_str("</vertices>");
    text.replace_range(range, &replacement);
}

fn append_components(text: &mut String, components: &str) {
    let doc = roxmltree::Document::parse(text).unwrap();
    let children = doc
        .descendants()
        .find(|node| node.has_tag_name("children"))
        .unwrap();
    let end = children.range().end - "</children>".len();
    text.insert_str(end, components);
}

fn workbook(extra_column: bool) -> Vec<u8> {
    let mut book = rust_xlsxwriter::Workbook::new();
    let sheet = book.add_worksheet();
    sheet.set_name("Quarterly").unwrap();
    for (column, value) in [(0, "A"), (1, "B"), (2, "C")] {
        if !extra_column || column == 0 {
            sheet.write_string(2, column, value).unwrap();
        }
    }
    sheet.write_number(6, 0, 10.0).unwrap();
    sheet.write_number(6, 2, 20.0).unwrap();
    sheet.write_number(8, 0, 1.25).unwrap();
    sheet.write_number(8, 2, 2.5).unwrap();
    book.save_to_buffer().unwrap()
}

fn records(imported: &mfd::Imported, bytes: &[u8]) -> Vec<Instance> {
    format_xlsx::from_bytes_transposed(
        bytes,
        &imported.project.source,
        Some("Quarterly"),
        &[3, 7, 9],
    )
    .unwrap()
}

fn has_binding(imported: &mfd::Imported, field: &str) -> bool {
    imported
        .project
        .root
        .bindings
        .iter()
        .any(|binding| binding.target_field == field)
}

#[test]
fn filtered_count_and_item_at_are_skipped_not_rebound_to_driver_records() {
    let bytes = workbook(false);
    for function in ["count", "item-at"] {
        for keep_direct in [false, true] {
            let mut text = exported(true);
            let (aligned, _) = alignment(&text, "Amount");
            let sequence = if function == "count" {
                r#"<datapoint/><datapoint pos="1" key="9001"/>"#
            } else {
                r#"<datapoint pos="0" key="9001"/><datapoint pos="1" key="9002"/>"#
            };
            append_components(
                &mut text,
                &format!(
                    r#"
                <component name="{function}" library="core" kind="5" uid="9000">
                <sources>{sequence}</sources><targets><datapoint pos="0" key="9000"/></targets></component>
                <component name="constant" library="core" kind="2" uid="9001">
                <targets><datapoint pos="0" key="9003"/></targets><data><constant value="2" datatype="integer"/></data></component>"#
                ),
            );
            let target = target(&text, if keep_direct { "Rate" } else { "Amount" });
            rewire(&mut text, &[(9001, aligned), (9002, 9003), (target, 9000)]);
            let imported = import(&text);
            assert_nonexecutable(&text, &imported, "sequence consumer");
            assert!(!has_binding(
                &imported,
                if keep_direct { "Rate" } else { "Amount" }
            ));
            assert!(has_binding(&imported, "Category"));
            assert_eq!(has_binding(&imported, "Amount"), keep_direct);
            let rows = records(&imported, &bytes);
            assert_eq!(rows.len(), 3);
            let cells = rows
                .iter()
                .filter_map(|row| match row.field("Amount") {
                    Some(Instance::Scalar(Value::Float(value))) => Some(*value),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(cells, [10.0, 20.0]);
            assert_eq!(
                rows[1].field("Amount"),
                Some(&Instance::Scalar(Value::Null))
            );
            // The native cell sequence has count 2 / item-at(2) 20, not the
            // normalized record sequence's count 3 / second amount Null.
            assert_eq!(cells.len(), 2);
            assert_eq!(cells[1], 20.0);
            engine::run(&imported.project, &Instance::Repeated(rows)).unwrap();
        }
    }
}

#[test]
fn later_unmarked_index_cannot_impersonate_the_first_row_driver() {
    let mut text = exported(true);
    let second = index(&text, "Amount");
    let (_, driver_input) = alignment(&text, "Rate");
    let doc = roxmltree::Document::parse(&text).unwrap();
    let node = row(&doc, "Amount")
        .children()
        .find(|node| node.attribute("name") == Some("n"))
        .unwrap();
    let range = node.range();
    let unmarked = text[range.clone()].replace(r#" ferrule-row-alignment="1""#, "");
    text.replace_range(range, &unmarked);
    rewire(&mut text, &[(driver_input, second)]);
    let imported = import(&text);
    assert_nonexecutable(&text, &imported, "first-row driver indexes");
    assert!(!has_binding(&imported, "Rate"));
    assert!(has_binding(&imported, "Amount"));

    let bytes = workbook(true);
    assert_eq!(records(&imported, &bytes).len(), 1);
    let schema = SchemaNode::group(
        "Later",
        vec![
            SchemaNode::scalar("Amount", ScalarType::Float),
            SchemaNode::scalar("Rate", ScalarType::Float),
        ],
    );
    let later =
        format_xlsx::from_bytes_transposed(&bytes, &schema, Some("Quarterly"), &[7, 9]).unwrap();
    assert_eq!(later.len(), 2);
    assert_eq!(
        later[1].field("Rate"),
        Some(&Instance::Scalar(Value::Float(2.5)))
    );
}

#[test]
fn missing_or_duplicate_indexes_do_not_establish_alignment() {
    for case in [
        "missing-driver",
        "missing-row",
        "duplicate-driver",
        "duplicate-row",
        "reused-key",
    ] {
        let mut text = exported(true);
        let first = index(&text, "Category");
        let name = if case.ends_with("driver") {
            "Category"
        } else {
            "Rate"
        };
        let doc = roxmltree::Document::parse(&text).unwrap();
        let cell = row(&doc, name);
        let node = cell
            .children()
            .find(|node| node.attribute("name") == Some("n"))
            .unwrap();
        let range = node.range();
        let replacement = if case.starts_with("missing") {
            String::new()
        } else if case == "reused-key" {
            text[range.clone()].replace(
                &format!(r#"outkey="{}""#, number(node, "outkey")),
                &format!(r#"outkey="{first}""#),
            )
        } else {
            format!(
                r#"{}<entry name="n" type="attribute" outkey="9100"/>"#,
                &text[range.clone()]
            )
        };
        text.replace_range(range, &replacement);
        let imported = import(&text);
        assert_nonexecutable(&text, &imported, "first-row driver indexes");
        assert!(!has_binding(&imported, "Rate"), "{case}");
        assert!(has_binding(&imported, "Category"), "{case}");
    }
}

#[test]
fn direct_scalar_reads_require_and_preserve_the_genuine_driver() {
    let bytes = workbook(false);
    for indexed in [false, true] {
        let text = exported(indexed);
        let imported = import(&text);
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert!(mfd::assess_import(&imported).executable);
        let output = engine::run(
            &imported.project,
            &Instance::Repeated(records(&imported, &bytes)),
        )
        .unwrap();
        let Instance::Repeated(rows) = output else {
            panic!("expected output records")
        };
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[1].field("Amount"),
            Some(&Instance::Scalar(Value::Null))
        );
        assert_eq!(
            rows[2].field("Amount"),
            Some(&Instance::Scalar(Value::Float(20.0)))
        );
    }
}

#[test]
fn dependent_functions_cannot_restore_a_skipped_sequence_result() {
    let mut text = exported(true);
    let (aligned, _) = alignment(&text, "Amount");
    let amount = target(&text, "Amount");
    append_components(
        &mut text,
        r#"
        <component name="count" library="core" kind="5" uid="9000">
        <sources><datapoint/><datapoint pos="1" key="9001"/></sources>
        <targets><datapoint pos="0" key="9000"/></targets></component>
        <component name="string" library="core" kind="5" uid="9002">
        <sources><datapoint pos="0" key="9002"/></sources>
        <targets><datapoint pos="0" key="9003"/></targets></component>"#,
    );
    rewire(&mut text, &[(9001, aligned), (9002, 9000), (amount, 9003)]);
    let imported = import(&text);
    assert_nonexecutable(&text, &imported, "sequence consumer");
    assert!(!has_binding(&imported, "Amount"));
    assert!(has_binding(&imported, "Rate"));
    engine::run(
        &imported.project,
        &Instance::Repeated(records(&imported, &workbook(false))),
    )
    .unwrap();
}

#[test]
fn direct_scalar_read_without_driver_context_is_not_absorbed() {
    let mut text = exported(true);
    let category_target = target(&text, "Category");
    let doc = roxmltree::Document::parse(&text).unwrap();
    let cell = number(row(&doc, "Category"), "outkey");
    let root = feeds(&doc)
        .into_iter()
        .find_map(|(input, output)| (output == cell && input != category_target).then_some(input))
        .unwrap();
    append_components(
        &mut text,
        r#"<component name="constant" library="core" kind="2" uid="9000">
        <targets><datapoint pos="0" key="9000"/></targets><data><constant value="1" datatype="integer"/></data></component>"#,
    );
    rewire(&mut text, &[(root, 9000)]);
    let imported = import(&text);
    assert_nonexecutable(&text, &imported, "unproved scalar context");
    assert!(!has_binding(&imported, "Amount"));
    assert!(!has_binding(&imported, "Rate"));
}

fn add_default_udf(text: &mut String) {
    append_components(
        text,
        r#"
        <component name="Fallback" library="helpers" kind="19" uid="9000"><data>
          <root><entry name="value" inpkey="9001" componentid="100"/></root>
          <root rootindex="1"><entry name="result" outkey="9000" componentid="101"/></root>
        </data></component>
        <component name="Fallback" library="helpers" kind="19" uid="9005"><data>
          <root><entry name="value" inpkey="9006" componentid="100"/></root>
          <root rootindex="1"><entry name="result" outkey="9005" componentid="101"/></root>
        </data></component>"#,
    );
    let definition = r#"
      <component name="Fallback" library="helpers" inline="1"><structure><children>
        <component name="value" library="core" uid="100" kind="6"><targets><datapoint key="1000"/></targets><data><input datatype="string"/></data></component>
        <component name="value-map" library="core" uid="102" kind="23"><sources><datapoint pos="0" key="1001"/></sources><targets><datapoint key="1002"/></targets><data><valuemap defaultValueMode="custom"><valuemapTable><entry from="10" to="MAPPED"/></valuemapTable><input name="input" type="string"/><result name="result" type="string" defaultValue="FALLBACK"/></valuemap></data></component>
        <component name="result" library="core" uid="101" kind="7"><sources><datapoint key="1003"/></sources><data><output datatype="string"/></data></component>
      </children><graph><vertices>
        <vertex vertexkey="1000"><edges><edge vertexkey="1001"/></edges></vertex>
        <vertex vertexkey="1002"><edges><edge vertexkey="1003"/></edges></vertex>
      </vertices></graph></structure></component>
    "#;
    let end = text.rfind("</mapping>").unwrap();
    text.insert_str(end, definition);
}

#[test]
fn scalar_udf_defaults_cannot_recreate_a_refused_alignment_result() {
    let mut text = exported(true).replace(
        r#"<field1 name="Amount" type="double"/>"#,
        r#"<field1 name="Amount" type="string"/>"#,
    );
    let (aligned, _) = alignment(&text, "Amount");
    let amount = target(&text, "Amount");
    let category = target(&text, "Category");
    add_default_udf(&mut text);
    // The unrelated call deliberately has no argument, proving that the
    // supported default still runs when its own result was not refused.
    rewire(&mut text, &[(amount, 9000), (category, 9005)]);
    let control = import(&text);
    assert!(control.warnings.is_empty(), "{:?}", control.warnings);
    let bytes = workbook(false);
    let Instance::Repeated(output) = engine::run(
        &control.project,
        &Instance::Repeated(records(&control, &bytes)),
    )
    .unwrap() else {
        panic!("expected rows")
    };
    assert_eq!(
        output[0].field("Amount"),
        Some(&Instance::Scalar(Value::String("FALLBACK".into())))
    );

    rewire(&mut text, &[(9001, aligned)]);
    let imported = import(&text);
    assert_nonexecutable(&text, &imported, "sequence consumer");
    assert!(!has_binding(&imported, "Amount"));
    assert!(has_binding(&imported, "Category"));
    assert!(has_binding(&imported, "Rate"));
    let Instance::Repeated(output) = engine::run(
        &imported.project,
        &Instance::Repeated(records(&imported, &bytes)),
    )
    .unwrap() else {
        panic!("expected rows")
    };
    assert!(output.iter().all(|row| row.field("Amount").is_none()));
    assert_eq!(
        output[0].field("Category"),
        Some(&Instance::Scalar(Value::String("FALLBACK".into())))
    );
    assert_eq!(
        output[0].field("Rate"),
        Some(&Instance::Scalar(Value::Float(1.25)))
    );
}

fn serializer(name: &str, output: u32, amount: u32, tag: u32) -> String {
    format!(
        r#"<component name="{name}" library="json" kind="31"><data><root>
      <entry name="FileInstance" outkey="{output}"><entry name="document"><entry name="root"><entry name="object">
        <entry name="amount" type="json-property"><entry name="number" inpkey="{amount}"/></entry>
        <entry name="tag" type="json-property"><entry name="string" inpkey="{tag}"/></entry>
      </entry></entry></entry></entry>
    </root><parameter usageKind="stringserialize"/><json schema="payload.schema.json"/></data></component>"#
    )
}

#[test]
fn json_serializers_and_downstream_functions_cannot_publish_partial_objects() {
    for downstream in [false, true] {
        let mut text = exported(true).replace(
            r#"<field1 name="Amount" type="double"/>"#,
            r#"<field1 name="Amount" type="string"/>"#,
        );
        let (aligned, _) = alignment(&text, "Amount");
        let amount = target(&text, "Amount");
        let category = target(&text, "Category");
        append_components(&mut text, &serializer("affected", 9000, 9001, 9002));
        append_components(&mut text, &serializer("unrelated", 9100, 9101, 9102));
        append_components(
            &mut text,
            r#"
          <component name="constant" library="core" kind="2"><targets><datapoint key="9003"/></targets><data><constant value="SAFE" datatype="string"/></data></component>
          <component name="constant" library="core" kind="2"><targets><datapoint key="9008"/></targets><data><constant value="7" datatype="integer"/></data></component>"#,
        );
        let result = if downstream {
            append_components(
                &mut text,
                r#"<component name="concat" library="core" kind="5"><sources><datapoint pos="0" key="9201"/><datapoint pos="1" key="9202"/></sources><targets><datapoint key="9203"/></targets></component>"#,
            );
            rewire(&mut text, &[(9201, 9000), (9202, 9003)]);
            9203
        } else {
            9000
        };
        rewire(
            &mut text,
            &[
                (amount, result),
                (category, 9100),
                (9001, 9008),
                (9002, 9003),
                (9101, 9008),
                (9102, 9003),
            ],
        );
        let control = import(&text);
        assert!(control.warnings.is_empty(), "{:?}", control.warnings);
        assert!(has_binding(&control, "Amount"));
        let bytes = workbook(false);
        let Instance::Repeated(output) = engine::run(
            &control.project,
            &Instance::Repeated(records(&control, &bytes)),
        )
        .unwrap() else {
            panic!("expected rows")
        };
        assert!(output[0].field("Amount").is_some());

        rewire(&mut text, &[(9001, aligned)]);
        let imported = import(&text);
        assert_nonexecutable(&text, &imported, "sequence consumer");
        assert!(!has_binding(&imported, "Amount"));
        assert!(has_binding(&imported, "Category"));
        assert!(has_binding(&imported, "Rate"));
        let Instance::Repeated(output) = engine::run(
            &imported.project,
            &Instance::Repeated(records(&imported, &bytes)),
        )
        .unwrap() else {
            panic!("expected rows")
        };
        assert!(output.iter().all(|row| row.field("Amount").is_none()));
        let Some(Instance::Scalar(Value::String(json))) = output[0].field("Category") else {
            panic!("unrelated serializer result was lost");
        };
        let json: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(json["amount"].as_f64(), Some(7.0));
        assert_eq!(json["tag"], "SAFE");
        assert_eq!(
            output[0].field("Rate"),
            Some(&Instance::Scalar(Value::Float(1.25)))
        );
    }
}
