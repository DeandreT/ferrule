use ir::Instance;
use mapping::ScopeConstruction;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

const DESIGN: &str = include_str!("fixtures/csv_document_output/grouped.mfd");

struct Directory(PathBuf);
impl Directory {
    fn new(design: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule-csv-documents-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("mapping.mfd"), design).unwrap();
        Self(root)
    }
    fn project(&self) -> mfd::Imported {
        mfd::import(&self.0.join("mapping.mfd")).unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn imports_whole_grouped_rows_with_filenames_and_exact_csv_policy() {
    let dir = Directory::new(DESIGN);
    let imported = dir.project();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = imported.project;
    assert_eq!(
        project.root.construction,
        ScopeConstruction::CopyCurrentRows
    );
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );
    let source = format_csv::from_str(
        "Code,Note\nA,one\nB,two\nC,three\nD,four\nE,five\n",
        &project.source,
        Some(','),
        true,
    )
    .unwrap();
    let output = engine::run(&project, &Instance::Repeated(source)).unwrap();
    let docs = format_csv::to_document_text(
        &project.target,
        &output,
        &format_csv::CsvWriteOptions::from(&project.target_options),
    )
    .unwrap();
    assert_eq!(
        docs.iter()
            .map(|doc| (doc.path.as_str(), doc.text.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("batch-1.csv", "Code;Note\nA;one\nB;two\n"),
            ("batch-2.csv", "Code;Note\nC;three\nD;four\n"),
            ("batch-3.csv", "Code;Note\nE;five\n"),
        ]
    );
    let program = codegen::lower(&project).unwrap();
    let policy = codegen::CsvOutputPolicy::from_format_options(&project.target_options).unwrap();
    assert!(codegen::validate_csv_document_output(&program, &policy).is_ok());
    assert!(codegen::validate_csv_output(&program, &policy).is_err());
    let options = codegen_rust::Options {
        package_name: "csv-documents".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Version("0.1.0".into()),
    };
    assert!(codegen_rust::emit_with_csv_document_output(&program, &options, &policy).is_ok());
    assert!(codegen_csharp::emit_with_csv_document_output(&program, &policy).is_ok());
    let export = dir.0.join("export.mfd");
    assert!(matches!(
        mfd::export(&project, &export),
        Err(mfd::MfdError::Unsupported(_))
    ));
    assert!(!export.exists());
}

#[test]
fn optional_block_size_keeps_its_connected_runtime_default() {
    let design = DESIGN.replace("</children>", r#"<component name="BlockSize" library="core" kind="6"><sources><datapoint pos="0" key="601"/></sources><targets><datapoint pos="0" key="602"/></targets><data><input datatype="decimal"/><wsdl/><parameter name="BlockSize" usageKind="input" optional="1"/></data></component></children>"#)
        .replace(r#"<vertex vertexkey="302"><edges><edge vertexkey="402"/></edges></vertex>"#,
            r#"<vertex vertexkey="302"><edges><edge vertexkey="601"/></edges></vertex><vertex vertexkey="602"><edges><edge vertexkey="402"/></edges></vertex>"#);
    let dir = Directory::new(&design);
    let imported = dir.project();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.project.root.construction,
        ScopeConstruction::CopyCurrentRows
    );
    let input = format_csv::from_str(
        "Code,Note\nA,one\nB,two\nC,three\n",
        &imported.project.source,
        Some(','),
        true,
    )
    .unwrap();
    let Instance::DocumentSet(documents) =
        engine::run(&imported.project, &Instance::Repeated(input)).unwrap()
    else {
        panic!("document set")
    };
    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].value().as_repeated().unwrap().len(), 2);
    assert_eq!(documents[1].value().as_repeated().unwrap().len(), 1);
}

#[test]
fn unsupported_or_ambiguous_document_shapes_keep_the_filename_warning() {
    for design in [
        DESIGN.replace("<constant value=\"2\" datatype=\"string\"/>", "<constant value=\"2\" datatype=\"string\"/><constant value=\"99\" datatype=\"string\"/>"),
        DESIGN.replace("<wsdl/>", "").replace("<constant value=\"2\" datatype=\"string\"/>", "<constant value=\"2\" datatype=\"string\"/><constant value=\"99\" datatype=\"string\"/>"),
        DESIGN.replace("<dataconnection type=\"2\"/>", "<dataconnection type=\"1\"/>"),
        DESIGN.replace("name=\"Row\" inpkey=\"200\"", "name=\"Row\" inpkey=\"200\" casttotargettypemode=\"cast-in-subtree\""),
        DESIGN.replace("pos=\"1\" key=\"408\"", "pos=\"0\" key=\"408\""),
        DESIGN.replace("<wsdl/>", "<wsdl unexpected=\"1\"/>"),
        DESIGN.replace("</children>", "<component name=\"constant\" library=\"core\" kind=\"2\"><targets><datapoint pos=\"0\" key=\"405\"/></targets><data><constant value=\"shadow\" datatype=\"string\"/></data></component></children>"),
    ] {
        let dir = Directory::new(&design);
        let imported = dir.project();
        assert_ne!(imported.project.root.construction, ScopeConstruction::CopyCurrentRows);
        assert!(imported.warnings.iter().any(|warning| warning.contains("filename-driven documents are unsupported")), "{:?}", imported.warnings);
    }
}
