use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use mapping::ScopeIteration;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

struct TestPackage(PathBuf);

impl Drop for TestPackage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn import_control(design: &str, folder: &str, label: &str) -> mfd::Imported {
    import_control_with_resources(design, folder, label, &[])
}

fn import_control_with_resources(
    design: &str,
    folder: &str,
    label: &str,
    resources: &[(&str, &str)],
) -> mfd::Imported {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let directory = TestPackage(std::env::temp_dir().join(format!(
        "ferrule-constructed-output-{}-{}-{label}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
    )));
    std::fs::create_dir(&directory.0).unwrap();
    for name in [
        "constructed-variable-drivers",
        "constructed-variable-copy-all",
        "constructed-variable-conditionals",
        "literal-group-drivers",
    ] {
        let destination = directory.0.join(name);
        std::fs::create_dir(&destination).unwrap();
        for entry in std::fs::read_dir(fixture_root().join(name)).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), destination.join(entry.file_name())).unwrap();
        }
    }
    for (name, contents) in resources {
        std::fs::write(directory.0.join(folder).join(name), contents).unwrap();
    }
    let path = directory.0.join(folder).join("control.mfd");
    std::fs::write(&path, design).unwrap();
    mfd::import_with_options(
        &path,
        &mfd::ImportOptions::default().with_package_root(&directory.0),
    )
    .unwrap()
}

fn record_fixture(copy_all: bool) -> (&'static str, String) {
    let (folder, file) = if copy_all {
        ("constructed-variable-copy-all", "json-object.mfd")
    } else {
        ("constructed-variable-drivers", "json.mfd")
    };
    (
        folder,
        std::fs::read_to_string(fixture_root().join(folder).join(file)).unwrap(),
    )
}

fn insert_component(design: &str, component: &str) -> String {
    assert!(design.contains("</children>"));
    design.replacen("</children>", &format!("{component}</children>"), 1)
}

fn replace_component(design: &str, name: &str, replacement: &str) -> String {
    let document = roxmltree::Document::parse(design).unwrap();
    let component = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some(name))
        .unwrap();
    let mut result = design.to_string();
    result.replace_range(component.range(), replacement);
    result
}

fn constant(output: u32) -> String {
    format!(
        r#"<component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="{output}"/></targets><data><constant value="5" datatype="integer"/></data></component>"#,
    )
}

fn variable(design: &str, output: u32) -> String {
    let document = roxmltree::Document::parse(design).unwrap();
    let component = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("buffer"))
        .unwrap();
    design[component.range()]
        .replace("name=\"buffer\"", "name=\"other_buffer\"")
        .replace("inpkey=\"20\"", "inpkey=\"220\"")
        .replace("inpkey=\"22\"", "inpkey=\"222\"")
        .replace("inpkey=\"24\"", "inpkey=\"224\"")
        .replace("outkey=\"21\"", &format!("outkey=\"{output}\""))
        .replace("outkey=\"23\"", "outkey=\"223\"")
        .replace("outkey=\"25\"", "outkey=\"225\"")
}

fn udf_collision(design: &str, output: u32) -> String {
    // An independent zero-input scalar definition supplies the competing key.
    let call = format!(
        r#"<component name="fixed-value" library="helpers" kind="19"><data><root rootindex="1"><entry name="Value" outkey="{output}" componentid="701"/></root></data></component>"#,
    );
    let definition = r#"<component name="fixed-value" library="helpers"><structure><children><component name="constant" library="core" uid="700" kind="2"><targets><datapoint pos="0" key="7000"/></targets><data><constant value="5" datatype="integer"/></data></component><component name="Value" library="core" uid="701" kind="7"><sources><datapoint pos="0" key="7001"/></sources><data><output datatype="integer"/></data></component></children><graph><vertices><vertex vertexkey="7000"><edges><edge vertexkey="7001"/></edges></vertex></vertices></graph></structure></component>"#;
    insert_component(design, &call).replace("</mapping>", &format!("{definition}</mapping>"))
}

fn refusal(imported: &mfd::Imported, key: u32, field: &str, label: &str) {
    let expected = format!("competing declared output producers for key {key}");
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains(&expected)),
        "{label}: {:?}",
        imported.warnings,
    );
    assert!(
        imported
            .project
            .root
            .children
            .iter()
            .all(|scope| scope.target_field != field),
        "{label}: a refused driver retained its target scope",
    );
}

#[test]
fn constructed_records_refuse_competing_function_outputs_before_copy_all_or_plain_fallback() {
    for copy_all in [true, false] {
        let (folder, original) = record_fixture(copy_all);
        let design = insert_component(&original, &constant(21));
        let imported = import_control(&design, folder, &format!("function-{copy_all}"));
        refusal(&imported, 21, "Records", "record/function output collision");
    }
}

#[test]
fn constructed_records_refuse_duplicate_intermediate_outputs() {
    for copy_all in [true, false] {
        let (folder, original) = record_fixture(copy_all);
        let design = insert_component(&original, &variable(&original, 21));
        let imported = import_control(&design, folder, &format!("variable-{copy_all}"));
        refusal(&imported, 21, "Records", "duplicate variable output");
    }
}

#[test]
fn constructed_records_refuse_differing_output_paths_inside_one_variable() {
    let schema = r#"{"type":"object","properties":{"First":{"type":"object","properties":{"Text":{"type":"string"},"Count":{"type":"integer"}},"additionalProperties":false},"Second":{"type":"object","properties":{"Text":{"type":"string"},"Count":{"type":"integer"}},"additionalProperties":false}},"additionalProperties":false,"title":"Buffer"}"#;
    let buffer = r#"<component name="buffer" library="json" kind="31"><data><root><entry name="document"><entry name="object"><entry name="First" type="json-property"><entry name="object" inpkey="200" outkey="21"><entry name="Text" type="json-property"><entry name="string" inpkey="202" outkey="203"/></entry><entry name="Count" type="json-property"><entry name="integer" inpkey="204" outkey="205"/></entry></entry></entry><entry name="Second" type="json-property"><entry name="object" inpkey="220" outkey="21"><entry name="Text" type="json-property"><entry name="string" inpkey="222" outkey="223"/></entry><entry name="Count" type="json-property"><entry name="integer" inpkey="224" outkey="225"/></entry></entry></entry></entry></entry></root><parameter usageKind="variable"><root><entry name="root"/><entry name="object"/></root></parameter><json schema="two-records.schema.json"/></data></component>"#;
    for copy_all in [true, false] {
        let (folder, original) = record_fixture(copy_all);
        // First and Second are complete compatible record shapes. Normalization
        // selects Second for key 21 and its connected fields otherwise qualify.
        let design = replace_component(&original, "buffer", buffer)
            .replace("vertexkey=\"22\"", "vertexkey=\"222\"")
            .replace("vertexkey=\"24\"", "vertexkey=\"224\"")
            .replace("vertexkey=\"23\"", "vertexkey=\"223\"")
            .replace("vertexkey=\"25\"", "vertexkey=\"225\"");
        let imported = import_control_with_resources(
            &design,
            folder,
            &format!("raw-variable-{copy_all}"),
            &[("two-records.schema.json", schema)],
        );
        refusal(
            &imported,
            21,
            "Records",
            "differing raw variable output paths",
        );
    }
}

#[test]
fn constructed_records_refuse_connected_input_paths_hidden_by_normalization() {
    for copy_all in [true, false] {
        let (folder, original) = record_fixture(copy_all);
        let design = original.replace("inpkey=\"24\"", "inpkey=\"22\"");
        let imported = import_control(&design, folder, &format!("raw-input-{copy_all}"));
        refusal(
            &imported,
            21,
            "Records",
            "differing connected raw input paths",
        );
    }
}

fn source_with_references(original: &str, references: &str) -> String {
    let source = format!(
        r#"<component name="source" library="json" kind="31"><data><root><entry name="FileInstance"><entry name="document"><entry name="root"><entry name="object">{references}</entry></entry></entry></entry></root><json schema="source-control.schema.json" inputinstance="source.json"/></data></component>"#
    );
    replace_component(original, "source", &source)
}

fn captured_source(kind: &str, request_key: Option<u32>, unused_output: Option<u32>) -> String {
    match kind {
        "get" => r#"<component name="source" library="webservice" kind="20"><data><root><entry name="HTTPMessage"><entry name="HTTPBody"><entry name="document" type="doc-xml"><document schemafile="../constructed-variable-drivers/source.xsd" root="{}Source" encoding="UTF-8"/><entry name="Source"><entry name="Count" type="attribute" outkey="12"/></entry></entry></entry></entry></root><wsdl kind="call" sourceMode="manual" url="https://example.test/captured" httpmethod="GET"/></data></component>"#.to_string(),
        "post" => {
            let request = request_key.map_or_else(String::new, |key| format!(r#"<root><entry name="HTTPMessage"><entry name="HTTPBody"><entry name="document" type="doc-json"><document encoding="UTF-8"/><entry name="root"><entry name="object"><entry name="query" type="json-property"><entry name="integer" inpkey="{key}"/></entry></entry></entry></entry></entry></entry></root>"#));
            format!(r#"<component name="source" library="webservice" kind="20"><data>{request}<root rootindex="1"><entry name="HTTPMessage"><entry name="HTTPBody"><entry name="document" type="doc-json"><document encoding="UTF-8"/><entry name="root"><entry name="object"><entry name="Count" type="json-property"><entry name="integer" outkey="12"/></entry></entry></entry></entry></entry></entry></root><wsdl kind="call" sourceMode="manual" url="https://example.test/captured" httpmethod="POST"/></data></component>"#)
        }
        "opaque" => {
            let unused = unused_output.map_or_else(String::new, |key| format!(r#"<entry name="unused" outkey="{key}"/>"#));
            format!(r#"<component name="source" library="user" kind="19"><data><root><entry name="result" componentid="700"><entry name="object"><entry name="Count" type="json-property"><entry name="integer" outkey="12"/></entry></entry></entry>{unused}</root></data></component>"#)
        }
        _ => unreachable!(),
    }
}

fn captured_record_fixture(copy_all: bool, source: &str) -> (&'static str, String) {
    let (folder, original) = record_fixture(copy_all);
    // The explicit scalar override keeps the opaque result visibly connected
    // to the target as required by its existing captured-source boundary.
    let design = replace_component(&original, "source", source)
        .replace("<vertex vertexkey=\"25\"><edges><edge vertexkey=\"32\"/></edges></vertex>", "")
        .replace("<vertex vertexkey=\"12\"><edges><edge vertexkey=\"24\"/></edges></vertex>", "<vertex vertexkey=\"12\"><edges><edge vertexkey=\"24\"/><edge vertexkey=\"32\"/></edges></vertex>");
    (folder, design)
}

#[test]
fn constructed_records_preserve_existing_captured_json_and_xml_scalar_sources() {
    for copy_all in [true, false] {
        for kind in ["get", "post", "opaque"] {
            let (folder, design) =
                captured_record_fixture(copy_all, &captured_source(kind, None, None));
            let imported = import_control(&design, folder, &format!("captured-{kind}-{copy_all}"));
            assert!(imported.project.extra_sources.is_empty());
            assert!(imported.project.source.child("Count").is_some());
            let result = if kind == "get" {
                assert!(imported.project.source_options.http_get.is_some());
                let source =
                    format_xml::from_str("<Source Count=\"7\"/>", &imported.project.source)
                        .unwrap();
                output_value(&imported, &source)
            } else {
                assert!(imported.project.source_options.external_source.is_some());
                value(&imported, r#"{"Count":7}"#)
            };
            assert_eq!(
                result,
                serde_json::json!({"Records":[{"Text":"created","Count":7}]})
            );
        }
    }
}

#[test]
fn constructed_records_refuse_a_captured_request_key_reused_by_its_response() {
    for copy_all in [true, false] {
        let (folder, design) =
            captured_record_fixture(copy_all, &captured_source("post", Some(12), None));
        let imported = import_control(&design, folder, &format!("captured-request-{copy_all}"));
        assert!(imported.project.source_options.external_source.is_some());
        refusal(
            &imported,
            21,
            "Records",
            "captured HTTP request/response key collision",
        );
    }
}

#[test]
fn constructed_records_refuse_a_captured_result_output_without_a_lowered_port() {
    for copy_all in [true, false] {
        let (folder, design) =
            captured_record_fixture(copy_all, &captured_source("opaque", None, Some(21)));
        let imported = import_control(&design, folder, &format!("captured-hidden-{copy_all}"));
        assert!(imported.project.source_options.external_source.is_some());
        refusal(&imported, 21, "Records", "unlowered captured result shadow");
    }
}

#[test]
fn constructed_records_refuse_differing_captured_scalar_output_paths() {
    for copy_all in [true, false] {
        for kind in ["post", "opaque"] {
            let source = captured_source(kind, None, None).replace("<entry name=\"Count\" type=\"json-property\"><entry name=\"integer\" outkey=\"12\"/></entry>", "<entry name=\"Count\" type=\"json-property\"><entry name=\"integer\" outkey=\"12\"/></entry><entry name=\"OtherCount\" type=\"json-property\"><entry name=\"integer\" outkey=\"12\"/></entry>");
            let (folder, design) = captured_record_fixture(copy_all, &source);
            let imported = import_control(
                &design,
                folder,
                &format!("captured-collision-{kind}-{copy_all}"),
            );
            assert!(imported.project.source_options.external_source.is_some());
            assert!(imported.project.source.child("Count").is_some());
            assert!(imported.project.source.child("OtherCount").is_some());
            refusal(
                &imported,
                21,
                "Records",
                "differing captured physical output paths",
            );
        }
    }
}

#[test]
fn identical_captured_scalar_references_preserve_complete_projects() {
    for copy_all in [true, false] {
        for kind in ["get", "post", "opaque"] {
            let source = captured_source(kind, None, None);
            let (folder, design) = captured_record_fixture(copy_all, &source);
            let baseline = import_control(
                &design,
                folder,
                &format!("captured-identical-baseline-{kind}-{copy_all}"),
            );
            let duplicate = if kind == "get" {
                source.replace("<entry name=\"Count\" type=\"attribute\" outkey=\"12\"/>", "<entry name=\"Count\" type=\"attribute\" outkey=\"12\"/><entry outkey=\"12\" type=\"attribute\" name=\"Count\"/>")
            } else {
                source.replace("<entry name=\"integer\" outkey=\"12\"/>", "<entry name=\"integer\" outkey=\"12\"/><entry outkey=\"12\" name=\"integer\"/>")
            };
            assert_ne!(duplicate, source);
            let (folder, design) = captured_record_fixture(copy_all, &duplicate);
            let imported = import_control(
                &design,
                folder,
                &format!("captured-identical-{kind}-{copy_all}"),
            );
            assert_eq!(
                mapping::project_file::encode_pretty(&imported.project).unwrap(),
                mapping::project_file::encode_pretty(&baseline.project).unwrap(),
            );
            let result = if kind == "get" {
                let input = format_xml::from_str("<Source Count=\"7\"/>", &imported.project.source)
                    .unwrap();
                output_value(&imported, &input)
            } else {
                value(&imported, r#"{"Count":7}"#)
            };
            assert_eq!(
                result,
                serde_json::json!({"Records":[{"Text":"created","Count":7}]})
            );
        }
    }
}

#[test]
fn a_connected_input_conflict_cannot_hide_outside_the_selected_record_path() {
    let schema = r#"{"type":"object","properties":{"First":{"type":"object","properties":{"Text":{"type":"string"},"Count":{"type":"integer"}},"additionalProperties":false},"Second":{"type":"object","properties":{"Text":{"type":"string"},"Count":{"type":"integer"}},"additionalProperties":false}},"additionalProperties":false,"title":"Buffer"}"#;
    // Key 222 is declared in the selected First record and normalized last into
    // the unrelated Second record. Its raw connected ownership still affects First.
    let buffer = r#"<component name="buffer" library="json" kind="31"><data><root><entry name="document"><entry name="object"><entry name="First" type="json-property"><entry name="object" inpkey="200" outkey="21"><entry name="Text" type="json-property"><entry name="string" inpkey="222" outkey="203"/></entry><entry name="Count" type="json-property"><entry name="integer" inpkey="204" outkey="205"/></entry></entry></entry><entry name="Second" type="json-property"><entry name="object" inpkey="220" outkey="221"><entry name="Text" type="json-property"><entry name="string" inpkey="222" outkey="223"/></entry><entry name="Count" type="json-property"><entry name="integer" inpkey="224" outkey="225"/></entry></entry></entry></entry></entry></root><parameter usageKind="variable"><root><entry name="root"/><entry name="object"/></root></parameter><json schema="two-records.schema.json"/></data></component>"#;
    for copy_all in [true, false] {
        let (folder, original) = record_fixture(copy_all);
        let design = replace_component(&original, "buffer", buffer)
            .replace("vertexkey=\"22\"", "vertexkey=\"222\"")
            .replace("vertexkey=\"24\"", "vertexkey=\"204\"")
            .replace("vertexkey=\"23\"", "vertexkey=\"203\"")
            .replace("vertexkey=\"25\"", "vertexkey=\"205\"");
        let imported = import_control_with_resources(
            &design,
            folder,
            &format!("hidden-input-{copy_all}"),
            &[("two-records.schema.json", schema)],
        );
        refusal(
            &imported,
            21,
            "Records",
            "connected raw input hidden outside selected path",
        );
    }
}

#[test]
fn constructed_records_refuse_differing_physical_scalar_output_paths_atomically() {
    let schema = r#"{"type":"object","properties":{"Count":{"type":"integer"},"OtherCount":{"type":"integer"}},"additionalProperties":false,"title":"Source"}"#;
    let references = r#"<entry name="Count" type="json-property"><entry name="integer" outkey="12"/></entry><entry name="OtherCount" type="json-property"><entry name="integer" outkey="12"/></entry>"#;
    for copy_all in [true, false] {
        let (folder, original) = record_fixture(copy_all);
        let design = source_with_references(&original, references);
        let imported = import_control_with_resources(
            &design,
            folder,
            &format!("raw-physical-{copy_all}"),
            &[("source-control.schema.json", schema)],
        );
        refusal(
            &imported,
            21,
            "Records",
            "differing raw physical output paths",
        );
    }
}

#[test]
fn constructed_records_refuse_differing_roles_at_one_physical_scalar_path() {
    let schema = r#"{"type":"object","properties":{"Count":{"type":"integer"}},"additionalProperties":false,"title":"Source"}"#;
    let references = r#"<entry name="Count" type="json-property"><entry name="integer" outkey="12"/><entry name="string" outkey="12"/></entry>"#;
    for copy_all in [true, false] {
        let (folder, original) = record_fixture(copy_all);
        let design = source_with_references(&original, references);
        let imported = import_control_with_resources(
            &design,
            folder,
            &format!("raw-role-{copy_all}"),
            &[("source-control.schema.json", schema)],
        );
        refusal(
            &imported,
            21,
            "Records",
            "differing raw physical output roles",
        );
    }
}

#[test]
fn constructed_records_refuse_cross_direction_output_keys() {
    for copy_all in [true, false] {
        let (folder, original) = record_fixture(copy_all);
        let design = original.replace("inpkey=\"20\"", "inpkey=\"21\"");
        let imported = import_control(&design, folder, &format!("raw-direction-{copy_all}"));
        refusal(
            &imported,
            21,
            "Records",
            "one key declared as input and output",
        );
    }
}

#[test]
fn constructed_records_refuse_unlowered_physical_output_declarations() {
    for copy_all in [true, false] {
        let (folder, original) = record_fixture(copy_all);
        // The ordinary reader chooses the first saved root entry. The other saved
        // entry still declares the scalar assignment key without a lowered port.
        let design = original.replacen(
            "</root>",
            "<entry name=\"ignored\" outkey=\"12\"/></root>",
            1,
        );
        let imported = import_control(&design, folder, &format!("raw-unlowered-{copy_all}"));
        refusal(
            &imported,
            21,
            "Records",
            "saved output without a lowered port",
        );
    }
}

#[test]
fn constructed_records_refuse_a_shadow_hidden_by_duplicate_function_output_positions() {
    for copy_all in [true, false] {
        let (folder, original) = record_fixture(copy_all);
        let shadow =
            constant(999).replace("</targets>", "<datapoint pos=\"0\" key=\"12\"/></targets>");
        let design = insert_component(&original, &shadow);
        let imported = import_control(&design, folder, &format!("raw-function-{copy_all}"));
        refusal(
            &imported,
            21,
            "Records",
            "raw function shadow of physical scalar",
        );
    }
}

#[test]
fn constructed_records_refuse_raw_shadows_through_scalar_aliases() {
    let folder = "constructed-variable-copy-all";
    let original =
        std::fs::read_to_string(fixture_root().join(folder).join("alias-json.mfd")).unwrap();
    let shadow =
        constant(999).replace("</targets>", "<datapoint pos=\"0\" key=\"403\"/></targets>");
    let imported = import_control(&insert_component(&original, &shadow), folder, "raw-alias");
    refusal(
        &imported,
        21,
        "Records",
        "raw function shadow of scalar alias",
    );
}

#[test]
fn constructed_records_refuse_raw_shadows_in_every_conditional_operand() {
    let folder = "constructed-variable-conditionals";
    let original =
        std::fs::read_to_string(fixture_root().join(folder).join("root-json.mfd")).unwrap();
    for copy_all in [true, false] {
        let original = if copy_all {
            original.clone()
        } else {
            original.replace("<connections><edge from=\"21\" to=\"30\"><data type=\"2\"/></edge></connections>", "")
                .replace("</vertices>", "<vertex vertexkey=\"23\"><edges><edge vertexkey=\"31\"/></edges></vertex><vertex vertexkey=\"25\"><edges><edge vertexkey=\"32\"/></edges></vertex></vertices>")
        };
        for output in [13, 12, 105] {
            let shadow = constant(999).replace(
                "</targets>",
                &format!("<datapoint pos=\"0\" key=\"{output}\"/></targets>"),
            );
            let imported = import_control(
                &insert_component(&original, &shadow),
                folder,
                &format!("raw-conditional-{copy_all}-{output}"),
            );
            refusal(
                &imported,
                21,
                "Records",
                "raw shadow in conditional condition, then or else operand",
            );
        }
    }
}

#[test]
fn constructed_records_refuse_competing_udf_outputs() {
    for copy_all in [true, false] {
        let (folder, original) = record_fixture(copy_all);
        let imported = import_control(
            &udf_collision(&original, 21),
            folder,
            &format!("udf-{copy_all}"),
        );
        refusal(&imported, 21, "Records", "record/UDF output collision");
        assert!(
            imported
                .warnings
                .iter()
                .all(|warning| !warning.contains("user-defined function")),
            "the authored competing UDF was skipped: {:?}",
            imported.warnings,
        );
    }
}

#[test]
fn constructed_records_refuse_a_primary_source_output_before_source_backed_resolution() {
    for copy_all in [true, false] {
        let (folder, original) = record_fixture(copy_all);
        let design = original
            .replace("outkey=\"12\"", "outkey=\"21\"")
            .replace("vertexkey=\"12\"", "vertexkey=\"21\"");
        let imported = import_control(&design, folder, &format!("primary-{copy_all}"));
        refusal(
            &imported,
            21,
            "Records",
            "record/primary source output collision",
        );
    }
}

#[test]
fn constructed_records_refuse_an_extra_source_output_before_source_backed_resolution() {
    for copy_all in [true, false] {
        let (folder, original) = record_fixture(copy_all);
        let source = format!(
            r#"<component name="other" library="json" kind="31"><data><root><entry name="FileInstance"><entry name="document"><entry name="root"><entry name="object"><entry name="Count" type="json-property"><entry name="integer" outkey="21"/></entry></entry></entry></entry></entry></root><json schema="{}source.schema.json" inputinstance="other.json"/></data></component>"#,
            if copy_all {
                "../constructed-variable-drivers/"
            } else {
                ""
            },
        );
        // Fix the original endpoint as primary so this is an actual extra source.
        let design = insert_component(&original, &source)
            .replacen("<mapping>", "<mapping ferrule-primary-source=\"1\">", 1)
            .replacen("name=\"source\"", "name=\"source\" uid=\"1\"", 1);
        let imported = import_control(&design, folder, &format!("extra-{copy_all}"));
        refusal(
            &imported,
            21,
            "Records",
            "record/extra source output collision",
        );
        assert_eq!(imported.project.extra_sources.len(), 1);
        assert_eq!(imported.project.extra_sources[0].name, "other");
    }
}

#[test]
fn literal_group_drivers_refuse_duplicate_function_outputs() {
    let folder = "literal-group-drivers";
    let original = std::fs::read_to_string(fixture_root().join(folder).join("root.mfd")).unwrap();
    let imported = import_control(
        &insert_component(&original, &constant(101)),
        folder,
        "literal",
    );
    refusal(&imported, 101, "Once", "duplicate literal output");
}

#[test]
fn literal_group_drivers_refuse_raw_output_multiplicity_hidden_by_normalization() {
    let folder = "literal-group-drivers";
    let original = std::fs::read_to_string(fixture_root().join(folder).join("root.mfd")).unwrap();
    for (label, design) in [
        (
            "position",
            original.replace(
                "<datapoint pos=\"0\" key=\"101\"/>",
                "<datapoint pos=\"0\" key=\"101\"/><datapoint pos=\"0\" key=\"999\"/>",
            ),
        ),
        (
            "container",
            original.replace(
                "<targets><datapoint pos=\"0\" key=\"101\"/></targets>",
                "<targets><datapoint pos=\"0\" key=\"101\"/></targets><targets/>",
            ),
        ),
    ] {
        assert_ne!(design, original);
        let imported = import_control(&design, folder, &format!("literal-raw-{label}"));
        refusal(&imported, 101, "Once", "raw literal output multiplicity");
    }
}

#[test]
fn literal_group_drivers_refuse_competing_udf_outputs() {
    let folder = "literal-group-drivers";
    let original = std::fs::read_to_string(fixture_root().join(folder).join("root.mfd")).unwrap();
    let imported = import_control(&udf_collision(&original, 101), folder, "literal-udf");
    refusal(&imported, 101, "Once", "literal/UDF output collision");
}

fn value(imported: &mfd::Imported, input: &str) -> serde_json::Value {
    let source = format_json::from_str(input, &imported.project.source).unwrap();
    output_value(imported, &source)
}

fn output_value(imported: &mfd::Imported, source: &ir::Instance) -> serde_json::Value {
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let result = engine::run(&imported.project, source).unwrap();
    serde_json::from_str(&format_json::to_string(&imported.project.target, &result).unwrap())
        .unwrap()
}

#[test]
fn unrelated_output_collisions_do_not_change_a_constructed_records_selected_producer() {
    let (folder, original) = record_fixture(true);
    let design = insert_component(&original, &format!("{}{}", constant(999), constant(999)));
    let imported = import_control(&design, folder, "unrelated");
    assert_eq!(
        value(&imported, r#"{"Count":7}"#),
        serde_json::json!({"Records":[{"Text":"created","Count":7}]}),
    );
}

#[test]
fn identical_saved_scalar_references_preserve_complete_constructed_projects() {
    for copy_all in [true, false] {
        let (folder, original) = record_fixture(copy_all);
        let baseline = import_control(&original, folder, &format!("identical-baseline-{copy_all}"));
        for (label, design) in [
            ("output", original.replace("<entry name=\"integer\" outkey=\"12\"/>", "<entry name=\"integer\" outkey=\"12\"/><entry outkey=\"12\" name=\"integer\"/>")),
            ("input", original.replace("<entry name=\"integer\" inpkey=\"24\" outkey=\"25\"/>", "<entry name=\"integer\" inpkey=\"24\" outkey=\"25\"/><entry outkey=\"25\" inpkey=\"24\" name=\"integer\"/>")),
        ] {
            let imported = import_control(&design, folder, &format!("identical-{label}-{copy_all}"));
            assert_eq!(mapping::project_file::encode_pretty(&imported.project).unwrap(), mapping::project_file::encode_pretty(&baseline.project).unwrap(), "{label}: identical reference changed the complete project");
            assert_eq!(value(&imported, r#"{"Count":7}"#), serde_json::json!({"Records":[{"Text":"created","Count":7}]}));
        }
    }
}

#[test]
fn unrelated_raw_physical_collisions_preserve_the_selected_scalar_output() {
    let schema = r#"{"type":"object","properties":{"Count":{"type":"integer"},"Unused":{"type":"integer"},"OtherUnused":{"type":"integer"}},"additionalProperties":false,"title":"Source"}"#;
    let references = r#"<entry name="Count" type="json-property"><entry name="integer" outkey="12"/></entry><entry name="Unused" type="json-property"><entry name="integer" outkey="99"/></entry><entry name="OtherUnused" type="json-property"><entry name="integer" outkey="99"/></entry>"#;
    for copy_all in [true, false] {
        let (folder, original) = record_fixture(copy_all);
        let design = source_with_references(&original, references);
        let imported = import_control_with_resources(
            &design,
            folder,
            &format!("unrelated-raw-{copy_all}"),
            &[("source-control.schema.json", schema)],
        );
        assert_eq!(
            value(&imported, r#"{"Count":7,"Unused":2,"OtherUnused":3}"#),
            serde_json::json!({"Records":[{"Text":"created","Count":7}]})
        );
    }
}

#[test]
fn ordinary_source_backed_group_traversal_keeps_its_existing_source_scope() {
    let folder = "literal-group-drivers";
    let original = std::fs::read_to_string(fixture_root().join(folder).join("root.mfd")).unwrap();
    let design = original.replace(
        "<vertex vertexkey=\"101\"><edges><edge vertexkey=\"20\"/></edges></vertex>",
        "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"20\"/></edges></vertex>",
    );
    let imported = import_control(&design, folder, "source-backed");
    assert_eq!(
        value(&imported, r#"{"Rows":[{"Label":"source"}]}"#),
        serde_json::json!({"Once":[{"Label":"created"}],"Empty":[{}]}),
    );
    let once = imported
        .project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Once")
        .unwrap();
    assert!(
        matches!(&once.iteration, ScopeIteration::Source(path) if path == &[String::from("Rows")])
    );
}

#[test]
fn constructed_copy_all_keeps_the_existing_exact_scalar_alias() {
    let folder = "constructed-variable-copy-all";
    let original =
        std::fs::read_to_string(fixture_root().join(folder).join("alias-json.mfd")).unwrap();
    let imported = import_control(&original, folder, "scalar-alias");
    assert_eq!(
        value(&imported, r#"{"Count":7}"#),
        serde_json::json!({"Records":[{"Text":"created","Count":7}]}),
    );
}
