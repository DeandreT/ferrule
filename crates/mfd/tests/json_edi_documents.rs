use ir::{Instance, Value};
use std::path::PathBuf;

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(n, v)| (n.to_string(), v))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn repeated(items: Vec<Instance>) -> Instance {
    Instance::Repeated(items)
}
fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn field(values: &[&str]) -> Instance {
    repeated(
        values
            .iter()
            .map(|v| group(vec![("Value", text(v))]))
            .collect(),
    )
}
fn row(name: &[&str], qty: &[&str]) -> Instance {
    group(vec![("name", field(name)), ("qty", field(qty))])
}
fn document(name: &str, rows: Vec<Instance>) -> Instance {
    group(vec![
        ("Path", text(name)),
        (
            "Root",
            repeated(vec![group(vec![(
                "rows",
                repeated(vec![group(vec![("Items", repeated(rows))])]),
            )])]),
        ),
    ])
}
fn project() -> mfd::ImportedJsonEdiDocument {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/json_edi_documents");
    mfd::import_json_edi_document_with_options(
        &root.join("mapping.mfd"),
        &mfd::ImportOptions::default().with_package_root(&root),
    )
    .unwrap()
}
#[test]
fn repeated_documents_and_rows_never_fall_back_to_a_previous_owner() {
    let imported = project();
    let source = group(vec![(
        "Documents",
        repeated(vec![
            document(
                "one",
                vec![row(&["", "A"], &["1.50"]), row(&["B"], &["2.50"])],
            ),
            document("two", vec![row(&["C"], &["3"])]),
        ]),
    )]);
    let value = engine::run(imported.document.project(), &source).unwrap();
    let interchanges = value.field("Interchange").unwrap().as_repeated().unwrap();
    assert_eq!(interchanges.len(), 2);
    let lines = |doc: usize| {
        interchanges[doc]
            .field("Group")
            .unwrap()
            .as_repeated()
            .unwrap()[0]
            .field("Message_850")
            .unwrap()
            .as_repeated()
            .unwrap()[0]
            .field("Lines")
            .unwrap()
            .as_repeated()
            .unwrap()
    };
    assert_eq!(lines(0).len(), 2);
    assert_eq!(lines(1).len(), 1);
    for (line, expected) in [
        (&lines(0)[0], "A"),
        (&lines(0)[1], "B"),
        (&lines(1)[0], "C"),
    ] {
        assert_eq!(
            line.field("LIN").unwrap().field("F1"),
            Some(&text(expected))
        );
    }
    let filename = engine::run(&imported.output_filename, &group(vec![])).unwrap();
    assert_eq!(filename.field("Path"), Some(&text("out.edi")));
}
#[test]
fn a_later_numeric_candidate_is_evaluated_before_first_value_selection() {
    let imported = project();
    let source = group(vec![(
        "Documents",
        repeated(vec![document("one", vec![row(&["A"], &["1", "1e999"])])]),
    )]);
    assert!(engine::run(imported.document.project(), &source).is_err());
}

#[test]
fn embedded_string_definitions_keep_output_and_parameter_identities() {
    let fixtures =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/json_edi_documents");
    let directory = std::env::temp_dir().join(format!(
        "json-edi-udf-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    for name in [
        "mapping.mfd",
        "source.json",
        "Envelope.Config",
        "850.Config",
        "Defs.Segment",
    ] {
        std::fs::copy(fixtures.join(name), directory.join(name)).unwrap();
    }
    let original = std::fs::read_to_string(directory.join("mapping.mfd")).unwrap();
    let call = r#"<component name="Decorate" library="Fixture" kind="19" uid="10"><data><root><entry name="Input" inpkey="101" componentid="11"/><entry name="Text" outkey="102" componentid="12"/><entry name="Number" outkey="103" componentid="13"/></root></data></component>"#;
    let definition = r#"<component name="Decorate" library="Fixture" uid="100" inline="1"><structure><children>
<component name="Input" library="core" kind="6" uid="11"><sources><datapoint/></sources><targets><datapoint pos="0" key="1"/></targets><data><input datatype="string"/><parameter usageKind="input" name="Input"/></data></component>
<component name="Text" library="core" kind="7" uid="12"><sources><datapoint pos="0" key="2"/></sources><data><output datatype="string"/><parameter usageKind="output" name="Text"/></data></component>
<component name="Number" library="core" kind="7" uid="13"><sources><datapoint pos="0" key="8"/></sources><data><output datatype="string"/><parameter usageKind="output" name="Number"/></data></component>
<component name="constant" library="core" kind="2" uid="14"><targets><datapoint pos="0" key="6"/></targets><data><constant value="!" datatype="string"/></data></component>
<component name="constant" library="core" kind="2" uid="15"><targets><datapoint pos="0" key="9"/></targets><data><constant value="7" datatype="string"/></data></component>
<component name="concat" library="core" kind="5" uid="16"><sources><datapoint pos="0" key="3"/><datapoint pos="1" key="4"/></sources><targets><datapoint pos="0" key="5"/></targets></component>
</children></structure><connections><edge from="1" to="3"/><edge from="6" to="4"/><edge from="5" to="2"/><edge from="9" to="8"/></connections></component>"#;
    let text = original.replace("</children>",&format!("{call}</children>"))
        .replace(r#"<vertex vertexkey="21"><edges><edge vertexkey="85"/></edges></vertex>"#, r#"<vertex vertexkey="21"><edges><edge vertexkey="101"/></edges></vertex><vertex vertexkey="102"><edges><edge vertexkey="85"/></edges></vertex>"#)
        .replace(r#"<vertex vertexkey="22"><edges><edge vertexkey="86"/></edges></vertex>"#, r#"<vertex vertexkey="103"><edges><edge vertexkey="86"/></edges></vertex>"#)
        .replace("</mapping>",&format!("{definition}</mapping>"));
    let path = directory.join("mapping.mfd");
    std::fs::write(&path, &text).unwrap();
    let options = mfd::ImportOptions::default().with_package_root(&directory);
    let imported = mfd::import_json_edi_document_with_options(&path, &options).unwrap();
    let source = group(vec![(
        "Documents",
        repeated(vec![document("one", vec![row(&["A"], &["99"])])]),
    )]);
    let value = engine::run(imported.document.project(), &source).unwrap();
    let line = &value.field("Interchange").unwrap().as_repeated().unwrap()[0]
        .field("Group")
        .unwrap()
        .as_repeated()
        .unwrap()[0]
        .field("Message_850")
        .unwrap()
        .as_repeated()
        .unwrap()[0]
        .field("Lines")
        .unwrap()
        .as_repeated()
        .unwrap()[0];
    assert_eq!(
        line.field("LIN").unwrap().field("F1"),
        Some(&self::text("A!"))
    );
    let Instance::Scalar(quantity) = line.field("LIN").unwrap().field("F2").unwrap() else {
        panic!("scalar quantity");
    };
    assert_eq!(
        functions::call("string", std::slice::from_ref(quantity)).unwrap(),
        Value::String("7".into())
    );
    assert!(codegen::lower_document(&imported.document).is_ok());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    let wrong_output = text.replace(r#"componentid="12""#, r#"componentid="999""#);
    std::fs::write(&path, wrong_output).unwrap();
    assert!(mfd::import_json_edi_document_with_options(&path, &options).is_err());
    for name in [
        "mapping.mfd",
        "source.json",
        "Envelope.Config",
        "850.Config",
        "Defs.Segment",
    ] {
        std::fs::remove_file(directory.join(name)).unwrap();
    }
    std::fs::remove_dir(directory).unwrap();
}
