//! Source generation must not become binary availability or runtime evidence.

use super::{load, source_only, unique_ids};

#[test]
fn schema_wrapper_sources_keep_language_and_binary_boundaries() {
    let wrappers = load("schema-wrappers.json");
    assert_eq!(wrappers["format_version"], 1);
    assert_eq!(wrappers["inventory_complete"], false);
    assert_eq!(wrappers["runtime_proof"], false);
    assert_eq!(
        wrappers["release"]["observation"],
        "artifact-observations.json"
    );
    let artifacts = load("artifact-observations.json");
    assert_eq!(
        wrappers["release"]["generator_version"],
        artifacts["release"]["executable_version"]
    );
    assert_eq!(wrappers["generation"]["generate_wrapper_classes"], true);
    assert_eq!(
        wrappers["documented_context"]["sources"],
        serde_json::json!([
            {"source_file":"cg_schema.html", "text_lines":[57071, 57153]},
            {"source_file":"cg_integrating_code.html", "text_lines":[57659, 57733]}
        ])
    );
    assert_eq!(
        artifacts["schema_wrapper_source"]["observation"],
        "schema-wrappers.json"
    );
    for hash in [
        &wrappers["release"]["generator_sha256"],
        &wrappers["generation"]["private_manifest_sha256"],
    ] {
        let hash = hash.as_str().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
    let profiles = wrappers["profiles"].as_array().unwrap();
    assert_eq!(profiles.len(), 4);
    unique_ids(profiles, "id");
    for (id, language, count, xml_library) in [
        ("csharp", "csharp", 53, None),
        ("java", "java", 212, None),
        ("cpp-msxml", "cpp", 180, Some("MSXML 6")),
        ("cpp-xerces", "cpp", 182, Some("Xerces 3.x")),
    ] {
        let profile = profiles.iter().find(|row| row["id"] == id).unwrap();
        assert_eq!(profile["language"], language);
        assert_eq!(profile["generated_file_count"], count);
        assert_eq!(profile["xml_library"].as_str(), xml_library);
        assert_eq!(profile["compiled"], false);
        assert_eq!(profile["executed"], false);
        assert_eq!(profile["compiled_artifacts_observed"], false);
        let sources = profile["wrapper_sources"].as_array().unwrap();
        assert_eq!(sources.len(), 2);
        unique_ids(sources, "path");
        for source in sources {
            let hash = source["sha256"].as_str().unwrap();
            assert_eq!(hash.len(), 64);
            assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
            let path = source["path"].as_str().unwrap();
            assert!(!path.starts_with('/'));
            assert!(!path.split('/').any(|part| part == ".."));
            assert!(path.ends_with(match language {
                "csharp" => ".cs",
                "java" => ".java",
                "cpp" => ".h",
                other => panic!("unexpected wrapper language {other}"),
            }));
        }
    }
    assert!(!wrappers["remaining"].as_array().unwrap().is_empty());
    source_only(&wrappers);
}

#[test]
fn schema_wrapper_requests_distinguish_source_from_compiled_delivery() {
    let supporting = load("supporting.json");
    let requests = supporting["installer_artifact_requests"]["bundles"]
        .as_array()
        .unwrap();
    for id in [
        "csharp-schema-wrapper",
        "java-schema-wrapper",
        "cpp-schema-wrapper",
    ] {
        let request = requests.iter().find(|row| row["request_id"] == id).unwrap();
        assert_eq!(
            request["resolution"],
            if id == "csharp-schema-wrapper" {
                "partial_source_bundle_obtained"
            } else {
                "source_bundle_obtained"
            }
        );
        assert_eq!(request["observation"], "schema-wrappers.json");
        assert_eq!(request["compiled_artifact_resolution"], "not_obtained");
    }
}

#[test]
fn wrapper_build_metadata_retains_execution_and_dependency_caveats() {
    let wrappers = load("schema-wrappers.json");
    let metadata = &wrappers["build_metadata"];
    assert_eq!(metadata["csharp"]["project_count"], 6);
    assert_eq!(
        metadata["csharp"]["target_framework"],
        ".NET Framework v4.8.1"
    );
    assert_eq!(
        metadata["csharp"]["library_projects"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(metadata["csharp"]["application_project"], "Mapping");
    let cpp = &metadata["cpp"];
    assert_eq!(cpp["project_count_per_profile"], 6);
    assert_eq!(cpp["platform_toolset"], "v145");
    assert_eq!(cpp["library_configuration_type"], "StaticLibrary");
    assert_eq!(cpp["application_configuration_type"], "Application");
    assert_eq!(cpp["use_of_mfc"], "Static");
    assert_eq!(
        cpp["configurations"],
        serde_json::json!(["Debug", "Release", "Unicode Debug", "Unicode Release"])
    );
    assert_eq!(cpp["platforms"], serde_json::json!(["Win32", "x64"]));
    let java = &metadata["java"];
    assert_eq!(java["default_target"], "test");
    assert_eq!(java["default_target_depends_on"], "compile");
    assert_eq!(
        java["default_target_executes"],
        "com.mapforce.MappingConsole"
    );
    assert!(java["explicit_source_target_release_version"].is_null());
    assert_eq!(java["jar_target_filename"], "Mapping.jar");
    assert_eq!(java["encoding"], "UTF-8");
    let profiles = wrappers["profiles"].as_array().unwrap();
    let xerces = profiles
        .iter()
        .find(|row| row["id"] == "cpp-xerces")
        .unwrap();
    let dependency = &xerces["external_dependencies"];
    assert_eq!(dependency["property_sheet"], "Altova/altova.xerces.props");
    assert_eq!(dependency["root_variable"], "XERCES3");
    assert_eq!(
        dependency["header_selected_libraries"]["_DEBUG"],
        "xerces-c_3D"
    );
    assert_eq!(
        dependency["header_selected_libraries"]["otherwise"],
        "xerces-c_3"
    );
    assert_eq!(dependency["availability"], "unverified");
}
