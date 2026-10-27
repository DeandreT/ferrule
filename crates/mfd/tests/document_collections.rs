use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use ir::{Instance, Value};
use mapping::ScopeConstruction;

const DESIGN: &str = include_str!("fixtures/document_collections/grouped.mfd");
const INPUT: &str = "<Input><Dept><Name>Alpha</Name><Person><Name>Aster</Name></Person><Person><Name>Birch</Name></Person><Person><Name>Cedar</Name></Person></Dept><Dept><Name>Empty</Name></Dept><Dept><Name>Beta</Name><Person><Name>Elm</Name></Person></Dept><Dept><Name>Alpha</Name><Person><Name>Fir</Name></Person><Person><Name>Grove</Name></Person></Dept></Input>";

struct Directory(PathBuf);
impl Directory {
    fn new(design: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-document-collections-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("mapping.mfd"), design).unwrap();
        std::fs::write(
            path.join("source.xsd"),
            include_str!("fixtures/document_collections/source.xsd"),
        )
        .unwrap();
        std::fs::write(
            path.join("target.xsd"),
            include_str!("fixtures/document_collections/target.xsd"),
        )
        .unwrap();
        Self(path)
    }
    fn file(&self) -> PathBuf {
        self.0.join("mapping.mfd")
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn outcome(project: &mapping::Project) -> Vec<(String, Vec<String>)> {
    let source = format_xml::from_str(INPUT, &project.source).unwrap();
    let Instance::DocumentSet(documents) = engine::run(project, &source).unwrap() else {
        panic!("document set required")
    };
    documents
        .into_iter()
        .map(|document| {
            let Instance::Repeated(people) = document.value().field("Person").unwrap() else {
                panic!("ordered people required")
            };
            let names = people
                .iter()
                .map(
                    |person| match person.field("Name").and_then(Instance::as_scalar) {
                        Some(Value::String(name)) => name.clone(),
                        _ => panic!("person name required"),
                    },
                )
                .collect();
            (document.path().to_string(), names)
        })
        .collect()
}

#[test]
fn grouped_variable_filename_keeps_nested_document_contexts() {
    let directory = Directory::new(DESIGN);
    let imported = mfd::import_with_profile(
        &directory.file(),
        &mfd::ImportOptions::default().with_package_root(&directory.0),
        mfd::ImportProfile::Executable,
    )
    .unwrap()
    .imported;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(matches!(
        imported.project.root.construction,
        ScopeConstruction::CollectDocuments
    ));
    assert!(imported.project.target_path.is_none());
    assert_eq!(
        outcome(&imported.project),
        vec![
            ("Alpha-1.xml".into(), vec!["Aster".into(), "Birch".into()]),
            ("Alpha-2.xml".into(), vec!["Cedar".into(), "Fir".into()]),
            ("Alpha-3.xml".into(), vec!["Grove".into()]),
            ("Beta-1.xml".into(), vec!["Elm".into()]),
        ]
    );
    let program = codegen::lower(&imported.project).unwrap();
    assert_eq!(
        program.xml_output_mode().unwrap(),
        Some(codegen::XmlOutputMode::DynamicPrimaryDocuments)
    );
    let output = directory.0.join("unqualified.mfd");
    assert!(
        matches!(mfd::export(&imported.project, &output), Err(mfd::MfdError::Unsupported(message))
        if message == "nested document collections do not have a qualified native export")
    );
    assert!(!output.exists());
    assert_eq!(std::fs::read_to_string(directory.file()).unwrap(), DESIGN);
}

#[test]
fn unsupported_context_or_position_connections_keep_executable_refusal() {
    for changed in [
        DESIGN.replace("<edge vertexkey=\"200\"/>", ""),
        DESIGN.replace("<edge vertexkey=\"30\"/>", ""),
        DESIGN.replace(
            "name=\"parent-context\" inpkey=\"201\"",
            "name=\"compute-when\" inpkey=\"201\"",
        ),
    ] {
        let directory = Directory::new(&changed);
        let result = mfd::import_with_profile(
            &directory.file(),
            &mfd::ImportOptions::default().with_package_root(&directory.0),
            mfd::ImportProfile::Executable,
        );
        assert!(
            result.is_err(),
            "an unqualified document driver was admitted"
        );
    }
}

#[test]
fn invalid_block_size_is_a_runtime_error_not_a_flattened_fallback() {
    let directory = Directory::new(&DESIGN.replace(
        "value=\"2\" datatype=\"integer\"",
        "value=\"0\" datatype=\"integer\"",
    ));
    let imported = mfd::import(&directory.file()).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let node = imported.project.root.children[0].group_into_blocks.unwrap();
    let source = format_xml::from_str(INPUT, &imported.project.source).unwrap();
    assert_eq!(
        engine::run(&imported.project, &source),
        Err(engine::EngineError::InvalidBlockSize { node })
    );
}

#[test]
fn variable_document_identity_cast_preserves_all_documents() {
    let design = DESIGN.replace(
        "<entry name=\"parent-context\" inpkey=\"201\"><entry name=\"document\">",
        "<entry name=\"parent-context\" inpkey=\"201\"><entry name=\"document\" casttotargettypemode=\"cast-in-subtree\">",
    );
    assert_ne!(design, DESIGN);
    let plain = Directory::new(DESIGN);
    let cast = Directory::new(&design);
    let plain = mfd::import(&plain.file()).unwrap();
    let cast = mfd::import_with_profile(
        &cast.file(),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap()
    .imported;
    assert_eq!(outcome(&plain.project), outcome(&cast.project));
}

#[test]
fn variable_casts_requiring_adaptation_remain_refused() {
    let identity = DESIGN.replace(
        "<entry name=\"parent-context\" inpkey=\"201\"><entry name=\"document\">",
        "<entry name=\"parent-context\" inpkey=\"201\"><entry name=\"document\" casttotargettypemode=\"cast-in-subtree\">",
    );
    for design in [
        identity.replace("casttotargettypemode=\"cast-in-subtree\"", "casttotargettypemode=\"cast-node\""),
        identity.replace("<entry name=\"Name\" inpkey=\"202\" outkey=\"203\"/>", "<entry name=\"Name\" inpkey=\"202\" outkey=\"203\" casttotargettypemode=\"cast-in-subtree\"/>"),
        identity.replace("schema=\"source.xsd\" instanceroot=\"{}Input/{}Dept\"", "schema=\"variable.xsd\" instanceroot=\"{}Input/{}Dept\""),
    ] {
        let directory = Directory::new(&design);
        let source = include_str!("fixtures/document_collections/source.xsd");
        let (before, after) = source.rsplit_once("<xs:element name=\"Name\" type=\"xs:string\"/>").unwrap();
        let incompatible = format!("{before}<xs:element name=\"Name\" type=\"xs:integer\"/>{after}");
        std::fs::write(directory.0.join("variable.xsd"), incompatible).unwrap();
        assert!(mfd::import_with_profile(&directory.file(), &mfd::ImportOptions::default(), mfd::ImportProfile::Executable).is_err());
    }
}

#[test]
fn same_runtime_string_type_does_not_prove_datetime_cast_identity() {
    let design = DESIGN.replace(
        "<entry name=\"parent-context\" inpkey=\"201\"><entry name=\"document\">",
        "<entry name=\"parent-context\" inpkey=\"201\"><entry name=\"document\" casttotargettypemode=\"cast-in-subtree\">",
    );
    let directory = Directory::new(&design);
    let source = include_str!("fixtures/document_collections/source.xsd");
    let (before, after) = source
        .rsplit_once("<xs:element name=\"Name\" type=\"xs:string\"/>")
        .unwrap();
    std::fs::write(
        directory.0.join("source.xsd"),
        format!("{before}<xs:element name=\"Name\" type=\"xs:dateTime\"/>{after}"),
    )
    .unwrap();
    assert!(
        mfd::import_with_profile(
            &directory.file(),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable
        )
        .is_err()
    );
}

#[test]
fn copied_variable_output_rules_and_cross_component_member_collisions_are_refused() {
    let collision = r#"<component name="collision" library="xml" kind="14"><data>
      <document schema="source.xsd" instanceroot="{}Input/{}Dept"/>
      <parameter usageKind="variable"/><root><entry name="document"><entry name="Dept">
      <entry name="Person" outkey="205"/></entry></entry></root></data></component>"#;
    for design in [
        DESIGN.replace("<entry name=\"Name\" outkey=\"206\"/>", "<entry name=\"Name\" outkey=\"206\"><outputnodefunctions><function name=\"unqualified\" library=\"custom\"/></outputnodefunctions></entry>"),
        DESIGN.replace("<component name=\"group-by\"", &format!("{collision}<component name=\"group-by\"")),
    ] {
        assert_ne!(design, DESIGN);
        let directory = Directory::new(&design);
        let imported = mfd::import(&directory.file()).unwrap();
        assert!(!matches!(imported.project.root.construction, ScopeConstruction::CollectDocuments));
        assert!(!imported.warnings.is_empty());
        assert!(mfd::import_with_profile(&directory.file(), &mfd::ImportOptions::default(), mfd::ImportProfile::Executable).is_err());
    }
}

#[test]
fn filename_key_cannot_cross_a_second_repeating_collection() {
    let design = DESIGN.replace(
        "<vertex vertexkey=\"3\"><edges><edge vertexkey=\"11\"/></edges></vertex>",
        "<vertex vertexkey=\"5\"><edges><edge vertexkey=\"11\"/></edges></vertex>",
    );
    assert_ne!(design, DESIGN);
    let directory = Directory::new(&design);
    let imported = mfd::import(&directory.file()).unwrap();
    assert!(!matches!(
        imported.project.root.construction,
        ScopeConstruction::CollectDocuments
    ));
    assert!(
        mfd::import_with_profile(
            &directory.file(),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable
        )
        .is_err()
    );
}
