use super::*;
use mfd::{ImportOptions, ResourceBindingKindV1, ResourceBindingV1, ResourceReadSetV1};
use sha2::{Digest, Sha256};

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

#[test]
fn natural_and_prepared_bound_buffers_keep_private_exact_provenance() {
    let fixture = Fixture::new();
    let path = fixture.0.join("mapping.mfd");
    let readset = ResourceReadSetV1::default();
    let options = ImportOptions::default().with_resource_readset_v1(readset.clone());
    let actual = mfd::import_with_options(&path, &options).unwrap();
    assert!(actual.warnings.is_empty(), "{:?}", actual.warnings);
    let records = readset.take_records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, ResourceBindingKindV1::MappingLibrary);
    assert_eq!(records[0].expected_sha256, None);
    assert_eq!(
        records[0].sha256,
        digest(format!("<mapping>{IDENTITY}</mapping>").as_bytes())
    );
    let definitions = readset.take_mapping_library_definitions();
    assert_eq!(definitions.len(), 1);
    assert_eq!(
        definitions[0].declaring_document,
        path.canonicalize().unwrap()
    );
    assert_eq!(definitions[0].declaring_sha256, digest(MAIN.as_bytes()));
    assert_eq!(definitions[0].identity.name, "Identity");
    assert_eq!(definitions[0].declaration_xml, IDENTITY);
    assert!(definitions[0].refusal.is_none());
    let project = serde_json::to_string(&actual.project).unwrap();
    assert!(!project.contains("declaration_xml"));
    assert!(!project.contains("helpers.mfd"));

    std::fs::create_dir(fixture.0.join("relocated")).unwrap();
    let library = format!("<mapping>{IDENTITY}</mapping>");
    fixture.write("relocated/selected.mfd", &library);
    let binding = ResourceBindingV1::new(
        ResourceBindingKindV1::MappingLibrary,
        &path,
        "helpers.mfd",
        fixture.0.join("relocated"),
        "selected.mfd",
        digest(library.as_bytes()),
    )
    .unwrap()
    .with_origin_sha256(digest(MAIN.as_bytes()))
    .prepare()
    .unwrap();
    fixture.write("helpers.mfd", "<mapping><not-the-bound-owner/></mapping>");
    fixture.write(
        "relocated/selected.mfd",
        "<mapping><changed-after-prepare/></mapping>",
    );
    let options = options.with_resource_bindings_v1([binding]).unwrap();
    let prepared = mfd::import_with_options(&path, &options).unwrap();
    assert!(prepared.warnings.is_empty(), "{:?}", prepared.warnings);
    let records = readset.take_records();
    assert_eq!(records.len(), 1);
    assert!(records[0].from_prepared_snapshot && records[0].delivered);
    assert_eq!(records[0].expected_sha256, Some(digest(library.as_bytes())));
    assert_eq!(
        serde_json::to_value(actual.project).unwrap(),
        serde_json::to_value(prepared.project).unwrap()
    );
}

#[test]
fn mismatched_authority_never_falls_back_to_a_natural_library() {
    for origin_mismatch in [false, true] {
        let fixture = Fixture::new();
        let path = fixture.0.join("mapping.mfd");
        let text = std::fs::read(fixture.0.join("helpers.mfd")).unwrap();
        let binding = ResourceBindingV1::new(
            ResourceBindingKindV1::MappingLibrary,
            &path,
            "helpers.mfd",
            &fixture.0,
            "helpers.mfd",
            if origin_mismatch {
                digest(&text)
            } else {
                [0; 32]
            },
        )
        .unwrap()
        .with_origin_sha256(if origin_mismatch {
            [0; 32]
        } else {
            digest(MAIN.as_bytes())
        });
        let options = ImportOptions::default()
            .with_resource_bindings_v1([binding])
            .unwrap();
        assert!(
            matches!(mfd::import_with_options(&path, &options), Err(mfd::MfdError::Resource(reason)) if reason.contains("SHA-256") || reason.contains("digest"))
        );
    }
}

#[test]
fn closure_is_owner_relative_not_reexported_and_never_imports_library_main() {
    let fixture = Fixture::new();
    std::fs::create_dir(fixture.0.join("nested")).unwrap();
    let unrelated = IDENTITY.replace("Identity", "Unrelated");
    fixture.write("leaf.mfd", &format!("<mapping>{unrelated}</mapping>"));
    let imports = "<settings><libraries><import path=\"../leaf.mfd\" saverelative=\"1\"/></libraries></settings>";
    let unused_main = "<component name=\"unused-main\"><structure><children><component library=\"xml\" kind=\"14\"><data><document schema=\"DO-NOT-OPEN.xsd\" inputinstance=\"DO-NOT-READ.xml\"/></data></component><component library=\"http\" name=\"DO-NOT-CALL\"/><component library=\"db\" name=\"DO-NOT-QUERY\"/></children></structure></component>";
    fixture.write(
        "nested/owner.mfd",
        &format!("<mapping>{imports}{IDENTITY}{unused_main}</mapping>"),
    );
    fixture.write(
        "mapping.mfd",
        &MAIN.replace("helpers.mfd", "nested/owner.mfd"),
    );
    let actual = imported(&fixture);
    assert!(actual.warnings.is_empty(), "{:?}", actual.warnings);
    assert_eq!(actual.project.user_functions.len(), 1);
    assert_eq!(
        actual.project.user_functions.values().next().unwrap().name,
        "Identity"
    );
    fixture.write(
        "mapping.mfd",
        &MAIN
            .replace("helpers.mfd", "nested/owner.mfd")
            .replace("name=\"Identity\"", "name=\"Unrelated\""),
    );
    refused(&fixture, "symbol is not directly exported");
    fixture.write("nested/owner.mfd", &format!("<mapping>{}<settings><libraries><import path=\"../missing.mfd\" saverelative=\"1\"/></libraries></settings></mapping>", IDENTITY));
    fixture.write(
        "mapping.mfd",
        &MAIN.replace("helpers.mfd", "nested/owner.mfd"),
    );
    refused(&fixture, "declared mapping library unresolved");
}

#[test]
fn missing_unsupported_aliased_cyclic_and_escaping_imports_are_explicit() {
    for (library, main, message) in [
        (
            "<mapping/>",
            MAIN.replace("helpers.mfd", "missing.mfd"),
            "unresolved",
        ),
        (
            "<mapping/>",
            MAIN.replace("helpers.mfd", "helpers.mff"),
            "bounded .mfd",
        ),
        (
            "<mapping/>",
            MAIN.replace("saverelative=\"1\"", "saverelative=\"0\""),
            "document-local",
        ),
        (
            "<mapping/>",
            MAIN.replace("saverelative=\"1\"", "saverelative=\"1\" alias=\"other\""),
            "document-local",
        ),
        (
            "<mapping/>",
            MAIN.replace("helpers.mfd", "../escape.mfd"),
            "above package root",
        ),
        (
            "<mapping><settings><libraries><import path=\"helpers.mfd\" saverelative=\"1\"/></libraries></settings></mapping>",
            MAIN.to_string(),
            "file import cycle",
        ),
        (
            "<!DOCTYPE mapping [<!ENTITY bad 'x'>]><mapping/>",
            MAIN.to_string(),
            "DTD/entity",
        ),
    ] {
        let fixture = Fixture::new();
        fixture.write("helpers.mfd", library);
        fixture.write("mapping.mfd", &main);
        refused(&fixture, message);
    }
    for library in [
        "core",
        "webservice",
        "wsdl",
        "xml",
        "http",
        "binary",
        "edifact",
        "IsbnConverterService",
    ] {
        let fixture = Fixture::new();
        fixture.write(
            "mapping.mfd",
            &MAIN.replace("library=\"generic\"", &format!("library=\"{library}\"")),
        );
        refused(&fixture, "refused before built-in/schema/HTTP dispatch");
    }
}

#[test]
fn refused_signature_evidence_retains_exact_metadata_without_admitting_a_call() {
    let fixture = Fixture::new();
    let definition = IDENTITY.replace("<input datatype=\"string\"/>", "<input datatype=\"string\"/><parameter name=\"value\" usageKind=\"input\" optional=\"1\"/>");
    fixture.write("helpers.mfd", &format!("<mapping>{definition}</mapping>"));
    let readset = ResourceReadSetV1::default();
    let options = ImportOptions::default().with_resource_readset_v1(readset.clone());
    let actual = mfd::import_with_options(&fixture.0.join("mapping.mfd"), &options).unwrap();
    assert!(actual.warnings.iter().any(|w| w.contains("optional")));
    let definitions = readset.mapping_library_definitions();
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].declaration_xml, definition);
    assert!(
        definitions[0]
            .refusal
            .as_ref()
            .unwrap()
            .contains("optional")
    );
    assert!(actual.project.user_functions.is_empty());
    assert!(readset.records()[0].delivered);
}

#[test]
fn library_depth_definition_and_file_delivery_limits_fail_closed() {
    let fixture = Fixture::new();
    for index in 0..17 {
        fixture.write(&format!("level-{index}.mfd"), &if index == 16 { "<mapping/>".into() } else { format!("<mapping><settings><libraries><import path=\"level-{}.mfd\" saverelative=\"1\"/></libraries></settings></mapping>", index + 1) });
    }
    fixture.write("mapping.mfd", &MAIN.replace("helpers.mfd", "level-0.mfd"));
    refused(&fixture, "count/depth budget");
    fixture.write("mapping.mfd", MAIN);
    fixture.write(
        "helpers.mfd",
        &format!("<mapping>{}</mapping>", IDENTITY.repeat(513)),
    );
    refused(&fixture, "definition count budget");
    fixture.write(
        "helpers.mfd",
        &format!("<mapping><!--{}--></mapping>", "x".repeat(8 * 1024 * 1024)),
    );
    refused(&fixture, "byte limit");
}

#[cfg(unix)]
#[test]
fn linked_library_is_not_a_natural_or_transitive_authority() {
    let fixture = Fixture::new();
    fixture.write("real.mfd", &format!("<mapping>{IDENTITY}</mapping>"));
    std::fs::remove_file(fixture.0.join("helpers.mfd")).unwrap();
    std::os::unix::fs::symlink("real.mfd", fixture.0.join("helpers.mfd")).unwrap();
    refused(&fixture, "linked");
}

#[test]
fn duplicated_expression_paths_exhaust_shared_construction_before_expansion() {
    let fixture = Fixture::new();
    let mut body = String::new();
    let mut edges = String::new();
    let mut feed = 1000;
    // Only sixteen saved operations; interpreting the fan-out as a tree would
    // expand exponentially. A shared preallocation quota must stop that work.
    for layer in 0..16 {
        let input = 2000 + layer * 3;
        let output = input + 2;
        body.push_str(&format!(r#"<component library="core" kind="5" uid="{}" name="concat"><sources><datapoint pos="0" key="{input}"/><datapoint pos="1" key="{}"/></sources><targets><datapoint key="{output}"/></targets></component>"#, layer + 200, input + 1));
        edges.push_str(&format!(r#"<vertex vertexkey="{feed}"><edges><edge vertexkey="{input}"/><edge vertexkey="{}"/></edges></vertex>"#, input + 1));
        feed = output;
    }
    edges.push_str(&format!(
        r#"<vertex vertexkey="{feed}"><edges><edge vertexkey="1001"/></edges></vertex>"#
    ));
    let definition = IDENTITY
        .replace("</children>", &format!("{body}</children>"))
        .replace(
            "<vertex vertexkey=\"1000\"><edges><edge vertexkey=\"1001\"/></edges></vertex>",
            &edges,
        );
    fixture.write("helpers.mfd", &format!("<mapping>{definition}</mapping>"));
    assert!(
        matches!(mfd::import(&fixture.0.join("mapping.mfd")), Err(mfd::MfdError::Resource(reason)) if reason.contains("shared construction"))
    );
}
