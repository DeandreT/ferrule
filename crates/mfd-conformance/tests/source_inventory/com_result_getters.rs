//! Independent frozen declarations: no vendor files, network or activation.

use serde_json::{Value, json};

use super::{load, source_only};

// Transcribed separately from the hash-pinned raw records and dump. Online
// displayed property types are not used to infer raw pointers or targets.
const NAMES: [&str; 7] = [
    "Application",
    "Parent",
    "Succeeded",
    "Failed",
    "ResultCode",
    "Messages",
    "MessageText",
];
const NAME_OFFSETS: [u64; 7] = [80, 104, 7584, 7608, 7956, 7980, 8000];
const OFFSETS: [[u64; 7]; 2] = [
    [46344, 46388, 46432, 46476, 46520, 46564, 46608],
    [46904, 46948, 46992, 47036, 47080, 47124, 47168],
];
const OWNERS: [&str; 2] = ["IOutputGenerationResult", "ICodeGenerationResult"];
const ENUMS: [&str; 2] = [
    "ENUMOutputGenerationResultCode",
    "ENUMCodeGenerationResultCode",
];
const PAGE_SHA: [[&str; 7]; 2] = [
    [
        "29423ad51f0e0572a28c9e515270b810bc6ce0900148735c274ec816b4f43f47",
        "806bc9afc199189255a093609fb02ca0985d818a81ee1ae746a5cbc5c243583a",
        "780b67b6cf4aefa2aea7b01233669c96014c3e8daffa2efe67e2aaf72f6b0cdd",
        "ccc50686a653ed5c34cb84efd4c4eb301bafb9a7b593414d641850af89c21981",
        "880b38bdb3fdbf31af5fa4d31686d8a490c6b1d6afd9ad929df6b2a72e3d2f73",
        "1280ff6508c66288983bf182d6ed565770be449ae91bfc0c24bae8ae76ed006f",
        "e95c68c24b072eb630a5adbc523f49c6a40f2da21fc405741025a32a3e06d4c5",
    ],
    [
        "7e77a504647082a9bd08c8cb6ef1d59dbb792dce820cf3ecd3946c7672504e2d",
        "7d34b2cd45c73059d3ab6ced50e365c2e4895cb046329c201b90c4d1da0c4946",
        "ce1bfb102ece31652f0027b291536678dfadae823f5a808247a93ca315cd6023",
        "ae93ce3df294dba2a0f836644d564a402db64427c96f3d30e32f527458e579de",
        "0bb08da0cdfaef0708f3e4873aae8d8931682877815ea60de246e29c0ead0a18",
        "68fdd5a5fdbd59edb77f7fad0a4a48222d7f47c431b24e48647fbf2706152358",
        "5fe444d461240e65e0ed0d859f5baffb2849ee22f07d67d3dbd52485e52cb538",
    ],
];

fn expected_type(owner: usize, member: usize) -> Value {
    let (name, chain, target) = match member {
        0 | 1 => ("IDispatch*".to_owned(), vec![8], None),
        2 | 3 => ("BOOL*".to_owned(), vec![16], None),
        4 => (
            format!("{}*", ENUMS[owner]),
            if owner == 0 {
                vec![560, 552]
            } else {
                vec![600, 592]
            },
            Some(if owner == 0 { 29 } else { 31 }),
        ),
        5 => (
            "IAppOutputLines**".to_owned(),
            vec![192, 184, 176],
            Some(14),
        ),
        6 => ("BSTR*".to_owned(), vec![0], None),
        _ => unreachable!(),
    };
    json!({"declared_type":name,"encoded":format!("{:08x}", chain[0]),
        "pointer_depth":if member == 5 {2} else {1},"descriptor_chain":chain,"target_type_info_index":target})
}

fn expected_catalog(owner: usize, member: usize, catalog: &Value) -> Value {
    let short = &OWNERS[owner][1..];
    let name = NAMES[member];
    let id = format!("provisional.com.{short}.{name}");
    let filename = format!(
        "mfapi_interfaces_{}_properties_{}.html",
        short.to_lowercase(),
        name.to_lowercase()
    );
    let ty = match member {
        0 | 1 => "Application",
        2 | 3 => "Boolean",
        4 => ENUMS[owner],
        5 => "AppOutputLines",
        6 => "String",
        _ => unreachable!(),
    };
    let signature = format!("{name} as {ty}");
    let source = json!({"official_url":format!("https://www.altova.com/manual/en/api/mapforceapi/{filename}"),
        "source_file":filename,"raw_sha256":PAGE_SHA[owner][member],"release_binding":"unestablished"});
    let page = catalog["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id)
        .unwrap();
    assert_eq!(page["source"], source);
    assert_eq!(
        page["contract"],
        json!({"arity":0,"declared_return_or_property_type":ty,
        "declared_signature_or_token":signature,"parameters":[],"property_access":"not_explicitly_read_only_in_summary",
        "return_contract_state":"displayed_type","runtime_proof":false,"signature_state":"typed_displayed_declaration"})
    );
    assert_eq!(page["constraints"], json!([]));
    assert_eq!(page["documented_error_codes"], json!([]));
    json!({"id":id,"source":source,"displayed_signature":signature,"displayed_property_type":ty,
        "property_access":"not_explicitly_read_only_in_summary","parameters":[],"constraints":[],"documented_error_codes":[]})
}

fn expected_getter(owner: usize, member: usize, catalog: &Value) -> Value {
    let offset = OFFSETS[owner][member];
    let first = [12381, 12589][owner];
    json!({"owner":OWNERS[owner],"name":NAMES[member],"record_index":member,"dispatch_member_id":member+1,
        "name_offset":NAME_OFFSETS[member],"record_offset":offset,"record_bytes":44,"invocation_kind":"property_get",
        "invocation_kind_value":2,"function_flags":0,"optional_argument_count":0,
        "return_type":{"vartype":"HRESULT","encoded":"80190019"},
        "parameters":[{"index":0,"name":"pVal","name_offset":64,"record_offset":offset+32,
            "parameter_flags":10,"direction":["out","retval"],"type":expected_type(owner,member)}],
        "source_anchor":{"function_dump_lines":[first+18*member,first+18*member+17],
            "member_id_dump_line":first+126+member,"member_name_dump_line":first+133+member,"member_offset_dump_line":first+140+member},
        "catalog":expected_catalog(owner,member,catalog)})
}

fn expected_descriptors() -> Value {
    json!([
        {"descriptor_offset":0,"descriptor_word":"4008001a","target_word":"80080008","vartype":"PTR","target_encoded":"80080008"},
        {"descriptor_offset":8,"descriptor_word":"4009001a","target_word":"80090009","vartype":"PTR","target_encoded":"80090009"},
        {"descriptor_offset":16,"descriptor_word":"400b001a","target_word":"800b000b","vartype":"PTR","target_encoded":"800b000b"},
        {"descriptor_offset":176,"descriptor_word":"7fff001d","target_word":"00000578","vartype":"USERDEFINED","type_info_index":14},
        {"descriptor_offset":184,"descriptor_word":"7fff001a","target_word":"000000b0","vartype":"PTR","target_descriptor_offset":176},
        {"descriptor_offset":192,"descriptor_word":"7fff001a","target_word":"000000b8","vartype":"PTR","target_descriptor_offset":184},
        {"descriptor_offset":552,"descriptor_word":"0003001d","target_word":"00000b54","vartype":"USERDEFINED","type_info_index":29},
        {"descriptor_offset":560,"descriptor_word":"4003001a","target_word":"00000228","vartype":"PTR","target_descriptor_offset":552},
        {"descriptor_offset":592,"descriptor_word":"0003001d","target_word":"00000c1c","vartype":"USERDEFINED","type_info_index":31},
        {"descriptor_offset":600,"descriptor_word":"4003001a","target_word":"00000250","vartype":"PTR","target_descriptor_offset":592}
    ])
}

fn expected_boundary() -> Value {
    json!({
        "return_notation":"Online property types are higher-level declarations; raw getters return HRESULT with one out/retval parameter. This is not a contradiction or ABI equivalence claim.",
        "application_parent":"Online Application and Parent are displayed as Application. Raw declarations retain generic PTR(DISPATCH), not a concrete IApplication target; neither representation replaces the other.",
        "access":"Only property_get records occur in this bounded raw cohort. The online not_explicitly_read_only_in_summary access labels remain unchanged; getter-only records do not rewrite those labels or establish runtime mutability.",
        "messages":"Online Messages describes a message tree and MessageText describes line-break-separated text. These release-unbound descriptions do not establish runtime ordering, flattening, ownership or lifetime.",
        "success":"Succeeded and Failed declarations do not establish complementarity or any relationship to HRESULT or ResultCode at runtime.",
        "types":"BOOL/Boolean, BSTR/String and named enum/interface comparisons are declaration-level only, not coercion or marshaling proof. Enum constants are already covered by com-enum-values.json and are not a new cohort."
    })
}

fn check(inventory: &Value) {
    source_only(inventory);
    let catalog = load("com-api.json");
    let mut expected = json!({
        "format":"mapforce-enterprise-2026r2-com-result-getters","format_version":1,
        "inventory_complete":false,"runtime_proof":false,"status":"unassessed","applicability":"unknown","execution_evidence":[],
        "purpose":"Fourteen property-get declarations on two generation-result interfaces only; not ABI or execution qualification.",
        "type_library_source":{"observation":"artifact-observations.json","member":"MapForce.tlb","cab_member":"MapForce.tlb_1193","bytes":54108,
            "sha256":"785c484fbaa882cae724a3568b04650300eb18941084ac23eb2d041dfb4a2789","library":"MapForceLib","version":"4.1","platform":"SYS_WIN64",
            "dump_sha256":"672097183d6a6b0d80f3aa5f92d02172bb3a588609cb743bb82b73c342192bb4","release_binding":"2026r2"},
        "catalog_source":{"file":"com-api.json","sha256":"d9a2fd8a4f8935598fdd2590f837fd6205931ba43c6218dc6b32a29fbe35c16a","release_binding":"unestablished"},
        "readback":{"source":"type_library_source","type_info_table_offset":624,"type_info_record_bytes":100,"type_info_records":75,
            "name_table_offset":10804,"name_table_bytes":13724,"descriptor_table_offset":30276,"descriptor_table_bytes":824,"descriptor_record_bytes":8,
            "method":[
                "Verify the official-release selected member and retained acquisition/comparison records by size and SHA-256 before bounded reads. HTTPS acquisition and byte identity are the asserted provenance; Authenticode integrity, timestamp and certificate-chain trust remain unverified.",
                "Resolve both owner identities and referenced enum/interface identities from raw type-info and name records. Read member IDs, names and offsets from each owner's parallel tables; compare all fourteen function records and ordered parameter triples with the pinned dump.",
                "Follow bounded descriptor chains, including the USERDEFINED target hidden by the dump's PTR -> PTR rendering for Messages. Absolute record/table offsets and segment-relative name/descriptor offsets remain distinct.",
                "Compare all fourteen hash-pinned HTML snapshots with com-api.json without fetching mutable pages. One-based dump anchors refer to the hash-pinned dump. Retain online types and access labels separately from raw getter records.",
                "Only declaration metadata is retained. No application, installer, native library or COM object is loaded or executed."
            ],
            "owners":[
                {"name":"IOutputGenerationResult","type_info_index":28,"kind":"TKIND_DISPATCH","name_offset":7548,"member_block_offset":46340,"member_data_bytes":308,"owner_function_records":7,"member_id_table_offset":46652,"member_name_table_offset":46680,"member_record_table_offset":46708},
                {"name":"ICodeGenerationResult","type_info_index":30,"kind":"TKIND_DISPATCH","name_offset":8104,"member_block_offset":46900,"member_data_bytes":308,"owner_function_records":7,"member_id_table_offset":47212,"member_name_table_offset":47240,"member_record_table_offset":47268}
            ]},
        "targets":[
            {"name":"IAppOutputLines","type_info_index":14,"kind":"TKIND_DISPATCH","name_offset":3428},
            {"name":"ENUMOutputGenerationResultCode","type_info_index":29,"kind":"TKIND_ENUM","name_offset":7628},
            {"name":"ENUMCodeGenerationResultCode","type_info_index":31,"kind":"TKIND_ENUM","name_offset":8140}
        ],
        "descriptors":expected_descriptors(),"getters":[],"comparison_boundary":expected_boundary(),
        "counts":{"interfaces":2,"property_gets":14,"parameters":14,"out_retval_parameters":14},
        "remaining":["Other COM method signatures remain outside this bounded fragment; inventory_complete remains false.",
            "The pinned online catalog's release binding remains unestablished.",
            "No ABI, calling convention, marshaling, ownership, lifetime, runtime default, error behavior or backend support is established.",
            "No activation or execution evidence is supplied; canonical capability assessments are unchanged."]
    });
    expected["getters"] = json!(
        (0..2)
            .flat_map(|o| (0..7).map(move |m| (o, m)))
            .map(|(o, m)| expected_getter(o, m, &catalog))
            .collect::<Vec<_>>()
    );
    assert_eq!(inventory, &expected);
    let artifacts = load("artifact-observations.json");
    let member = artifacts["matched_members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "MapForce.tlb")
        .unwrap();
    for key in ["sha256", "bytes", "cab_member"] {
        assert_eq!(inventory["type_library_source"][key], member[key]);
    }
    let alignment = load("com-name-alignment.json");
    for key in ["sha256", "dump_sha256", "library", "version", "platform"] {
        assert_eq!(
            inventory["type_library_source"][key],
            alignment["type_library_source"][key]
        );
    }
    assert_eq!(inventory["catalog_source"], alignment["catalog_source"]);
    assert_eq!(artifacts["release"]["release"], "2026r2");
    assert_eq!(catalog["release_binding"], "unestablished");
}

fn rejects(inventory: &Value, label: &str) {
    assert!(
        std::panic::catch_unwind(|| check(inventory)).is_err(),
        "accepted mutation: {label}"
    );
}

#[test]
fn result_getters_match_independent_declarations_and_online_boundaries() {
    check(&load("com-result-getters.json"));
    let canonical: Value =
        serde_json::from_str(include_str!("../../../../conformance/mfd-2026r2.json")).unwrap();
    assert_eq!(canonical["inventory_complete"], false);
}

#[test]
fn result_getters_reject_missing_duplicates_order_and_every_record_field() {
    let original = load("com-result-getters.json");
    for i in 0..14 {
        let mut missing = original.clone();
        missing["getters"].as_array_mut().unwrap().remove(i);
        missing["counts"]["property_gets"] = json!(13);
        missing["counts"]["parameters"] = json!(13);
        rejects(&missing, "missing with adjusted counts");
        let mut duplicate = original.clone();
        duplicate["getters"][(i + 1) % 14] = original["getters"][i].clone();
        rejects(&duplicate, "same-count duplicate");
        let mut swapped = original.clone();
        swapped["getters"]
            .as_array_mut()
            .unwrap()
            .swap(i, (i + 1) % 14);
        rejects(&swapped, "order");
        // Walk every leaf, not only a selection of convenient fields. This also
        // checks each source anchor, online hash, parameter and descriptor layer.
        mutate_leaves(&original, &original["getters"][i], &format!("/getters/{i}"));
        for field in [
            "abi_verified",
            "runtime_default",
            "ownership",
            "coercion",
            "marshaling",
            "error_behavior",
        ] {
            let mut changed = original.clone();
            changed["getters"][i][field] = json!(true);
            rejects(&changed, field);
        }
        let mut missing_parameter = original.clone();
        missing_parameter["getters"][i]["parameters"] = json!([]);
        missing_parameter["counts"]["parameters"] = json!(13);
        rejects(&missing_parameter, "missing parameter");
        let mut duplicate_parameter = original.clone();
        duplicate_parameter["getters"][i]["parameters"]
            .as_array_mut()
            .unwrap()
            .push(original["getters"][i]["parameters"][0].clone());
        rejects(&duplicate_parameter, "duplicate parameter");
    }
}

fn mutate_leaves(original: &Value, node: &Value, pointer: &str) {
    match node {
        Value::Object(fields) => {
            for (key, value) in fields {
                mutate_leaves(original, value, &format!("{pointer}/{key}"));
            }
        }
        Value::Array(values) if !values.is_empty() => {
            for (i, value) in values.iter().enumerate() {
                mutate_leaves(original, value, &format!("{pointer}/{i}"));
            }
        }
        _ => {
            let wrong = match node {
                Value::Bool(b) => json!(!b),
                Value::Number(n) => json!(n.as_u64().unwrap() + 1),
                Value::String(_) => json!("wrong"),
                Value::Null => json!(0),
                Value::Array(_) => json!(["unsupported"]),
                _ => unreachable!(),
            };
            let mut changed = original.clone();
            *changed.pointer_mut(pointer).unwrap() = wrong;
            rejects(&changed, pointer);
        }
    }
}

#[test]
fn result_getters_reject_coherent_owner_enum_and_pointer_substitutions() {
    let original = load("com-result-getters.json");
    let mut swapped = original.clone();
    for i in 0..7 {
        swapped["getters"][i]["owner"] = json!(OWNERS[1]);
        swapped["getters"][i + 7]["owner"] = json!(OWNERS[0]);
    }
    swapped["readback"]["owners"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    rejects(&swapped, "coherent owner swap");
    let mut swapped = original.clone();
    for (index, other) in [(4, 11), (11, 4)] {
        swapped["getters"][index]["parameters"][0]["type"] =
            original["getters"][other]["parameters"][0]["type"].clone();
        swapped["getters"][index]["catalog"]["displayed_property_type"] =
            original["getters"][other]["catalog"]["displayed_property_type"].clone();
        swapped["getters"][index]["catalog"]["displayed_signature"] =
            original["getters"][other]["catalog"]["displayed_signature"].clone();
    }
    swapped["targets"].as_array_mut().unwrap().swap(1, 2);
    rejects(&swapped, "coherent cross-owner enum swap");
    for i in [0, 1, 7, 8] {
        let mut concrete = original.clone();
        concrete["getters"][i]["parameters"][0]["type"] = json!({"declared_type":"IApplication**","encoded":"00000008","pointer_depth":2,"descriptor_chain":[8],"target_type_info_index":0});
        rejects(&concrete, "online concrete type replacing generic dispatch");
    }
    for i in [5, 12] {
        let mut wrong = original.clone();
        wrong["getters"][i]["parameters"][0]["type"] = json!({"declared_type":"IAppOutputLines*","encoded":"000000b8","pointer_depth":1,"descriptor_chain":[184,176],"target_type_info_index":14});
        rejects(&wrong, "coherent lost pointer layer");
    }
}

#[test]
fn result_getters_reject_provenance_and_qualification_promotions() {
    let original = load("com-result-getters.json");
    for (key, value) in original.as_object().unwrap() {
        if key != "getters" && key != "readback" {
            mutate_leaves(&original, value, &format!("/{key}"));
        }
    }
    for (key, value) in original["readback"].as_object().unwrap() {
        mutate_leaves(&original, value, &format!("/readback/{key}"));
    }
    for field in [
        "abi_verified",
        "calling_convention",
        "backend_support",
        "runtime_default",
        "ownership",
        "lifetime",
        "coercion",
        "marshaling",
        "error_behavior",
    ] {
        let mut changed = original.clone();
        changed[field] = json!(true);
        rejects(&changed, field);
    }
    for (key, claim) in [
        (
            "application_parent",
            "Raw pointers establish concrete Application objects.",
        ),
        (
            "access",
            "Getter-only records prove runtime read-only properties.",
        ),
        (
            "messages",
            "MessageText preserves Messages traversal order and lifetime.",
        ),
        (
            "success",
            "Succeeded and Failed are complementary and derive from HRESULT.",
        ),
        (
            "types",
            "BSTR and String establish marshaling and coercion equivalence.",
        ),
    ] {
        let mut changed = original.clone();
        changed["comparison_boundary"][key] = json!(claim);
        rejects(&changed, key);
    }
}
