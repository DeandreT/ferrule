use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use ir::{Instance, Value};

const INPUT: &str = "<Input><Branch><Label>North.xml</Label><Person><First>Aster</First><Last>A</Last></Person><Person><First>Birch</First><Last>B</Last></Person></Branch><Branch><Label></Label></Branch><Branch><Label>South.xml</Label><Person><First>Cedar</First><Last>C</Last></Person></Branch></Input>";
const DESIGN: &str = include_str!("fixtures/filtered_document_filename/filtered.mfd");

struct Directory(PathBuf);
impl Directory {
    fn new(design: &str, source: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-filtered-filename-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("mapping.mfd"), design).unwrap();
        std::fs::write(path.join("source.xsd"), source).unwrap();
        std::fs::write(
            path.join("target.xsd"),
            include_str!("fixtures/filtered_document_filename/target.xsd"),
        )
        .unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
            == Some(std::ffi::OsStr::new("1"))
        {
            eprintln!("Retained filtered filename fixture: {}", self.0.display());
            return;
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project() -> mapping::Project {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/filtered_document_filename/filtered.mfd");
    let imported = mfd::import_with_profile(
        &file,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap()
    .imported;
    assert!(imported.warnings.is_empty());
    imported.project
}

fn person(first: &str, last: &str) -> Instance {
    Instance::Group(
        vec![
            (
                "First".into(),
                Instance::Scalar(Value::String(first.into())),
            ),
            ("Last".into(), Instance::Scalar(Value::String(last.into()))),
        ]
        .into(),
    )
}

#[test]
fn existence_filter_keeps_one_document_per_outer_record() {
    let project = project();
    let input = format_xml::from_str(INPUT, &project.source).unwrap();
    assert_eq!(
        project.root.source().unwrap(),
        Some(["Branch".into()].as_slice())
    );
    assert!(project.root.filter.is_some());
    let expected = [
        (
            "North.xml",
            vec![person("Aster", "A"), person("Birch", "B")],
        ),
        ("South.xml", vec![person("Cedar", "C")]),
    ]
    .into_iter()
    .map(|(path, people)| {
        ir::DocumentMember::new(
            path,
            Instance::Group(vec![("Person".into(), Instance::Repeated(people))].into()),
        )
        .unwrap()
    })
    .collect();
    assert_eq!(
        engine::run(&project, &input).unwrap(),
        Instance::DocumentSet(expected)
    );
}

#[test]
fn filtered_empty_filename_is_not_evaluated_but_accepted_empty_filename_fails() {
    let project = project();
    let skipped = format_xml::from_str(
        "<Input><Branch><Label></Label></Branch></Input>",
        &project.source,
    )
    .unwrap();
    assert_eq!(
        engine::run(&project, &skipped).unwrap(),
        Instance::DocumentSet(Vec::new())
    );
    let failing = format_xml::from_str("<Input><Branch><Label></Label><Person><First>A</First><Last>B</Last></Person></Branch></Input>", &project.source).unwrap();
    assert_eq!(
        engine::run(&project, &failing),
        Err(engine::EngineError::EmptyDynamicTargetPath {
            node: project.root.output_path().unwrap()
        })
    );
}

#[test]
fn ambiguous_or_unsupported_filename_filters_keep_executable_refusal() {
    for design in [
        DESIGN.replace("<targets><datapoint pos=\"0\" key=\"32\"/><datapoint/></targets>", "<targets><datapoint/><datapoint pos=\"1\" key=\"32\"/></targets>"),
        DESIGN.replace("name=\"exists\"", "name=\"not-exists\""),
        DESIGN.replace("<edge vertexkey=\"20\"/>", ""),
        DESIGN.replace("<vertex vertexkey=\"3\"><edges><edge vertexkey=\"10\"/>", "<vertex vertexkey=\"5\"><edges><edge vertexkey=\"10\"/>"),
        DESIGN.replace("<component name=\"exists\"", "<component name=\"constant\" library=\"core\" kind=\"2\"><data><constant value=\"other\" datatype=\"string\"/></data><targets><datapoint key=\"12\" pos=\"0\"/></targets></component><component name=\"exists\""),
    ] {
        assert_ne!(design, DESIGN);
        let directory = Directory::new(&design, include_str!("fixtures/filtered_document_filename/source.xsd"));
        let result = mfd::import_with_profile(&directory.0.join("mapping.mfd"), &mfd::ImportOptions::default(), mfd::ImportProfile::Executable);
        assert!(result.is_err(), "unqualified filtered filename was admitted");
    }
}

#[test]
fn nested_collection_cannot_fall_back_to_an_unrelated_root_collection() {
    let source = include_str!("fixtures/filtered_document_filename/source.xsd");
    let marker = "  </xs:sequence></xs:complexType></xs:element>\n</xs:schema>";
    let changed = source.replace(marker, &format!("    <xs:element name=\"Person\" type=\"xs:string\" minOccurs=\"0\" maxOccurs=\"unbounded\"/>\n{marker}"));
    assert_ne!(source, changed);
    let directory = Directory::new(DESIGN, &changed);
    assert!(
        mfd::import_with_profile(
            &directory.0.join("mapping.mfd"),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable
        )
        .is_err()
    );
}

#[test]
fn filtered_filename_survives_native_export_and_reimport() {
    let project = project();
    let directory = Directory::new(
        DESIGN,
        include_str!("fixtures/filtered_document_filename/source.xsd"),
    );
    let file = directory.0.join("exported.mfd");
    assert!(mfd::export(&project, &file).unwrap().is_empty());
    let restored = mfd::import_with_profile(
        &file,
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap()
    .imported
    .project;
    let input = format_xml::from_str(INPUT, &project.source).unwrap();
    assert_eq!(
        engine::run(&project, &input),
        engine::run(&restored, &input)
    );
}

#[test]
fn unqualified_filtered_exports_do_not_discard_the_guard() {
    let mut project = project();
    project.graph.nodes.insert(
        999,
        mapping::Node::Const {
            value: Value::Bool(false),
        },
    );
    project.root.filter = Some(999);
    let directory = Directory::new(
        DESIGN,
        include_str!("fixtures/filtered_document_filename/source.xsd"),
    );
    let file = directory.0.join("unsupported.mfd");
    assert!(
        matches!(mfd::export(&project, &file), Err(mfd::MfdError::Unsupported(reason)) if reason.contains("qualified outer-record existence guard"))
    );
    assert!(!file.exists());
}

fn with_preceding_target(design: &str) -> String {
    let target = r#"<component name="before" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data>
      <document schema="target.xsd" outputinstance="before.xml" instanceroot="{}Output"/>
      <root><entry name="document"><entry name="Output"><entry name="Person" inpkey="201">
      <entry name="First" inpkey="202"/><entry name="Last" inpkey="203"/>
      </entry></entry></entry></root></data></component>"#;
    design
        .replace(
            "<component name=\"target\" library=\"xml\" kind=\"14\">",
            &format!("{target}<component name=\"target\" library=\"xml\" kind=\"14\">"),
        )
        .replace(
            "<edge vertexkey=\"101\"/>",
            "<edge vertexkey=\"101\"/><edge vertexkey=\"201\"/>",
        )
        .replace(
            "<edge vertexkey=\"102\"/>",
            "<edge vertexkey=\"102\"/><edge vertexkey=\"202\"/>",
        )
        .replace(
            "<edge vertexkey=\"103\"/>",
            "<edge vertexkey=\"103\"/><edge vertexkey=\"203\"/>",
        )
}

#[test]
fn an_earlier_target_cannot_supply_a_cached_global_existence_check() {
    let design = with_preceding_target(DESIGN);
    let directory = Directory::new(
        &design,
        include_str!("fixtures/filtered_document_filename/source.xsd"),
    );
    let imported = mfd::import_with_profile(
        &directory.0.join("mapping.mfd"),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap()
    .imported
    .project;
    let input = format_xml::from_str(INPUT, &imported.source).unwrap();
    let output = engine::run_outputs(&imported, &input).unwrap();
    assert!(!matches!(output.primary, Instance::DocumentSet(_)));
    assert_eq!(output.extras.len(), 1);
    assert_eq!(
        output.extras[0].instance,
        engine::run(&project(), &input).unwrap()
    );
}

#[test]
fn named_source_shadow_cannot_supply_missing_nested_members() {
    let extra = r#"<component name="Person" library="xml" kind="14"><data>
      <document schema="target.xsd" inputinstance="extra.xml" instanceroot="{}Output"/>
      <root><entry name="document"><entry name="Output" outkey="300"><entry name="Person" outkey="301">
      <entry name="First" outkey="302"/><entry name="Last" outkey="303"/>
      </entry></entry></entry></root></data></component>"#;
    let design = with_preceding_target(DESIGN)
        .replace("<component name=\"before\"", &format!("{extra}<component name=\"before\""))
        .replace("<edge vertexkey=\"201\"/>", "")
        .replace("<edge vertexkey=\"202\"/>", "")
        .replace("<edge vertexkey=\"203\"/>", "")
        .replace("</vertices>", "<vertex vertexkey=\"301\"><edges><edge vertexkey=\"201\"/></edges></vertex><vertex vertexkey=\"302\"><edges><edge vertexkey=\"202\"/></edges></vertex><vertex vertexkey=\"303\"><edges><edge vertexkey=\"203\"/></edges></vertex></vertices>");
    let directory = Directory::new(
        &design,
        include_str!("fixtures/filtered_document_filename/source.xsd"),
    );
    assert!(
        mfd::import_with_profile(
            &directory.0.join("mapping.mfd"),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable
        )
        .is_err()
    );
}

#[test]
fn duplicate_source_port_and_normalized_function_pins_are_not_proofs() {
    let source = include_str!("fixtures/filtered_document_filename/source.xsd");
    let duplicate_source = source.replace("<xs:element name=\"Label\" type=\"xs:string\"/>", "<xs:element name=\"Label\" type=\"xs:string\"/><xs:element name=\"Alias\" type=\"xs:string\"/>");
    let duplicate_design = DESIGN.replace(
        "<entry name=\"Label\" outkey=\"3\"/>",
        "<entry name=\"Label\" outkey=\"3\"/><entry name=\"Alias\" outkey=\"3\"/>",
    );
    let mut cases = vec![(duplicate_design, duplicate_source)];
    for key in [10, 20, 30] {
        let changed = DESIGN
            .replace(
                &format!("<datapoint pos=\"0\" key=\"{key}\"/>"),
                &format!("<datapoint pos=\"0\" key=\"{key}\"/><datapoint pos=\"0\" key=\"400\"/>"),
            )
            .replace(
                "<edge vertexkey=\"102\"/>",
                "<edge vertexkey=\"102\"/><edge vertexkey=\"400\"/>",
            );
        cases.push((changed, source.into()));
    }
    for (design, source) in cases {
        let directory = Directory::new(&design, &source);
        assert!(
            mfd::import_with_profile(
                &directory.0.join("mapping.mfd"),
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable
            )
            .is_err()
        );
    }
}

#[test]
fn absolute_counts_and_constant_content_are_not_equivalent_exports() {
    for absolute in [false, true] {
        let mut project = project();
        if absolute {
            let filter = project.root.filter.unwrap();
            let mapping::Node::Call { args, .. } = &project.graph.nodes[&filter] else {
                panic!("count predicate")
            };
            let count = args[0];
            let mapping::Node::Aggregate { collection, .. } =
                project.graph.nodes.get_mut(&count).unwrap()
            else {
                panic!("count")
            };
            *collection = vec!["Branch".into(), "Person".into()];
        } else {
            project.graph.nodes.insert(
                999,
                mapping::Node::Const {
                    value: Value::String("replaced".into()),
                },
            );
            project.root.children[0].bindings[0].node = 999;
        }
        let directory = Directory::new(
            DESIGN,
            include_str!("fixtures/filtered_document_filename/source.xsd"),
        );
        let file = directory.0.join("unqualified.mfd");
        assert!(
            matches!(mfd::export(&project, &file), Err(mfd::MfdError::Unsupported(reason)) if reason.contains("qualified outer-record existence guard"))
        );
        assert!(!file.exists());
    }
}

#[test]
fn explicit_pin_positions_are_independent_of_xml_order() {
    let design = DESIGN
        .replace(
            "<datapoint pos=\"0\" key=\"10\"/><datapoint pos=\"1\" key=\"11\"/>",
            "<datapoint pos=\"1\" key=\"11\"/><datapoint pos=\"0\" key=\"10\"/>",
        )
        .replace(
            "<datapoint pos=\"0\" key=\"30\"/><datapoint pos=\"1\" key=\"31\"/>",
            "<datapoint pos=\"1\" key=\"31\"/><datapoint pos=\"0\" key=\"30\"/>",
        );
    assert_ne!(design, DESIGN);
    let directory = Directory::new(
        &design,
        include_str!("fixtures/filtered_document_filename/source.xsd"),
    );
    let imported = mfd::import_with_profile(
        &directory.0.join("mapping.mfd"),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap()
    .imported
    .project;
    let input = format_xml::from_str(INPUT, &imported.source).unwrap();
    assert_eq!(
        engine::run(&imported, &input),
        engine::run(&project(), &input)
    );
}

#[test]
fn conflicting_literal_declarations_are_not_filename_operands() {
    let design = DESIGN.replace(
        "<constant value=\"\" datatype=\"string\"/>",
        "<constant value=\"\" datatype=\"string\"/><constant value=\"other\" datatype=\"string\"/>",
    );
    let directory = Directory::new(
        &design,
        include_str!("fixtures/filtered_document_filename/source.xsd"),
    );
    assert!(
        mfd::import_with_profile(
            &directory.0.join("mapping.mfd"),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable
        )
        .is_err()
    );
}

#[test]
fn an_empty_wsdl_marker_does_not_change_a_literal_operand() {
    let design = DESIGN.replace(
        "<constant value=\"\" datatype=\"string\"/>",
        "<constant value=\"\" datatype=\"string\"/><wsdl/>",
    );
    let directory = Directory::new(
        &design,
        include_str!("fixtures/filtered_document_filename/source.xsd"),
    );
    let imported = mfd::import_with_profile(
        &directory.0.join("mapping.mfd"),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap()
    .imported
    .project;
    let input = format_xml::from_str(INPUT, &imported.source).unwrap();
    assert_eq!(
        engine::run(&imported, &input),
        engine::run(&project(), &input)
    );
    let nonempty = design.replace("<wsdl/>", "<wsdl><unknown/></wsdl>");
    let directory = Directory::new(
        &nonempty,
        include_str!("fixtures/filtered_document_filename/source.xsd"),
    );
    assert!(
        mfd::import_with_profile(
            &directory.0.join("mapping.mfd"),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable
        )
        .is_err()
    );
}

#[test]
fn inert_metadata_proof_cannot_be_borrowed_from_a_fake_output_owner() {
    let fake = r#"<component name="hint" library="core" kind="5"><targets><ignored key="15"/></targets><data><constant value="safe" datatype="string"/><wsdl/></data></component>"#;
    let design = DESIGN.replace("<component name=\"constant\"", &format!("{fake}<component name=\"constant\""))
        .replace("<constant value=\"\" datatype=\"string\"/>", "<constant value=\"\" datatype=\"string\"/><constant value=\"other\" datatype=\"string\"/>");
    let directory = Directory::new(
        &design,
        include_str!("fixtures/filtered_document_filename/source.xsd"),
    );
    let imported = mfd::import(&directory.0.join("mapping.mfd")).unwrap();
    assert!(imported.project.root.output_path().is_none());
    assert!(
        mfd::import_with_profile(
            &directory.0.join("mapping.mfd"),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable
        )
        .is_err()
    );
}

#[test]
fn singular_member_targets_are_not_silently_given_collection_semantics() {
    let directory = Directory::new(
        DESIGN,
        include_str!("fixtures/filtered_document_filename/source.xsd"),
    );
    let target = include_str!("fixtures/filtered_document_filename/target.xsd")
        .replace("maxOccurs=\"unbounded\"", "maxOccurs=\"1\"");
    std::fs::write(directory.0.join("target.xsd"), target).unwrap();
    assert!(
        mfd::import_with_profile(
            &directory.0.join("mapping.mfd"),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable
        )
        .is_err()
    );
}

#[test]
fn conditioned_member_sequence_is_not_an_unconditional_count() {
    let condition = r#"<condition><expression><function name="equal" library="core"><expression><attribute name="type" ns="http://www.w3.org/2001/XMLSchema-instance"/></expression><expression><constant value="Selected" datatype="QName"/></expression></function></expression></condition>"#;
    let design = DESIGN.replace(
        "<entry name=\"Person\" outkey=\"4\">",
        &format!("<entry name=\"Person\" outkey=\"4\">{condition}"),
    );
    let source = include_str!("fixtures/filtered_document_filename/source.xsd").replace("</xs:schema>", "<xs:complexType name=\"Selected\"><xs:sequence><xs:element name=\"First\" type=\"xs:string\"/><xs:element name=\"Last\" type=\"xs:string\"/></xs:sequence></xs:complexType></xs:schema>");
    let directory = Directory::new(&design, &source);
    let imported = mfd::import(&directory.0.join("mapping.mfd")).unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("without a supported document driver"))
    );
    assert!(
        mfd::import_with_profile(
            &directory.0.join("mapping.mfd"),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable
        )
        .is_err()
    );
}

#[test]
fn non_xml_primary_cannot_be_exported_as_a_qualified_xml_driver() {
    let mut project = project();
    project.source_options.xml_document = false;
    project.source_options.json_document = true;
    let directory = Directory::new(
        DESIGN,
        include_str!("fixtures/filtered_document_filename/source.xsd"),
    );
    let file = directory.0.join("non-xml.mfd");
    assert!(
        matches!(mfd::export(&project, &file), Err(mfd::MfdError::Unsupported(reason)) if reason.contains("qualified outer-record existence guard"))
    );
    assert!(!file.exists());
}

#[test]
fn one_argument_filename_concat_cannot_claim_roundtrip_support() {
    let mut project = project();
    let node = project.root.output_path().unwrap();
    let mapping::Node::Call { args, .. } = project.graph.nodes.get_mut(&node).unwrap() else {
        panic!("filename concat")
    };
    args.truncate(1);
    let directory = Directory::new(
        DESIGN,
        include_str!("fixtures/filtered_document_filename/source.xsd"),
    );
    let file = directory.0.join("one-argument.mfd");
    assert!(
        matches!(mfd::export(&project, &file), Err(mfd::MfdError::Unsupported(reason)) if reason.contains("qualified outer-record existence guard"))
    );
    assert!(!file.exists());
}
