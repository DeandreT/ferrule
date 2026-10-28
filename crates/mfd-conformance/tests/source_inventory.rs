//! Source inventories describe documentation, never execution qualification.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

#[path = "source_inventory/com_names.rs"]
mod com_names;

#[path = "source_inventory/schema_wrappers.rs"]
mod schema_wrappers;

fn load(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/mfd-2026r2-source-inventory")
        .join(name);
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn source_only(value: &Value) {
    match value {
        Value::Object(fields) => {
            for (name, value) in fields {
                match name.as_str() {
                    "runtime_proof" => assert_eq!(value, &Value::Bool(false)),
                    "status" => assert_eq!(value, "unassessed"),
                    "applicability" => assert_eq!(value, "unknown"),
                    "execution_evidence" | "execution_receipts" => {
                        assert!(value.as_array().unwrap().is_empty());
                    }
                    "text_lines" | "physical_pdf_pages" => {
                        let range = value.as_array().unwrap();
                        assert!(!range.is_empty());
                        let numbers: Vec<_> = range.iter().map(|n| n.as_u64().unwrap()).collect();
                        assert!(numbers.iter().all(|n| *n > 0));
                        assert!(numbers.windows(2).all(|pair| pair[0] <= pair[1]));
                        if name == "physical_pdf_pages" {
                            assert!(numbers.iter().all(|n| *n <= 1751));
                        }
                    }
                    _ => {}
                }
                source_only(value);
            }
        }
        Value::Array(values) => values.iter().for_each(source_only),
        _ => {}
    }
}

fn unique_ids(values: &[Value], field: &str) {
    let mut seen = BTreeSet::new();
    for value in values {
        let id = value[field].as_str().unwrap();
        assert!(seen.insert(id), "duplicate source identity: {id}");
    }
}

#[test]
fn manual_reviews_preserve_finite_coverage_and_source_only_assessments() {
    let functions = load("functions.json");
    let constructors = load("constructors.json");
    let nonleaf = load("nonleaf.json");
    let functions_units = functions["source_units"].as_array().unwrap();
    let constructor_units = constructors["source_units"]["xs_constructors"]
        .as_array()
        .unwrap();
    let parent_units = nonleaf["source_units"].as_array().unwrap();
    assert_eq!(functions_units.len(), 319);
    assert_eq!(constructor_units.len(), 43);
    assert_eq!(nonleaf["coverage"].as_array().unwrap().len(), 191);
    assert!(
        nonleaf["unreviewed_parent_pages"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        nonleaf["unresolved_parent_bindings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    unique_ids(functions_units, "research_id");
    unique_ids(constructor_units, "research_id");
    unique_ids(parent_units, "research_id");
    for inventory in [&functions, &constructors, &nonleaf] {
        source_only(inventory);
    }
}

#[test]
fn provisional_com_catalog_does_not_claim_pinned_release_or_execution() {
    let com = load("com-api.json");
    assert_eq!(com["release_binding"], "unestablished");
    assert_eq!(com["interfaces"].as_array().unwrap().len(), 22);
    let members = com["members"].as_array().unwrap();
    assert_eq!(members.len(), 261);
    assert_eq!(com["enumerations"].as_array().unwrap().len(), 20);
    unique_ids(members, "id");
    for source in com["source_documents"].as_array().unwrap() {
        assert_eq!(source["release_binding"], "unestablished");
        let hash = source["raw_sha256"].as_str().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
    source_only(&com);
}

#[test]
fn external_references_keep_historical_and_runtime_boundaries_explicit() {
    let references = load("external-references.json");
    let rows = references["references"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    unique_ids(rows, "id");
    for row in rows {
        assert!(
            row["requested_url"]
                .as_str()
                .unwrap()
                .starts_with("https://")
        );
        assert!(
            row["resolved_url"]
                .as_str()
                .unwrap()
                .starts_with("https://")
        );
        let hash = row["raw_sha256"].as_str().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(!row["boundary"].as_str().unwrap().is_empty());
    }
    source_only(&references);
}

#[test]
fn supporting_declarations_preserve_source_and_installer_boundaries() {
    let supporting = load("supporting.json");
    assert_eq!(supporting["inventory_complete"], false);
    let io = supporting["io_classes"].as_array().unwrap();
    let exceptions = supporting["vendor_exception_classes"].as_array().unwrap();
    assert_eq!(io.len(), 24);
    assert_eq!(exceptions.len(), 16);
    unique_ids(io, "research_id");
    unique_ids(exceptions, "research_id");
    assert_eq!(
        supporting["shared_cpp_exception_methods"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        supporting["platform_exception_references"]
            .as_array()
            .unwrap()
            .len(),
        9
    );
    let requests = supporting["installer_artifact_requests"]["bundles"]
        .as_array()
        .unwrap();
    assert_eq!(requests.len(), 6);
    for request in requests {
        let (expected, observation) = match request["request_id"].as_str().unwrap() {
            "installer-release-manifest" => {
                ("partial_selected_members", "artifact-observations.json")
            }
            "csharp-convertproducts-io" | "java-convertproducts-io" => {
                ("source_bundle_obtained", "artifact-observations.json")
            }
            "cpp-schema-wrapper" | "java-schema-wrapper" => {
                ("source_bundle_obtained", "schema-wrappers.json")
            }
            "csharp-schema-wrapper" => ("partial_source_bundle_obtained", "schema-wrappers.json"),
            other => panic!("unreviewed artifact request {other}"),
        };
        assert_eq!(request["resolution"], expected);
        assert_eq!(request["observation"], observation);
    }
    source_only(&supporting);
}

#[test]
fn artifact_observations_do_not_promote_execution_or_completeness() {
    let observation = load("artifact-observations.json");
    assert_eq!(observation["inventory_complete"], false);
    assert_eq!(observation["runtime_proof"], false);
    assert_eq!(observation["release"]["release"], "2026r2");
    assert_eq!(observation["release"]["executable_version"], "28.2.0.0");
    assert_eq!(
        observation["release"]["archive_sha256"],
        "16fc9298366a93921d85a9165beb34736e227ba0985699743b53a5c6c6758278"
    );
    assert!(
        observation["release"]["archive_url"]
            .as_str()
            .unwrap()
            .starts_with("https://cdn.sw.altova.com/v2026r2/")
    );
    let members = observation["matched_members"].as_array().unwrap();
    assert_eq!(members.len(), 6);
    unique_ids(members, "name");
    for value in members.iter().chain(
        observation["release"]["container_chain"]
            .as_array()
            .unwrap(),
    ) {
        let hash = value["sha256"].as_str().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
    let generated = &observation["generated_source"];
    assert_eq!(generated["documented_io_classes_matched"], 24);
    assert_eq!(generated["csharp_files"], 41);
    assert_eq!(generated["java_files"], 98);
    let copied = &observation["copied_application_tree"];
    assert_eq!(copied["matched_files"], 1981);
    assert_eq!(copied["original_manifest_files"], 1980);
    assert_eq!(copied["additional_installer_resources"], 1);
    assert_eq!(copied["unexpected_or_missing_files"], 0);
    for field in ["support_cab_sha256", "comparison_manifest_sha256"] {
        let hash = copied[field].as_str().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
    assert!(
        generated["csharp_project_target"]
            .as_str()
            .unwrap()
            .contains("v4.8.1")
    );
    let declarations = generated["public_declaration_observations"]
        .as_array()
        .unwrap();
    assert_eq!(declarations.len(), 4);
    unique_ids(declarations, "type");
    assert_eq!(
        generated["public_declaration_observations"],
        serde_json::json!([
            {"type":"Altova.IO.StreamOutput","constructor_parameter_types":[["System.IO.Stream"],["System.String","System.IO.FileMode"]]},
            {"type":"Altova.IO.StringOutput","constructor_parameter_types":[["System.Text.StringBuilder"]]},
            {"type":"com.altova.io.StreamOutput","constructor_parameter_types":[["java.io.OutputStream"]],"getStream_return_type":"java.io.OutputStream"},
            {"type":"com.altova.io.StringOutput","constructor_parameter_types":[[]],"getString_return_type":"java.lang.StringBuffer"}
        ])
    );
    assert!(!observation["remaining"].as_array().unwrap().is_empty());
    let installed = observation["installed_declarations"].as_array().unwrap();
    assert_eq!(installed.len(), 4);
    unique_ids(installed, "id");
    assert_eq!(installed[0]["return_type"], "const std::exception*");
    assert_eq!(
        installed[0]["types"],
        serde_json::json!(["altova::Error", "altova::Exception"])
    );
    assert_eq!(
        installed[1]["public_type"],
        "com.altova.SourceInstanceUnavailableException"
    );
    let parameter = &installed[2];
    assert_eq!(parameter["public_member"], "IComponent.GenerateOutput");
    assert_eq!(parameter["cab_member"], "MapForce.tlb_1193");
    assert_eq!(parameter["sha256"], members[1]["sha256"]);
    assert_eq!(parameter["type_info_index"], 11);
    assert_eq!(parameter["member_record_index"], 10);
    assert_eq!(parameter["dispatch_member_id"], 9);
    assert_eq!(
        parameter["parameter"],
        serde_json::json!({
            "index":0, "name":"pbError", "type":"VARIANT*",
            "parameter_flags":3, "direction":["in","out"]
        })
    );
    let catalog = load("com-api.json");
    let documented = catalog["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["id"] == parameter["catalog_comparison"]["member"])
        .unwrap();
    let documented_parameter = &documented["contract"]["parameters"][0];
    assert_eq!(documented_parameter["name"], "pbError");
    assert_eq!(documented_parameter["declared_type"], "Boolean");
    assert_eq!(documented_parameter["direction"], "out");
    assert_eq!(
        parameter["catalog_comparison"]["release_binding"],
        "unestablished"
    );
    let enumeration = &installed[3];
    assert_eq!(enumeration["public_type"], "ENUMProjectType");
    assert_eq!(enumeration["type_info_index"], 4);
    assert_eq!(enumeration["sha256"], parameter["sha256"]);
    assert_eq!(
        enumeration["members"],
        serde_json::json!([
            {"name":"eVisualStudio2026Project","value":16,"encoded_value":"8c000010"},
            {"name":"eDotNet10_0","value":17,"encoded_value":"8c000011"},
            {"name":"eProjectTypeCount","value":18,"encoded_value":"8c000012"}
        ])
    );
    let documented_enum = catalog["enumerations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "ENUMProjectType")
        .unwrap();
    let documented_members = documented_enum["members"].as_array().unwrap();
    let count = documented_members
        .iter()
        .find(|member| member["name"] == "eProjectTypeCount")
        .unwrap();
    assert_eq!(count["value_literal"], "16");
    for name in ["eVisualStudio2026Project", "eDotNet10_0"] {
        assert!(
            !documented_members
                .iter()
                .any(|member| member["name"] == name)
        );
    }
    assert_eq!(
        enumeration["catalog_comparison"]["release_binding"],
        "unestablished"
    );
    for declaration in installed {
        let hash = declaration["sha256"].as_str().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
    source_only(&observation);
}
