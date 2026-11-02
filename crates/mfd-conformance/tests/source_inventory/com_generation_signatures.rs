//! Frozen raw-declaration oracle, independent of the fragment and online returns.
//! These checks need no vendor files, network access, activation or execution.

use serde_json::{Value, json};

use super::{load, source_only, unique_ids};

const LIBRARY_SHA: &str = "785c484fbaa882cae724a3568b04650300eb18941084ac23eb2d041dfb4a2789";
const DUMP_SHA: &str = "672097183d6a6b0d80f3aa5f92d02172bb3a588609cb743bb82b73c342192bb4";
const CATALOG_SHA: &str = "d9a2fd8a4f8935598fdd2590f837fd6205931ba43c6218dc6b32a29fbe35c16a";

struct Method {
    name: &'static str,
    record: u64,
    dispid: u64,
    offset: u64,
    name_offset: u64,
    dump_lines: [u64; 2],
    input: Option<&'static str>,
    result: &'static str,
    result_index: u64,
    result_chain: [u64; 3],
    page_sha: &'static str,
}

// Independently transcribed from the four hash-pinned raw records and dump.
// In particular, neither result pointers nor native argument counts come from
// the online signatures, which omit the HRESULT/out-retval representation.
const METHODS: [Method; 4] = [
    Method {
        name: "GenerateCodeEx",
        record: 25,
        dispid: 22,
        offset: 37000,
        name_offset: 7164,
        dump_lines: [8846, 8868],
        input: Some("i_nLanguage"),
        result: "IErrorMarkers",
        result_index: 24,
        result_chain: [520, 512, 504],
        page_sha: "80b828a023a219dec2a866d1fa260863a17079e3e8cd17b8e5f67caa5ce35c1d",
    },
    Method {
        name: "GenerateOutputEx",
        record: 30,
        dispid: 27,
        offset: 37236,
        name_offset: 7408,
        dump_lines: [8944, 8961],
        input: None,
        result: "IAppOutputLines",
        result_index: 14,
        result_chain: [192, 184, 176],
        page_sha: "40204e16c749cb298ac4be6e2d42f5942c772df744392d0c9ca7b19b8d184a36",
    },
    Method {
        name: "GenerateOutputEx2",
        record: 32,
        dispid: 29,
        offset: 37384,
        name_offset: 8024,
        dump_lines: [9005, 9027],
        input: Some("strBaseDirectory"),
        result: "IOutputGenerationResult",
        result_index: 28,
        result_chain: [584, 576, 568],
        page_sha: "5c06bbb6e07eb168471ca9379c24748ca85bd5e3900a51f8a4b8d42204b2917c",
    },
    Method {
        name: "GenerateCodeEx2",
        record: 33,
        dispid: 30,
        offset: 37440,
        name_offset: 8284,
        dump_lines: [9028, 9050],
        input: Some("i_nLanguage"),
        result: "ICodeGenerationResult",
        result_index: 30,
        result_chain: [624, 616, 608],
        page_sha: "78bc29c9db0b9051753f2d204721e44af12fc23f7fa6da6e7a6cef3f82564246",
    },
];

const BOUNDARIES: [&str; 4] = [
    "The online signatures display the result object as a higher-level return value; the raw declarations return HRESULT and use a separate out/retval parameter. These are different declaration levels, not a contradiction or an ABI equivalence claim.",
    "The online enum names align with the named raw enum target. The online String parameter is compared with the raw BSTR declaration without claiming marshaling equivalence.",
    "Online constraints and error lists remain release-unbound documentation, not observed behavior. The visible-window or GUI-embedding wording belongs to GenerateOutputEx; do not transfer it to GenerateOutputEx2 or infer that Ex2 works without a visible window.",
    "Zero optional arguments and the recorded parameter flags describe these raw declarations only; no runtime default, nullability, coercion or omission behavior is inferred.",
];

const REMAINING: [&str; 4] = [
    "Other COM method signatures remain outside this bounded fragment; inventory_complete remains false.",
    "The pinned online catalog's release binding remains unestablished.",
    "No ABI, calling convention, marshaling, ownership, lifetime, runtime default, error behavior or backend support is established.",
    "No activation or execution evidence is supplied; canonical capability assessments are unchanged.",
];

fn keys(value: &Value, expected: &[&str]) {
    let actual: std::collections::BTreeSet<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(actual, expected.iter().copied().collect());
}

fn expected_parameters(m: &Method) -> Vec<Value> {
    let mut params = Vec::new();
    if let Some(name) = m.input {
        let (name_offset, ty) = if name == "i_nLanguage" {
            (
                7192,
                json!({"declared_type":"ENUMProgrammingLanguage", "encoded":"000001f0", "pointer_depth":0, "descriptor_chain":[496], "target_type_info_index":23}),
            )
        } else {
            assert_eq!(name, "strBaseDirectory");
            (
                8056,
                json!({"declared_type":"BSTR", "encoded":"80080008", "pointer_depth":0, "descriptor_chain":[], "target_type_info_index":null}),
            )
        };
        params.push(json!({"index":0, "name":name, "name_offset":name_offset,
            "record_offset":m.offset+32, "parameter_flags":1, "direction":["in"], "type":ty}));
    }
    let (name, name_offset) = if m.record < 32 {
        ("pVal", 64)
    } else {
        ("pResult", 8084)
    };
    params.push(json!({"index":params.len(), "name":name, "name_offset":name_offset,
        "record_offset":m.offset+32+12*params.len() as u64, "parameter_flags":10,
        "direction":["out", "retval"], "type":{
            "declared_type":format!("{}**", m.result), "encoded":format!("{:08x}", m.result_chain[0]),
            "pointer_depth":2, "descriptor_chain":m.result_chain, "target_type_info_index":m.result_index
        }}));
    params
}

fn expected_catalog(m: &Method, catalog: &Value) -> Value {
    let id = format!("provisional.com.Document.{}", m.name);
    let page = catalog["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap();
    let filename = format!(
        "mfapi_interfaces_document_methods_{}.html",
        m.name.to_lowercase()
    );
    let source = json!({"official_url":format!("https://www.altova.com/manual/en/api/mapforceapi/{filename}"),
        "raw_sha256":m.page_sha, "release_binding":"unestablished", "source_file":filename});
    assert_eq!(page["source"], source);
    let result = m.result.strip_prefix('I').unwrap();
    let params = match m.input {
        Some("i_nLanguage") => {
            json!([{"name":"i_nLanguage", "declared_type":"ENUMProgrammingLanguage", "direction":"in"}])
        }
        Some("strBaseDirectory") => {
            json!([{"name":"strBaseDirectory", "declared_type":"String", "direction":"in"}])
        }
        None => json!([]),
        other => panic!("unexpected input {other:?}"),
    };
    let arguments = params
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            format!(
                "in {} as {}",
                p["name"].as_str().unwrap(),
                p["declared_type"].as_str().unwrap()
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let signature = format!("{}({arguments}) -> {result}", m.name);
    assert_eq!(page["contract"]["declared_signature_or_token"], signature);
    assert_eq!(page["contract"]["declared_return_or_property_type"], result);
    assert_eq!(page["contract"]["arity"], params.as_array().unwrap().len());
    for (documented, expected) in page["contract"]["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .zip(params.as_array().unwrap())
    {
        for field in ["name", "declared_type", "direction"] {
            assert_eq!(documented[field], expected[field]);
        }
    }
    let (constraints, errors) = match m.record {
        25 => (json!([]), json!(["1200", "1201", "1205"])),
        30 => (
            json!([
                "Output generation requires a visible MapForce UI (the document methods also permit GUI embedding)."
            ]),
            json!(["1200", "1201", "1206", "1210"]),
        ),
        32 => (
            json!([
                "strBaseDirectory specifies the base against which output-file paths are resolved. No optional/default argument is declared."
            ]),
            json!([]),
        ),
        33 => (json!([]), json!([])),
        other => panic!("unexpected record {other}"),
    };
    assert_eq!(page["constraints"], constraints);
    assert_eq!(page["documented_error_codes"], errors);
    json!({"id":id, "source":source, "displayed_signature":signature, "displayed_return_type":result,
        "parameters":params, "constraints":constraints, "documented_error_codes":errors})
}

fn expected_descriptors() -> Vec<Value> {
    let mut result = vec![
        json!({"descriptor_offset":496, "descriptor_word":"0003001d",
        "target_word":"000008fc", "vartype":"USERDEFINED", "type_info_index":23}),
    ];
    for m in &METHODS {
        let [outer, inner, target] = m.result_chain;
        result.push(json!({"descriptor_offset":target, "descriptor_word":"7fff001d",
            "target_word":format!("{:08x}", m.result_index*100), "vartype":"USERDEFINED", "type_info_index":m.result_index}));
        for (offset, next) in [(inner, target), (outer, inner)] {
            result.push(json!({"descriptor_offset":offset, "descriptor_word":"7fff001a",
                "target_word":format!("{next:08x}"), "vartype":"PTR", "target_descriptor_offset":next}));
        }
    }
    result
}

fn check_inventory(inventory: &Value) {
    keys(
        inventory,
        &[
            "format",
            "format_version",
            "inventory_complete",
            "runtime_proof",
            "status",
            "applicability",
            "execution_evidence",
            "purpose",
            "type_library_source",
            "catalog_source",
            "readback",
            "targets",
            "descriptors",
            "methods",
            "comparison_boundary",
            "counts",
            "remaining",
        ],
    );
    assert_eq!(
        inventory["format"],
        "mapforce-enterprise-2026r2-com-generation-signatures"
    );
    assert_eq!(inventory["format_version"], 1);
    assert_eq!(inventory["inventory_complete"], false);
    assert_eq!(inventory["runtime_proof"], false);
    assert_eq!(inventory["status"], "unassessed");
    assert_eq!(inventory["applicability"], "unknown");
    assert_eq!(inventory["execution_evidence"], json!([]));
    assert_eq!(
        inventory["purpose"],
        "Four IDocument generation-method declarations only; not ABI or execution qualification."
    );
    source_only(inventory);
    let artifacts = load("artifact-observations.json");
    let alignment = load("com-name-alignment.json");
    let catalog = load("com-api.json");
    assert_eq!(
        inventory["type_library_source"],
        json!({"observation":"artifact-observations.json",
        "member":"MapForce.tlb", "cab_member":"MapForce.tlb_1193", "bytes":54108,
        "sha256":LIBRARY_SHA, "library":"MapForceLib", "version":"4.1", "platform":"SYS_WIN64",
        "dump_sha256":DUMP_SHA, "release_binding":"2026r2"})
    );
    let artifact = artifacts["matched_members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "MapForce.tlb")
        .unwrap();
    for field in ["sha256", "bytes", "cab_member"] {
        assert_eq!(inventory["type_library_source"][field], artifact[field]);
    }
    for field in ["sha256", "dump_sha256", "library", "version", "platform"] {
        assert_eq!(
            inventory["type_library_source"][field],
            alignment["type_library_source"][field]
        );
    }
    assert_eq!(artifacts["release"]["release"], "2026r2");
    assert_eq!(
        inventory["catalog_source"],
        json!({"file":"com-api.json", "sha256":CATALOG_SHA, "release_binding":"unestablished"})
    );
    assert_eq!(inventory["catalog_source"], alignment["catalog_source"]);
    assert_eq!(catalog["release_binding"], "unestablished");
    let mut readback = inventory["readback"].clone();
    let notes = readback.as_object_mut().unwrap().remove("method").unwrap();
    assert_eq!(notes.as_array().unwrap().len(), 5);
    assert!(
        notes
            .as_array()
            .unwrap()
            .iter()
            .all(|n| !n.as_str().unwrap().is_empty())
    );
    assert_eq!(
        readback,
        json!({"source":"type_library_source", "type_info_table_offset":624,
        "type_info_record_bytes":100, "type_info_records":75, "name_table_offset":10804,
        "name_table_bytes":13724, "descriptor_table_offset":30276, "descriptor_table_bytes":824,
        "descriptor_record_bytes":8, "owner":{"name":"IDocument", "type_info_index":7,
            "kind":"TKIND_DISPATCH", "name_offset":1776}, "member_block_offset":36016,
        "member_data_bytes":1696, "owner_function_records":39, "member_id_table_offset":37716,
        "member_name_table_offset":37872, "member_record_table_offset":38028})
    );
    assert_eq!(
        inventory["targets"],
        json!([
            {"name":"ENUMProgrammingLanguage", "type_info_index":23, "kind":"TKIND_ENUM", "name_offset":6592},
            {"name":"IErrorMarkers", "type_info_index":24, "kind":"TKIND_DISPATCH", "name_offset":6796},
            {"name":"IAppOutputLines", "type_info_index":14, "kind":"TKIND_DISPATCH", "name_offset":3428},
            {"name":"IOutputGenerationResult", "type_info_index":28, "kind":"TKIND_DISPATCH", "name_offset":7548},
            {"name":"ICodeGenerationResult", "type_info_index":30, "kind":"TKIND_DISPATCH", "name_offset":8104}
        ])
    );
    assert_eq!(inventory["descriptors"], json!(expected_descriptors()));
    let methods = inventory["methods"].as_array().unwrap();
    assert_eq!(methods.len(), METHODS.len());
    unique_ids(methods, "name");
    for (row, m) in methods.iter().zip(&METHODS) {
        let params = expected_parameters(m);
        assert_eq!(
            row,
            &json!({"name":m.name, "record_index":m.record, "dispatch_member_id":m.dispid,
            "name_offset":m.name_offset, "record_offset":m.offset, "record_bytes":32+12*params.len(),
            "invocation_kind":"function", "invocation_kind_value":1, "function_flags":0,
            "optional_argument_count":0, "return_type":{"vartype":"HRESULT", "encoded":"80190019"},
            "parameters":params, "source_anchor":{"function_dump_lines":m.dump_lines,
                "member_id_dump_line":9145+m.record, "member_name_dump_line":9184+m.record,
                "member_offset_dump_line":9223+m.record}, "catalog":expected_catalog(m, &catalog)})
        );
    }
    assert_eq!(
        inventory["comparison_boundary"],
        json!({"return_notation":BOUNDARIES[0],
        "input_notation":BOUNDARIES[1], "constraint_scope":BOUNDARIES[2], "optionality":BOUNDARIES[3]})
    );
    assert_eq!(
        inventory["counts"],
        json!({"methods":4, "parameters":7, "input_parameters":3, "out_retval_parameters":4})
    );
    assert_eq!(inventory["remaining"], json!(REMAINING));
}

fn rejects(inventory: &Value, label: &str) {
    assert!(
        std::panic::catch_unwind(|| check_inventory(inventory)).is_err(),
        "accepted mutation: {label}"
    );
}

#[test]
fn generation_signatures_match_independent_raw_oracle_and_online_boundaries() {
    check_inventory(&load("com-generation-signatures.json"));
    let canonical: Value =
        serde_json::from_str(include_str!("../../../../conformance/mfd-2026r2.json")).unwrap();
    assert_eq!(canonical["inventory_complete"], false);
}

#[test]
fn generation_signatures_reject_missing_duplicate_and_changed_member_records() {
    let original = load("com-generation-signatures.json");
    for i in 0..METHODS.len() {
        let mut missing = original.clone();
        missing["methods"].as_array_mut().unwrap().remove(i);
        missing["counts"]["methods"] = json!(3);
        rejects(&missing, "missing method with adjusted count");
        let mut duplicate = original.clone();
        duplicate["methods"][(i + 1) % METHODS.len()] = original["methods"][i].clone();
        rejects(&duplicate, "same-count duplicate method");
        for (field, wrong) in [
            ("name", json!("GenerateOutput")),
            ("record_index", json!(0)),
            ("dispatch_member_id", json!(99)),
            ("name_offset", json!(0)),
            ("record_offset", json!(0)),
            ("record_bytes", json!(0)),
            ("invocation_kind", json!("property_get")),
            ("invocation_kind_value", json!(2)),
            ("function_flags", json!(64)),
            ("optional_argument_count", json!(1)),
            (
                "return_type",
                json!({"vartype":"BSTR", "encoded":"80080008"}),
            ),
        ] {
            let mut changed = original.clone();
            changed["methods"][i][field] = wrong;
            rejects(&changed, field);
        }
    }
}

#[test]
fn generation_signatures_reject_parameter_order_type_depth_targets_and_flags() {
    let original = load("com-generation-signatures.json");
    for (i, m) in METHODS.iter().enumerate() {
        let count = expected_parameters(m).len();
        if count == 2 {
            let mut reordered = original.clone();
            reordered["methods"][i]["parameters"]
                .as_array_mut()
                .unwrap()
                .swap(0, 1);
            rejects(&reordered, "parameter order");
        }
        for j in 0..count {
            let mut missing = original.clone();
            missing["methods"][i]["parameters"]
                .as_array_mut()
                .unwrap()
                .remove(j);
            missing["counts"]["parameters"] = json!(6);
            rejects(&missing, "missing parameter with adjusted count");
            let mut duplicate = original.clone();
            let param = duplicate["methods"][i]["parameters"][j].clone();
            duplicate["methods"][i]["parameters"]
                .as_array_mut()
                .unwrap()
                .push(param);
            rejects(&duplicate, "duplicate parameter");
            for (field, wrong) in [
                ("index", json!(9)),
                ("name", json!("wrong")),
                ("name_offset", json!(0)),
                ("record_offset", json!(0)),
                ("parameter_flags", json!(3)),
                ("direction", json!(["in", "out"])),
            ] {
                let mut changed = original.clone();
                changed["methods"][i]["parameters"][j][field] = wrong;
                rejects(&changed, field);
            }
            for (field, wrong) in [
                ("declared_type", json!("VARIANT*")),
                ("encoded", json!("800c000c")),
                ("pointer_depth", json!(1)),
                ("descriptor_chain", json!([512, 504])),
                ("target_type_info_index", json!(22)),
            ] {
                let mut changed = original.clone();
                changed["methods"][i]["parameters"][j]["type"][field] = wrong;
                rejects(&changed, field);
            }
        }
    }
    // Coherent edits across all redundant type fields must not pass merely
    // because a descriptor walker or record count agrees with the fragment.
    let mut changed = original.clone();
    changed["methods"][0]["parameters"][1]["type"] =
        original["methods"][1]["parameters"][0]["type"].clone();
    rejects(&changed, "coherent wrong interface target");
    let mut changed = original.clone();
    changed["targets"][0]["name"] = json!("ENUMConnectionType");
    for i in [0, 3] {
        changed["methods"][i]["parameters"][0]["type"]["declared_type"] =
            json!("ENUMConnectionType");
    }
    rejects(&changed, "coherent wrong enum target");
    for i in 0..original["descriptors"].as_array().unwrap().len() {
        for (field, value) in [
            ("descriptor_offset", json!(0)),
            ("descriptor_word", json!("00000000")),
            ("target_word", json!("00000000")),
            ("vartype", json!("BSTR")),
        ] {
            let mut changed = original.clone();
            changed["descriptors"][i][field] = value;
            rejects(&changed, field);
        }
    }
}

#[test]
fn generation_signatures_reject_bad_provenance_and_qualification_promotions() {
    let original = load("com-generation-signatures.json");
    for (pointer, wrong) in [
        ("/inventory_complete", json!(true)),
        ("/runtime_proof", json!(true)),
        ("/status", json!("verified")),
        ("/applicability", json!("applicable")),
        (
            "/execution_evidence",
            json!(["declarations are not execution"]),
        ),
        ("/type_library_source/sha256", json!("0".repeat(64))),
        ("/type_library_source/dump_sha256", json!("0".repeat(64))),
        ("/catalog_source/sha256", json!("0".repeat(64))),
        ("/catalog_source/release_binding", json!("2026r2")),
        ("/readback/owner/type_info_index", json!(11)),
        ("/readback/member_record_table_offset", json!(0)),
        (
            "/comparison_boundary/return_notation",
            json!("The raw and online returns contradict each other."),
        ),
        (
            "/comparison_boundary/constraint_scope",
            json!("GenerateOutputEx2 requires a visible window."),
        ),
        ("/remaining", json!([])),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).unwrap() = wrong;
        rejects(&changed, pointer);
    }
    for field in [
        "abi_verified",
        "calling_convention",
        "runtime_default",
        "ownership",
        "backend_support",
    ] {
        let mut changed = original.clone();
        changed[field] = json!(true);
        rejects(&changed, field);
        let mut changed = original.clone();
        changed["methods"][0][field] = json!(true);
        rejects(&changed, field);
    }
    for i in 0..METHODS.len() {
        for (pointer, value) in [
            ("source/raw_sha256", json!("0".repeat(64))),
            ("source/release_binding", json!("2026r2")),
            ("displayed_return_type", json!("HRESULT")),
            ("constraints", json!([])),
        ] {
            if pointer == "constraints"
                && original["methods"][i]["catalog"]["constraints"] == json!([])
            {
                continue;
            }
            let mut changed = original.clone();
            *changed["methods"][i]["catalog"]
                .pointer_mut(&format!("/{pointer}"))
                .unwrap() = value;
            rejects(&changed, pointer);
        }
        let mut changed = original.clone();
        changed["methods"][i]["source_anchor"]["function_dump_lines"] = json!([1, 2]);
        rejects(&changed, "wrong dump anchor");
    }
    let mut changed = original.clone();
    changed["methods"][2]["catalog"]["constraints"] =
        original["methods"][1]["catalog"]["constraints"].clone();
    rejects(&changed, "transferred visible-window constraint");
}
