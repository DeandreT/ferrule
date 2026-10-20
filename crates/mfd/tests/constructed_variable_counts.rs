use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

#[path = "fixtures/constructed-variable-counts/expected_project.rs"]
mod oracle;

use oracle::Shape;

const ROOT: Shape = Shape {
    nested: false,
    copy_all: true,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/constructed-variable-counts")
        .join(name)
}

fn read(name: &str) -> String {
    std::fs::read_to_string(fixture(name)).unwrap()
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
    let output = engine::run(project, &source.unwrap()).unwrap();
    let output = format_json::to_string(&project.target, &output).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&output).unwrap(),
        case["expected"],
        "{case}"
    );
}

fn assert_admitted(design: &str, path: &Path, shape: Shape, catalog: &str) {
    let imported = import(path);
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

#[test]
fn forwarded_counts_preserve_complete_projects_and_literal_counts() {
    let per_field = |nested| Shape {
        nested,
        copy_all: false,
    };
    for (design, shape, catalog) in [
        ("root-json.mfd", ROOT, "cases.json"),
        ("chain-json.mfd", ROOT, "cases.json"),
        ("direct-json.mfd", ROOT, "cases.json"),
        ("root-edi.mfd", per_field(false), "cases.json"),
        ("nested-json.mfd", per_field(true), "nested-cases.json"),
        ("nested-edi.mfd", per_field(true), "nested-cases.json"),
        ("direct-edi.mfd", per_field(true), "nested-cases.json"),
    ] {
        assert_admitted(design, &fixture(design), shape, catalog);
    }
}

struct Package(PathBuf);

impl Package {
    fn new(design: &str) -> (Self, PathBuf) {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let package = Package(std::env::temp_dir().join(format!(
            "ferrule-constructed-count-{}-{}",
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
        (package, path)
    }
}

impl Drop for Package {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A refused count rejects the constructed record with a feed warning and
/// keeps the design out of the executable profile; it is never admitted as
/// an empty-sequence constant.
fn refuses(design: &str, label: &str) {
    refuses_with(design, label, true);
}

/// Unresolved resources warn on their own before any record proof.
fn refuses_unresolved(design: &str, label: &str) {
    refuses_with(design, label, false);
}

fn refuses_with(design: &str, label: &str, feed_warning: bool) {
    let (package, path) = Package::new(design);
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

const COUNT: &str = r#"<component name="count" library="core" kind="5"><sources><datapoint/><datapoint pos="1" key="61"/></sources><targets><datapoint pos="0" key="60"/></targets></component>"#;

fn forward(name: &str, input: u32, output: u32) -> String {
    format!(
        r#"<component name="{name}" library="json" kind="31"><data><root><entry name="document"><entry name="array" inpkey="{input}" outkey="{output}"><entry name="item" type="json-item"><entry name="object"><entry name="Label" type="json-property"><entry name="string"/></entry><entry name="Total" type="json-property"><entry name="integer"/></entry></entry></entry></entry></entry></root><parameter usageKind="variable"><root><entry name="root"/><entry name="object"/><entry name="Items" type="json-property"/><entry name="array"/></root></parameter><json schema="forward.schema.json"/></data></component>"#
    )
}

fn vertex(from: u32, to: u32) -> String {
    format!(r#"<vertex vertexkey="{from}"><edges><edge vertexkey="{to}"/></edges></vertex>"#)
}

fn replace_once(design: &str, from: &str, to: &str) -> String {
    assert_eq!(design.matches(from).count(), 1, "{from}");
    design.replace(from, to)
}

fn with_vertices(design: &str, vertices: &[(u32, u32)]) -> String {
    let added = vertices
        .iter()
        .map(|(from, to)| vertex(*from, *to))
        .collect::<String>();
    replace_once(design, "</vertices>", &format!("{added}</vertices>"))
}

fn without_vertex(design: &str, from: u32, to: u32) -> String {
    replace_once(design, &format!("{}\n", vertex(from, to)), "")
}

fn with_components(design: &str, components: &str) -> String {
    replace_once(design, "</children>", &format!("{components}</children>"))
}

/// A root design whose count reads through `hops` chained forwards.
fn chain(hops: u32) -> String {
    let original = read("root-json.mfd");
    let mut design = without_vertex(
        &without_vertex(
            &replace_once(
                &original,
                &format!("{}\n", forward("forward0", 200, 201)),
                "",
            ),
            10,
            200,
        ),
        201,
        61,
    );
    let mut feed = 10;
    for hop in 0..hops {
        let (input, output) = (1000 + 2 * hop, 1001 + 2 * hop);
        design = with_vertices(
            &with_components(&design, &forward(&format!("forward{hop}"), input, output)),
            &[(feed, input)],
        );
        feed = output;
    }
    with_vertices(&design, &[(feed, 61)])
}

#[test]
fn forward_chains_are_bounded() {
    let (_package, path) = Package::new(&chain(11));
    assert_admitted("11-hop chain", &path, ROOT, "cases.json");
    refuses(&chain(12), "12-hop chain");
}

#[test]
fn counts_refuse_malformed_foreign_and_shadowed_signatures() {
    let udf_definition = r#"<component name="tally" library="helpers"><structure><children><component name="constant" library="core" uid="700" kind="2"><targets><datapoint pos="0" key="7000"/></targets><data><constant value="2" datatype="integer"/></data></component><component name="Value" library="core" uid="701" kind="7"><sources><datapoint pos="0" key="7001"/></sources><data><output datatype="integer"/></data></component></children><graph><vertices><vertex vertexkey="7000"><edges><edge vertexkey="7001"/></edges></vertex></vertices></graph></structure></component>"#;
    let udf_call = r#"<component name="tally" library="helpers" kind="19"><data><root rootindex="1"><entry name="Value" outkey="60" componentid="701"/></root></data></component>"#;
    for original in ["root-json.mfd", "root-edi.mfd", "nested-json.mfd"] {
        let original = read(original);
        let raw = |replacement: String| replace_once(&original, COUNT, &replacement);
        for (label, design) in [
            (
                "lang library",
                raw(COUNT.replace("library=\"core\"", "library=\"lang\"")),
            ),
            ("wrong kind", raw(COUNT.replace("kind=\"5\"", "kind=\"4\""))),
            (
                "other aggregate",
                raw(COUNT.replace("name=\"count\"", "name=\"sum\"")),
            ),
            (
                "extra component attribute",
                raw(COUNT.replace("kind=\"5\"", "kind=\"5\" other=\"1\"")),
            ),
            (
                "keyed context slot",
                raw(COUNT.replace("<datapoint/>", "<datapoint pos=\"0\" key=\"62\"/>")),
            ),
            (
                "connected context slot",
                with_vertices(
                    &raw(COUNT.replace("<datapoint/>", "<datapoint pos=\"0\" key=\"62\"/>")),
                    &[(10, 62)],
                ),
            ),
            (
                "missing context slot",
                raw(COUNT.replace("<datapoint/>", "")),
            ),
            (
                "context slot at sequence position",
                raw(COUNT.replace("<datapoint/>", "<datapoint pos=\"1\"/>")),
            ),
            ("keyless sequence", raw(COUNT.replace(" key=\"61\"", ""))),
            (
                "unpositioned sequence",
                raw(COUNT.replace(" pos=\"1\" key=\"61\"", " key=\"61\"")),
            ),
            (
                "extra raw input",
                raw(COUNT.replace("</sources>", "<datapoint pos=\"2\" key=\"63\"/></sources>")),
            ),
            (
                "secondary output",
                raw(COUNT.replace("pos=\"0\" key=\"60\"", "pos=\"1\" key=\"60\"")),
            ),
            (
                "extra output",
                raw(COUNT.replace(
                    "key=\"60\"/>",
                    "key=\"60\"/><datapoint pos=\"1\" key=\"64\"/>",
                )),
            ),
            (
                "hidden payload",
                raw(COUNT.replace("</targets>", "</targets><data/>")),
            ),
            (
                "competing literal producer",
                with_components(
                    &original,
                    r#"<component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="60"/></targets><data><constant value="7" datatype="integer"/></data></component>"#,
                ),
            ),
            (
                "competing count producer",
                with_components(&original, &COUNT.replace("key=\"61\"", "key=\"63\"")),
            ),
            (
                "variable output shadow",
                original.replace("outkey=\"25\"", "outkey=\"60\""),
            ),
            (
                "source output shadow",
                original.replace("outkey=\"12\"", "outkey=\"60\""),
            ),
            (
                "UDF shadow",
                with_components(&original, udf_call)
                    .replace("</mapping>", &format!("{udf_definition}</mapping>")),
            ),
            (
                "UDF producer",
                raw(udf_call.to_string())
                    .replace("</mapping>", &format!("{udf_definition}</mapping>")),
            ),
        ] {
            assert_ne!(design, original, "{label}");
            refuses(&design, label);
        }
    }
}

#[test]
fn counts_refuse_controlled_ambiguous_and_retyped_sequences() {
    let filter = r#"<component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="300"/><datapoint pos="1" key="301"/></sources><targets><datapoint pos="0" key="302"/></targets></component><component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="303"/></targets><data><constant value="true" datatype="boolean"/></data></component>"#;
    let sort = r#"<component name="sort" library="core" kind="30"><sources><datapoint pos="0" key="310"/><datapoint pos="1" key="311"/></sources><targets><datapoint pos="0" key="312"/></targets><data><sort><collation/><key direction="descending"/></sort></data></component>"#;
    let window = r#"<component name="first-items" library="core" kind="5"><sources><datapoint pos="0" key="320"/><datapoint pos="1" key="321"/></sources><targets><datapoint pos="0" key="322"/></targets></component><component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="323"/></targets><data><constant value="2" datatype="integer"/></data></component>"#;
    let distinct = r#"<component name="distinct-values" library="core" kind="5"><sources><datapoint pos="0" key="330"/></sources><targets><datapoint pos="0" key="331"/></targets></component>"#;
    let group = r#"<component name="group-by" library="core" kind="5"><sources><datapoint pos="0" key="340"/><datapoint pos="1" key="341"/></sources><targets><datapoint pos="0" key="342"/><datapoint pos="1" key="343"/></targets></component>"#;
    let tokenize = r#"<component name="tokenize" library="core" kind="5"><sources><datapoint pos="0" key="350"/><datapoint pos="1" key="351"/></sources><targets><datapoint pos="0" key="352"/></targets></component><component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="354"/></targets><data><constant value="a,b" datatype="string"/></data></component><component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="355"/></targets><data><constant value="," datatype="string"/></data></component>"#;
    let udf_definition = r#"<component name="rows" library="helpers"><structure><children><component name="Rows" library="core" uid="700" kind="6"><sources><datapoint pos="0" key="7000"/></sources><targets><datapoint pos="0" key="7001"/></targets><data><input datatype="string"/><parameter usageKind="input" name="Rows"/></data></component><component name="Value" library="core" uid="701" kind="7"><sources><datapoint pos="0" key="7002"/></sources><data><output datatype="string"/></data></component></children><graph><vertices><vertex vertexkey="7001"><edges><edge vertexkey="7002"/></edges></vertex></vertices></graph></structure></component>"#;
    let udf_call = r#"<component name="rows" library="helpers" kind="19"><sources><datapoint pos="0" key="360"/></sources><data><root rootindex="1"><entry name="Value" outkey="361" componentid="701"/></root></data></component>"#;
    let item = "<entry name=\"item\" type=\"json-item\"><entry name=\"object\"><entry name=\"Label\" type=\"json-property\"><entry name=\"string\"/>";
    for original in ["root-json.mfd", "root-edi.mfd", "nested-json.mfd"] {
        let original = read(original);
        let unfed = without_vertex(&original, 10, 200);
        let unconsumed = without_vertex(&original, 201, 61);
        let forward0 = forward("forward0", 200, 201);
        let reforward = |replacement: String| replace_once(&original, &forward0, &replacement);
        for (label, design) in [
            ("missing sequence", unconsumed.clone()),
            ("conflicting sequence feeds", with_vertices(&original, &[(10, 61)])),
            ("missing structural input", unfed.clone()),
            ("conflicting structural inputs", with_vertices(&original, &[(201, 200)])),
            ("descendant override", with_vertices(&reforward(forward0.replace("<entry name=\"string\"/>", "<entry name=\"string\" inpkey=\"202\"/>")), &[(11, 202)])),
            ("item structural input", with_vertices(&reforward(forward0.replace(item, &item.replace("type=\"json-item\"", "type=\"json-item\" inpkey=\"203\""))), &[(10, 203)])),
            ("forward control", with_vertices(&reforward(forward0.replace("<root><entry name=\"document\">", "<root><entry name=\"compute-when\" inpkey=\"209\"/><entry name=\"document\">")), &[(101, 209)])),
            ("narrowed forward cardinality", reforward(forward0.replace("forward.schema.json", "narrow-forward.schema.json"))),
            ("widened forward items", reforward(forward0.replace("forward.schema.json", "wide-forward.schema.json"))),
            ("filtered forward", with_vertices(&with_components(&unfed, filter), &[(10, 300), (303, 301), (302, 200)])),
            ("sorted forward", with_vertices(&with_components(&unfed, sort), &[(10, 310), (12, 311), (312, 200)])),
            ("windowed forward", with_vertices(&with_components(&unfed, window), &[(10, 320), (323, 321), (322, 200)])),
            ("distinct forward", with_vertices(&with_components(&unfed, distinct), &[(10, 330), (331, 200)])),
            ("grouped forward", with_vertices(&with_components(&unfed, group), &[(10, 340), (11, 341), (342, 200)])),
            ("filtered count", with_vertices(&with_components(&unconsumed, filter), &[(201, 300), (303, 301), (302, 61)])),
            ("generated sequence", with_vertices(&with_components(&unconsumed, tokenize), &[(354, 350), (355, 351), (352, 61)])),
            ("UDF sequence", with_vertices(&with_components(&unfed, udf_call), &[(10, 360), (361, 200)]).replace("</mapping>", &format!("{udf_definition}</mapping>"))),
            ("self-forwarding cycle", with_vertices(&unfed, &[(201, 200)])),
            ("scalar sequence", with_vertices(&unconsumed, &[(11, 61)])),
            ("nonrepeating record as collection", with_vertices(&unconsumed, &[(21, 61)])),
            ("nonrepeating forward selection", reforward(forward0.replace("<entry name=\"Items\" type=\"json-property\"/><entry name=\"array\"/>", ""))),
        ] {
            assert_ne!(design, original, "{label}");
            refuses(&design, label);
        }
    }
    // Two chained forwards that feed each other never reach a physical source.
    let chained = read("chain-json.mfd");
    refuses(
        &with_vertices(&without_vertex(&chained, 10, 200), &[(211, 200)]),
        "forward cycle",
    );
    for (design, from, to) in [
        (
            "root-json.mfd",
            "\"forward.schema.json\"",
            "\"missing.schema.json\"",
        ),
        (
            "chain-json.mfd",
            "\"forward.schema.json\"",
            "\"missing.schema.json\"",
        ),
        (
            "root-json.mfd",
            "\"source.schema.json\"",
            "\"missing.schema.json\"",
        ),
        (
            "root-json.mfd",
            "\"variable.schema.json\"",
            "\"missing.schema.json\"",
        ),
    ] {
        let original = read(design);
        let label = format!("{design}: unresolved {to}");
        refuses_unresolved(&original.replacen(from, to, 1), &label);
    }
}

#[test]
fn counts_never_witness_presence_or_bypass_parent_frames() {
    for design in ["root-json.mfd", "root-edi.mfd", "nested-json.mfd"] {
        let original = read(design);
        refuses(
            &without_vertex(&original, 101, 22),
            &format!("{design}: count-only record"),
        );
    }
    let filter = r#"<component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="170"/><datapoint pos="1" key="171"/></sources><targets><datapoint pos="0" key="172"/></targets></component><component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="173"/></targets><data><constant value="true" datatype="boolean"/></data></component>"#;
    for design in ["nested-json.mfd", "nested-edi.mfd", "direct-edi.mfd"] {
        let original = read(design);
        let undriven = without_vertex(&original, 9, 40);
        let controlled = with_vertices(
            &with_components(&undriven, filter),
            &[(9, 170), (173, 171), (172, 40)],
        );
        refuses(&controlled, &format!("{design}: controlled parent"));
        refuses(&undriven, &format!("{design}: uniterated parent"));
        refuses(
            &with_vertices(&original, &[(8, 40)]),
            &format!("{design}: competing parent drivers"),
        );
        // The counted collection itself drives the parent, so the count would
        // reduce one current item and its outer repetition has no frame.
        refuses(
            &with_vertices(&undriven, &[(10, 40)]),
            &format!("{design}: counted collection as parent"),
        );
    }
}
