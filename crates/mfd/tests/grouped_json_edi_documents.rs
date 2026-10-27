use ir::{Instance, Value};
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "grouped-json-edi-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/json_edi_documents");
        for name in [
            "mapping.mfd",
            "source.json",
            "Envelope.Config",
            "850.Config",
            "Defs.Segment",
        ] {
            std::fs::copy(fixture.join(name), root.join(name)).unwrap();
        }
        let text = std::fs::read_to_string(root.join("mapping.mfd")).unwrap()
            .replace("</children>",r#"<component name="group-by" library="core" kind="5" uid="30"><sources><datapoint pos="0" key="101"/><datapoint pos="1" key="102"/></sources><targets><datapoint/><datapoint pos="1" key="103"/></targets></component><component name="concat" library="core" kind="5" uid="31"><sources><datapoint pos="0" key="104"/><datapoint pos="1" key="105"/></sources><targets><datapoint pos="0" key="106"/></targets></component></children>"#)
            .replace(r#"<vertex vertexkey="2"><edges><edge vertexkey="80"/></edges></vertex>"#,r#"<vertex vertexkey="2"><edges><edge vertexkey="104"/></edges></vertex><vertex vertexkey="103"><edges><edge vertexkey="105"/></edges></vertex><vertex vertexkey="106"><edges><edge vertexkey="80"/><edge vertexkey="81"/><edge vertexkey="82"/></edges></vertex><vertex vertexkey="20"><edges><edge vertexkey="101"/></edges></vertex><vertex vertexkey="21"><edges><edge vertexkey="102"/></edges></vertex>"#)
            .replace(r#"<vertex vertexkey="11"><edges><edge vertexkey="81"/><edge vertexkey="82"/></edges></vertex>"#, "");
        std::fs::write(root.join("mapping.mfd"), text).unwrap();
        Self(root)
    }
    fn change(&self, from: &str, to: &str) {
        let path = self.0.join("mapping.mfd");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(from));
        std::fs::write(path, text.replace(from, to)).unwrap();
    }
    fn import(&self) -> Result<mfd::ImportedGroupedJsonEdiDocument, mfd::MfdError> {
        mfd::import_grouped_json_edi_document_with_options(
            &self.0.join("mapping.mfd"),
            &mfd::ImportOptions::default().with_package_root(&self.0),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for name in [
            "mapping.mfd",
            "source.json",
            "Envelope.Config",
            "850.Config",
            "Defs.Segment",
        ] {
            std::fs::remove_file(self.0.join(name)).unwrap();
        }
        std::fs::remove_dir(&self.0).unwrap();
    }
}

fn input(imported: &mfd::ImportedGroupedJsonEdiDocument, texts: &[&str]) -> Instance {
    let docs = texts
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let document = codegen_runtime::JsonSourceDocument::parse(
                &format!("file{index}.json"),
                text.as_bytes(),
            )
            .unwrap();
            codegen_runtime::project_json_source_document(
                &imported.source.shape,
                &document,
                Default::default(),
            )
            .unwrap()
        })
        .collect();
    Instance::Group(vec![("Documents".into(), Instance::Repeated(docs))].into())
}

#[test]
fn grouped_keys_keep_empty_values_and_merge_physical_files_without_crossing_owners() {
    let fixture = Fixture::new();
    let before = std::fs::read(fixture.0.join("mapping.mfd")).unwrap();
    let imported = fixture.import().unwrap();
    let source = input(
        &imported,
        &[
            r#"{"rows":[{"name":"a","qty":1},{"name":"b","qty":2},{"name":"","qty":3},{"qty":99},{"name":null,"qty":98}]}"#,
            r#"{"rows":[{"name":"a","qty":4},{"name":"b","qty":5}]}"#,
        ],
    );
    let Instance::DocumentSet(documents) =
        engine::run(imported.document.project(), &source).unwrap()
    else {
        panic!("document set");
    };
    assert_eq!(
        documents.iter().map(|d| d.path()).collect::<Vec<_>>(),
        ["out.edia", "out.edib", "out.edi"]
    );
    for (document, expected) in documents
        .iter()
        .zip([vec!["1", "4"], vec!["2", "5"], vec!["3"]])
    {
        let interchange = &document
            .value()
            .field("Interchange")
            .unwrap()
            .as_repeated()
            .unwrap()[0];
        let group = &interchange.field("Group").unwrap().as_repeated().unwrap()[0];
        let message = group
            .field("Message_850")
            .unwrap_or_else(|| panic!("{group:?}"));
        let message = match message {
            Instance::Repeated(items) => &items[0],
            value => value,
        };
        let lines = message.field("Lines").unwrap().as_repeated().unwrap();
        let actual = lines
            .iter()
            .map(|line| {
                let Instance::Scalar(value) = line.field("LIN").unwrap().field("F2").unwrap()
                else {
                    panic!("scalar qty");
                };
                functions::call("string", std::slice::from_ref(value)).unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            expected
                .into_iter()
                .map(|s| Value::String(s.into()))
                .collect::<Vec<_>>()
        );
    }
    let program = codegen::lower_document(&imported.document).unwrap();
    assert!(
        codegen::prepare_mapforce_x12_writer(
            program.program(),
            &imported.document.project().target_options,
            &imported.writer_minimums
        )
        .is_err()
    );
    // This small import fixture deliberately has no complete ISA/GS schema.
    // Admitting document-set execution must not bypass writer-schema checks.
    assert!(
        codegen::prepare_mapforce_x12_document_writer(
            &program,
            &imported.document.project().target_options,
            &imported.writer_minimums
        )
        .is_err()
    );
    assert!(
        mfd::import_json_edi_document_with_options(
            &fixture.0.join("mapping.mfd"),
            &mfd::ImportOptions::default().with_package_root(&fixture.0)
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(fixture.0.join("mapping.mfd")).unwrap(),
        before
    );
}

#[test]
fn grouped_filename_admission_keeps_its_raw_pins_and_source_guards() {
    for (from, to) in [
        (
            r#"<datapoint pos="1" key="103"/>"#,
            r#"<datapoint pos="0" key="103"/>"#,
        ),
        (r#"<edge vertexkey="101"/>"#, r#"<edge vertexkey="102"/>"#),
        (
            r#"name="group-by" library="core""#,
            r#"name="group-by" library="foreign""#,
        ),
    ] {
        let fixture = Fixture::new();
        fixture.change(from, to);
        assert!(fixture.import().is_err());
    }
}
