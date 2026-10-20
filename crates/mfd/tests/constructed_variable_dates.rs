use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

#[path = "fixtures/constructed-variable-dates/expected_project.rs"]
mod oracle;

use oracle::Shape;

const ROOT: Shape = Shape::Source {
    nested: false,
    picture: "[Y]-[M]-[D]",
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/constructed-variable-dates")
        .join(name)
}

fn import(path: &Path) -> mfd::Imported {
    mfd::import_with_options(
        path,
        &mfd::ImportOptions::default().with_package_root(path.parent().unwrap()),
    )
    .unwrap()
}

fn verify_case(project: &mapping::Project, case: &serde_json::Value) {
    let source = format_json::from_str(case["input"].as_str().unwrap(), &project.source);
    if case["reject"] == true {
        assert!(source.is_err(), "{case}");
        return;
    }
    let mut parameters = engine::RuntimeParameters::new();
    for (name, value) in case
        .get("parameters")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flatten()
    {
        let value = ir::Value::String(value.as_str().unwrap().into());
        parameters.insert(name, value).unwrap();
    }
    let context =
        engine::ExecutionContext::new(Path::new("dates.mfd")).with_parameters(&parameters);
    let result = engine::run_with_context(project, &source.unwrap(), &context);
    if let Some(expected) = case.get("execution_error") {
        assert!(
            matches!(&result, Err(engine::EngineError::Function(functions::FunctionError::InvalidArgument { function, message }))
                if *function == expected["function"].as_str().unwrap()
                    && *message == expected["message"].as_str().unwrap()),
            "{case}: {result:?}"
        );
    } else if let Some(expected) = case.get("type_error") {
        assert!(
            matches!(&result, Err(engine::EngineError::Function(functions::FunctionError::TypeMismatch { function, got }))
                if *function == expected["function"].as_str().unwrap()
                    && *got == expected["found"].as_str().unwrap()),
            "{case}: {result:?}"
        );
    } else if let Some(expected) = case.get("boolean_error") {
        assert!(
            matches!(&result, Err(engine::EngineError::NotABool { node, found })
                if u64::from(*node) == expected["node"].as_u64().unwrap()
                    && *found == expected["found"].as_str().unwrap()),
            "{case}: {result:?}"
        );
    } else if let Some(expected) = case.get("missing_parameter") {
        assert!(
            matches!(&result, Err(engine::EngineError::MissingRuntimeParameter { node, name })
                if u64::from(*node) == expected["node"].as_u64().unwrap()
                    && name == expected["name"].as_str().unwrap()),
            "{case}: {result:?}"
        );
    } else {
        let output = format_json::to_string(&project.target, &result.unwrap()).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output).unwrap(),
            case["expected"],
            "{case}"
        );
    }
}

/// Every authored design paired with its oracle shape and value catalog.
fn designs() -> [(&'static str, Shape, &'static str); 11] {
    let nested = Shape::Source {
        nested: true,
        picture: "[Y]-[M]-[D]",
    };
    [
        ("root-json.mfd", ROOT, "cases.json"),
        ("root-edi.mfd", ROOT, "cases.json"),
        ("alias-json.mfd", ROOT, "cases.json"),
        ("alias-edi.mfd", ROOT, "cases.json"),
        ("nested-json.mfd", nested, "nested-cases.json"),
        ("nested-edi.mfd", nested, "nested-cases.json"),
        ("parser-json.mfd", Shape::Parser, "parser-cases.json"),
        ("parser-edi.mfd", Shape::Parser, "parser-cases.json"),
        (
            "malformed-picture.mfd",
            Shape::Source {
                nested: false,
                picture: "[]-x",
            },
            "picture-cases.json",
        ),
        (
            "blank-picture.mfd",
            Shape::Source {
                nested: false,
                picture: "",
            },
            "picture-cases.json",
        ),
        ("connection-edi.mfd", ROOT, "cases.json"),
    ]
}

#[test]
fn parsed_dates_preserve_complete_projects_values_and_typed_errors() {
    for (design, shape, catalog) in designs() {
        let imported = import(&fixture(design));
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        assert_eq!(
            mapping::project_file::encode_pretty(&imported.project).unwrap(),
            mapping::project_file::encode_pretty(&oracle::expected(shape)).unwrap(),
            "{design}"
        );
        assert!(engine::validate(&imported.project).is_empty(), "{design}");
        let cases: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(fixture(catalog)).unwrap()).unwrap();
        for case in &cases {
            verify_case(&imported.project, case);
        }
    }
}

struct Package(PathBuf);

impl Drop for Package {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A refused date assignment removes the constructed record with a feed
/// warning; it never silently admits the record.
fn refuses(design: &str, label: &str) {
    refuses_with(design, label, true);
}

/// Unresolved resources warn on their own before any record proof.
fn refuses_unresolved(design: &str, label: &str) {
    refuses_with(design, label, false);
}

/// Either refusal keeps the design out of the executable profile.
fn refuses_with(design: &str, label: &str, feed_warning: bool) {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let package = Package(std::env::temp_dir().join(format!(
        "ferrule-constructed-date-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    std::fs::create_dir(&package.0).unwrap();
    for entry in std::fs::read_dir(fixture("")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), package.0.join(entry.file_name())).unwrap();
    }
    let path = package.0.join("control.mfd");
    std::fs::write(&path, design).unwrap();
    let imported = import(&path);
    assert!(
        !imported.warnings.is_empty()
            && (!feed_warning
                || imported.warnings.iter().any(|warning| {
                    warning.contains("unsupported feed")
                        || warning.contains("cannot construct repeating target")
                })),
        "{label}: {:?}",
        imported.warnings
    );
    assert!(
        matches!(
            mfd::import_with_profile(
                &path,
                &mfd::ImportOptions::default().with_package_root(&package.0),
                mfd::ImportProfile::Executable,
            ),
            Err(mfd::MfdError::IncompatibleImport(_))
        ),
        "{label}: {:?}",
        imported.warnings
    );
}

const PARSE: &str = r#"<component name="parse-date" library="core" kind="5"><sources><datapoint pos="0" key="111"/><datapoint pos="1" key="112"/></sources><targets><datapoint pos="0" key="110"/></targets></component>"#;
const PICTURE: &str = r#"<component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="102"/></targets><data><constant value="[Y]-[M]-[D]" datatype="string"/></data></component>"#;
const VALUE_EDGE: &str =
    r#"<vertex vertexkey="15"><edges><edge vertexkey="111"/></edges></vertex>"#;
const PICTURE_EDGE: &str =
    r#"<vertex vertexkey="102"><edges><edge vertexkey="112"/></edges></vertex>"#;

fn replace_once(design: &str, from: &str, to: &str) -> String {
    assert_eq!(design.matches(from).count(), 1, "{from}");
    design.replace(from, to)
}

fn vertex(from: u32, to: u32) -> String {
    format!(r#"<vertex vertexkey="{from}"><edges><edge vertexkey="{to}"/></edges></vertex>"#)
}

fn with_vertices(design: &str, vertices: &[(u32, u32)]) -> String {
    let added = vertices
        .iter()
        .map(|(from, to)| vertex(*from, *to))
        .collect::<String>();
    replace_once(design, "</vertices>", &format!("{added}</vertices>"))
}

/// Add one edge in the legacy vocabulary beside the design's graph vertices.
fn with_legacy_edge(design: &str, from: u32, to: u32) -> String {
    let edge = format!(r#"<edge from="{from}" to="{to}"/>"#);
    if design.contains("<connections>") {
        replace_once(design, "<connections>", &format!("<connections>{edge}"))
    } else {
        replace_once(
            design,
            "</structure>",
            &format!("<connections>{edge}</connections></structure>"),
        )
    }
}

fn with_components(design: &str, components: &str) -> String {
    replace_once(design, "</children>", &format!("{components}</children>"))
}

#[test]
fn date_assignments_refuse_malformed_foreign_and_shadowed_signatures() {
    let udf_definition = r#"<component name="stamp" library="helpers"><structure><children><component name="constant" library="core" uid="700" kind="2"><targets><datapoint pos="0" key="7000"/></targets><data><constant value="2024-01-01" datatype="string"/></data></component><component name="Value" library="core" uid="701" kind="7"><sources><datapoint pos="0" key="7001"/></sources><data><output datatype="string"/></data></component></children><graph><vertices><vertex vertexkey="7000"><edges><edge vertexkey="7001"/></edges></vertex></vertices></graph></structure></component>"#;
    let udf_call = r#"<component name="stamp" library="helpers" kind="19"><data><root rootindex="1"><entry name="Value" outkey="110" componentid="701"/></root></data></component>"#;
    let raw = |original: &str, replacement: String| replace_once(original, PARSE, &replacement);
    for original in ["root-json.mfd", "root-edi.mfd", "nested-edi.mfd"] {
        let original = std::fs::read_to_string(fixture(original)).unwrap();
        for (label, design) in [
            (
                "foreign library",
                raw(
                    &original,
                    PARSE.replace("library=\"core\"", "library=\"xpath2\""),
                ),
            ),
            (
                "lang library",
                raw(
                    &original,
                    PARSE.replace("library=\"core\"", "library=\"lang\""),
                ),
            ),
            (
                "wrong kind",
                raw(&original, PARSE.replace("kind=\"5\"", "kind=\"4\"")),
            ),
            (
                "foreign name",
                raw(
                    &original,
                    PARSE.replace("name=\"parse-date\"", "name=\"parse-time\""),
                ),
            ),
            (
                "extra component attribute",
                raw(
                    &original,
                    PARSE.replace("kind=\"5\"", "kind=\"5\" uid=\"9\""),
                ),
            ),
            (
                "incomplete raw inputs",
                raw(
                    &original,
                    PARSE.replace("<datapoint pos=\"1\" key=\"112\"/>", ""),
                ),
            ),
            (
                "keyless raw input",
                raw(&original, PARSE.replace(" key=\"112\"", "")),
            ),
            (
                "duplicate raw input position",
                raw(
                    &original,
                    PARSE.replace("pos=\"1\" key=\"112\"", "pos=\"0\" key=\"112\""),
                ),
            ),
            (
                "duplicate raw input key",
                raw(&original, PARSE.replace("key=\"112\"", "key=\"111\"")),
            ),
            (
                "malformed raw input position",
                raw(
                    &original,
                    PARSE.replace("pos=\"1\" key=\"112\"", "pos=\"one\" key=\"112\""),
                ),
            ),
            (
                "extra raw input",
                raw(
                    &original,
                    PARSE.replace("</sources>", "<datapoint pos=\"2\" key=\"113\"/></sources>"),
                ),
            ),
            (
                "extra raw input container",
                raw(
                    &original,
                    PARSE.replace("</sources>", "</sources><sources/>"),
                ),
            ),
            (
                "hidden raw child",
                raw(&original, PARSE.replace("<sources>", "<sources><hidden/>")),
            ),
            (
                "secondary output",
                raw(
                    &original,
                    PARSE.replace("pos=\"0\" key=\"110\"", "pos=\"1\" key=\"110\""),
                ),
            ),
            (
                "extra output",
                raw(
                    &original,
                    PARSE.replace(
                        "key=\"110\"/>",
                        "key=\"110\"/><datapoint pos=\"1\" key=\"119\"/>",
                    ),
                ),
            ),
            (
                "output keyed as input",
                raw(&original, PARSE.replace("key=\"110\"", "key=\"111\""))
                    .replace("vertexkey=\"110\"", "vertexkey=\"111\""),
            ),
            (
                "competing literal producer",
                with_components(
                    &original,
                    r#"<component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="110"/></targets><data><constant value="2024-01-01" datatype="string"/></data></component>"#,
                ),
            ),
            (
                "competing parse producer",
                with_components(
                    &original,
                    &PARSE
                        .replace("key=\"111\"", "key=\"117\"")
                        .replace("key=\"112\"", "key=\"118\""),
                ),
            ),
            (
                "skipped component shadow",
                with_components(
                    &original,
                    r#"<component name="unknown" library="unlowered" kind="999"><targets><datapoint key="110"/></targets></component>"#,
                ),
            ),
            (
                "skipped picture shadow",
                with_components(
                    &original,
                    r#"<component name="unknown" library="unlowered" kind="999"><targets><datapoint key="102"/></targets></component>"#,
                ),
            ),
            (
                "skipped value shadow",
                with_components(
                    &original,
                    r#"<component name="unknown" library="xml" kind="999"><data><root><entry name="Hidden" outkey="15"/></root></data></component>"#,
                ),
            ),
            (
                "physical source output shadow",
                original
                    .replace("outkey=\"14\"", "outkey=\"110\"")
                    .replace("vertexkey=\"14\"", "vertexkey=\"110\""),
            ),
            (
                "variable output shadow",
                original.replace("outkey=\"27\"", "outkey=\"110\""),
            ),
            (
                "UDF shadow",
                with_components(&original, udf_call)
                    .replace("</mapping>", &format!("{udf_definition}</mapping>")),
            ),
            (
                "UDF producer",
                raw(&original, udf_call.to_string())
                    .replace("</mapping>", &format!("{udf_definition}</mapping>")),
            ),
        ] {
            assert_ne!(design, original, "{label}");
            refuses(&design, label);
        }
    }
}

#[test]
fn date_assignments_refuse_computed_unbound_and_ambiguous_operands() {
    let concat = r#"<component name="concat" library="core" kind="5"><sources><datapoint pos="0" key="161"/><datapoint pos="1" key="162"/></sources><targets><datapoint pos="0" key="160"/></targets></component>"#;
    for original in ["root-json.mfd", "root-edi.mfd", "nested-json.mfd"] {
        let original = std::fs::read_to_string(fixture(original)).unwrap();
        let without_value = replace_once(&original, VALUE_EDGE, "");
        let without_picture = replace_once(&original, PICTURE_EDGE, "");
        for (label, design) in [
            ("missing value", without_value.clone()),
            ("missing picture", without_picture.clone()),
            (
                "conflicting value feeds",
                with_vertices(&original, &[(14, 111)]),
            ),
            (
                "repeated value edge",
                with_vertices(&original, &[(15, 111)]),
            ),
            (
                "repeated legacy value edge",
                with_legacy_edge(&original, 15, 111),
            ),
            (
                "legacy picture edge beside graph edge",
                with_legacy_edge(&original, 102, 112),
            ),
            (
                "computed value",
                with_vertices(
                    &with_components(&without_value, concat),
                    &[(15, 161), (16, 162), (160, 111)],
                ),
            ),
            (
                "literal value",
                with_vertices(&without_value, &[(101, 111)]),
            ),
            (
                "conditional value",
                with_vertices(&without_value, &[(135, 111)]),
            ),
            (
                "nested parsed value",
                with_vertices(&without_value, &[(120, 111)]),
            ),
            (
                "source-field picture",
                with_vertices(&without_picture, &[(14, 112)]),
            ),
            (
                "computed picture",
                with_vertices(
                    &with_components(&without_picture, concat),
                    &[(102, 161), (102, 162), (160, 112)],
                ),
            ),
            (
                "conditional picture",
                with_vertices(&without_picture, &[(135, 112)]),
            ),
            (
                "non-String literal picture",
                replace_once(
                    &original,
                    PICTURE,
                    &PICTURE.replace(
                        "value=\"[Y]-[M]-[D]\" datatype=\"string\"",
                        "value=\"1\" datatype=\"integer\"",
                    ),
                ),
            ),
            (
                "untyped literal picture",
                replace_once(
                    &original,
                    PICTURE,
                    &PICTURE.replace(" datatype=\"string\"", ""),
                ),
            ),
            (
                "annotated literal picture",
                replace_once(
                    &original,
                    PICTURE,
                    &PICTURE.replace("datatype=\"string\"", "datatype=\"string\" hidden=\"1\""),
                ),
            ),
            (
                "multi-output picture",
                replace_once(
                    &original,
                    PICTURE,
                    &PICTURE.replace(
                        "key=\"102\"/>",
                        "key=\"102\"/><datapoint pos=\"1\" key=\"109\"/>",
                    ),
                ),
            ),
        ] {
            assert_ne!(design, original, "{label}");
            refuses(&design, label);
        }
    }
}

#[test]
fn date_assignments_refuse_structural_forwards_and_unresolved_schemas() {
    let alias = std::fs::read_to_string(fixture("alias-json.mfd")).unwrap();
    let exposed = replace_once(
        &alias,
        "<entry name=\"object\"><entry name=\"Choose\"",
        "<entry name=\"object\" outkey=\"17\"><entry name=\"Choose\"",
    );
    let forward = r#"<vertex vertexkey="15"><edges><edge vertexkey="402"/></edges></vertex>"#;
    for (label, design) in [
        (
            "structural forward only",
            with_vertices(&replace_once(&exposed, forward, ""), &[(17, 400)]),
        ),
        (
            "structural ancestor beside scalar forward",
            with_vertices(&exposed, &[(17, 400)]),
        ),
    ] {
        assert_ne!(design, alias, "{label}");
        refuses(&design, label);
    }
    refuses_unresolved(
        &replace_once(
            &alias,
            "<json schema=\"source.schema.json\"/>",
            "<json schema=\"missing.schema.json\"/>",
        ),
        "unresolved alias schema",
    );
    for (design, from, to) in [
        (
            "root-json.mfd",
            "variable.schema.json",
            "missing.schema.json",
        ),
        (
            "root-json.mfd",
            "\"source.schema.json\"",
            "\"missing.schema.json\"",
        ),
        (
            "parser-json.mfd",
            "payload.schema.json",
            "missing.schema.json",
        ),
        (
            "parser-edi.mfd",
            "payload.schema.json",
            "missing.schema.json",
        ),
        (
            "root-edi.mfd",
            "config=\"Envelope.Config\"",
            "config=\"Missing.Config\"",
        ),
    ] {
        let original = std::fs::read_to_string(fixture(design)).unwrap();
        refuses_unresolved(
            &replace_once(&original, from, to),
            &format!("{design}: unresolved {to}"),
        );
    }
}

#[test]
fn parsed_dates_never_witness_presence_or_bypass_parent_frames() {
    let literal = vertex(101, 22);
    for design in [
        "root-json.mfd",
        "root-edi.mfd",
        "nested-json.mfd",
        "parser-json.mfd",
    ] {
        let original = std::fs::read_to_string(fixture(design)).unwrap();
        refuses(
            &replace_once(&original, &literal, ""),
            &format!("{design}: date-only record"),
        );
    }
    let filter = r#"<component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="170"/><datapoint pos="1" key="171"/></sources><targets><datapoint pos="0" key="172"/></targets></component><component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="173"/></targets><data><constant value="true" datatype="boolean"/></data></component>"#;
    let driver = vertex(10, 40);
    let others = "<entry name=\"Others\" type=\"json-property\"><entry name=\"array\" outkey=\"18\"><entry name=\"item\" type=\"json-item\"><entry name=\"object\"/></entry></entry></entry></entry></entry></entry></entry></root><json schema=\"competing-source.schema.json\"";
    for design in ["nested-json.mfd", "nested-edi.mfd"] {
        // Leave the parsed date as the only frame-dependent assignment.
        let original = replace_once(
            &std::fs::read_to_string(fixture(design)).unwrap(),
            &vertex(135, 26),
            "",
        );
        let controlled = with_vertices(
            &replace_once(&with_components(&original, filter), &driver, ""),
            &[(10, 170), (173, 171), (172, 40)],
        );
        refuses(&controlled, &format!("{design}: controlled parent"));
        refuses(
            &replace_once(&original, &driver, ""),
            &format!("{design}: uniterated parent"),
        );
        refuses(
            &with_vertices(&original, &[(14, 40)]),
            &format!("{design}: competing parent drivers"),
        );
        // Another collection drives the enclosing target, so the date's own
        // repeated ancestor has no actual parent frame.
        let foreign = replace_once(
            &replace_once(
                &original,
                "</entry></entry></entry></entry></root><json schema=\"nested-source.schema.json\"",
                others,
            ),
            &driver,
            &vertex(18, 40),
        );
        refuses(&foreign, &format!("{design}: foreign parent frame"));
    }
}
