//! Frozen declaration oracle: no vendor files, network, loading or activation.

use std::sync::OnceLock;

use serde_json::{Value, json};

use super::{load, source_only};

#[derive(Clone, Copy)]
enum Getter {
    Application,
    Parent,
    Count,
    Item,
    NewEnum,
}

use Getter::{Application, Count, Item, NewEnum, Parent};

// Transcribed independently from the pinned bytes/dump, not from the proposed
// fragment or its readback helper. Owner order is deliberately not normalized.
const OWNERS: [&str; 2] = ["IAppOutputLines", "IErrorMarkers"];
const ORDER: [[Getter; 5]; 2] = [
    [Application, Parent, Count, Item, NewEnum],
    [Count, Item, NewEnum, Application, Parent],
];
const OFFSETS: [[u64; 5]; 2] = [
    [42512, 42556, 42600, 42636, 42684],
    [45284, 45320, 45368, 45404, 45448],
];
const LINES: [[u64; 5]; 2] = [
    [10926, 10944, 10962, 10978, 10999],
    [11975, 11991, 12012, 12028, 12046],
];
// Online page hashes are in Application, Parent, Count, Item order, separate
// from raw declaration order and types. There is no online _NewEnum record.
const PAGE_SHA: [[&str; 4]; 2] = [
    [
        "67f55171c8c56e6f144f787bac24a5f8f47b47ba52e9d8e07d8c3e23681167bb",
        "73a5ed101cce6771c835caa6cdced96e22f34189a4ef3c4b8ff80a6e360ba1e8",
        "9f656aa4ebf1e001f9154a21026682d1a1fba62154c9b3ff630a5f78f97238f4",
        "939bef1a126415b295eee5beecdfb4e2659c572160fd260a5acbaab73c18f189",
    ],
    [
        "b79f8615af4b278867ce9cecddf2dcb4760985d139b5643c07a948ea44db292d",
        "8673ddccc4adfe5c0850decd92084694deb21aae7273d8b2708762a894a9e778",
        "1fc092cf12ab67082bcc34d814186f47129a92f90b3ab5241ba8d726fc8a2a8b",
        "125a13371ce9a178433392b8f7dfd90671618c73b7bdd03da8f39944dea237f0",
    ],
];

impl Getter {
    fn name(self) -> &'static str {
        match self {
            Application => "Application",
            Parent => "Parent",
            Count => "Count",
            Item => "Item",
            NewEnum => "_NewEnum",
        }
    }
}

fn source(filename: &str, sha: &str) -> Value {
    json!({"official_url":format!("https://www.altova.com/manual/en/api/mapforceapi/{filename}"),
        "source_file":filename,"raw_sha256":sha,"release_binding":"unestablished"})
}

fn catalog(owner: usize, kind: Getter) -> Value {
    let (slot, ty) = match kind {
        Application => (0, "Application"),
        Parent => (
            1,
            if owner == 0 {
                "AppOutputLine"
            } else {
                "Application"
            },
        ),
        Count => (2, "Integer"),
        Item => (
            3,
            if owner == 0 {
                "AppOutputLine"
            } else {
                "ErrorMarker"
            },
        ),
        NewEnum => return Value::Null,
    };
    let short = &OWNERS[owner][1..];
    let name = kind.name();
    let filename = format!(
        "mfapi_interfaces_{}_properties_{}.html",
        short.to_lowercase(),
        name.to_lowercase()
    );
    let indexed = matches!(kind, Item);
    let signature = if indexed {
        format!("Item(in n as Integer) -> {ty}")
    } else {
        format!("{name} as {ty}")
    };
    json!({"id":format!("provisional.com.{short}.{name}"),"source":source(&filename,PAGE_SHA[owner][slot]),
        "displayed_signature":signature,"displayed_property_type":ty,"property_access":"read_only",
        "parameters":if indexed {json!([{"declared_type":"Integer","default_or_optionality":"no_default_or_omission_contract_stated",
            "default_value":null,"direction":"in","name":"n","type_state":"displayed_type"}])} else {json!([])},
        "constraints":if indexed {vec!["Collection index is one-based."]} else {vec![]},
        "documented_error_codes":if owner == 0 {vec!["4000","4001"]} else {vec!["1800","1801"]}})
}

fn parameter(owner: usize, kind: Getter, input: bool, record_offset: u64) -> Value {
    let (name, name_offset, ty, encoded, depth, chain, target) = match (kind, input) {
        (Application | Parent, false) => ("pVal", 64, "IDispatch*", "00000008", 1, vec![8], None),
        (Count, false) => ("pnCount", 2592, "I4*", "00000080", 1, vec![128], None),
        (Item, true) => ("n", 4300, "I4", "80030003", 0, vec![], None),
        (Item, false) if owner == 0 => (
            "ppItem",
            3508,
            "IAppOutputLine**",
            "000000f0",
            2,
            vec![240, 232, 224],
            Some(15),
        ),
        (Item, false) => (
            "ppItem",
            3508,
            "IErrorMarker**",
            "000001e8",
            2,
            vec![488, 480, 472],
            Some(25),
        ),
        (NewEnum, false) => ("ppEnum", 4336, "IUnknown*", "000000f8", 1, vec![248], None),
        _ => unreachable!(),
    };
    json!({"index":usize::from(matches!(kind,Item) && !input),"name":name,"name_offset":name_offset,"record_offset":record_offset,
        "parameter_flags":if input {1} else {10},"direction":if input {vec!["in"]} else {vec!["out","retval"]},
        "type":{"declared_type":ty,"encoded":encoded,"pointer_depth":depth,"descriptor_chain":chain,"target_type_info_index":target}})
}

fn getter(owner: usize, index: usize) -> Value {
    let kind = ORDER[owner][index];
    let offset = OFFSETS[owner][index];
    let (id, name_offset, bytes, dump_lines) = match kind {
        Application => (1_i64, 80, 44, 18),
        Parent => (2, 104, 44, 18),
        Count => (
            if owner == 0 { 0x60020002 } else { 0x60020000 },
            2572,
            36,
            16,
        ),
        Item => (0, 4284, 48, 21),
        NewEnum => (-4, 4316, 36, 16),
    };
    let params = if matches!(kind, Item) {
        vec![
            parameter(owner, kind, true, offset + 24),
            parameter(owner, kind, false, offset + 36),
        ]
    } else {
        vec![parameter(owner, kind, false, offset + bytes - 12)]
    };
    let id_line = [11015, 12064][owner] + index as u64;
    json!({"owner":OWNERS[owner],"name":kind.name(),"record_index":index,"dispatch_member_id":id,
        "name_offset":name_offset,"record_offset":offset,"record_bytes":bytes,"invocation_kind":"property_get","invocation_kind_value":2,
        "function_flags":0,"optional_argument_count":0,"return_type":{"vartype":"HRESULT","encoded":"80190019"},"parameters":params,
        "source_anchor":{"function_dump_lines":[LINES[owner][index],LINES[owner][index]+dump_lines-1],
            "member_id_dump_line":id_line,"member_name_dump_line":id_line+5,"member_offset_dump_line":id_line+10},
        "catalog":catalog(owner,kind)})
}

fn expected() -> &'static Value {
    static EXPECTED: OnceLock<Value> = OnceLock::new();
    EXPECTED.get_or_init(|| {
        let mut value = json!({
            "format":"mapforce-enterprise-2026r2-com-collection-getters","format_version":1,
            "inventory_complete":false,"runtime_proof":false,"status":"unassessed","applicability":"unknown","execution_evidence":[],
            "purpose":"Ten property-get declarations on two collection interfaces only; not ABI or execution qualification.",
            "type_library_source":{"observation":"artifact-observations.json","member":"MapForce.tlb","cab_member":"MapForce.tlb_1193","bytes":54108,
                "sha256":"785c484fbaa882cae724a3568b04650300eb18941084ac23eb2d041dfb4a2789","library":"MapForceLib","version":"4.1","platform":"SYS_WIN64",
                "dump_sha256":"672097183d6a6b0d80f3aa5f92d02172bb3a588609cb743bb82b73c342192bb4","release_binding":"2026r2"},
            "catalog_source":{"file":"com-api.json","sha256":"d9a2fd8a4f8935598fdd2590f837fd6205931ba43c6218dc6b32a29fbe35c16a","release_binding":"unestablished"},
            "readback":{"source":"type_library_source","type_info_table_offset":624,"type_info_record_bytes":100,"type_info_records":75,
                "name_table_offset":10804,"name_table_bytes":13724,"descriptor_table_offset":30276,"descriptor_table_bytes":824,"descriptor_record_bytes":8,
                "owners":[
                    {"name":"IAppOutputLines","type_info_index":14,"kind":"TKIND_DISPATCH","name_offset":3428,"member_block_offset":42508,"member_data_bytes":208,"owner_function_records":5,"member_id_table_offset":42720,"member_name_table_offset":42740,"member_record_table_offset":42760},
                    {"name":"IErrorMarkers","type_info_index":24,"kind":"TKIND_DISPATCH","name_offset":6796,"member_block_offset":45280,"member_data_bytes":208,"owner_function_records":5,"member_id_table_offset":45492,"member_name_table_offset":45512,"member_record_table_offset":45532}],
                "method":[
                    "Verify the official-release selected member by size and SHA-256, and verify the retained dump hash before bounded reads. HTTPS acquisition and byte identity are the asserted provenance; Authenticode integrity, timestamp and certificate-chain trust remain unverified.",
                    "Resolve owner and Item target identities from raw type-info and name records. Check all ten function records, owner-specific member tables and twelve ordered parameter triples against the pinned dump. Absolute record/table offsets and segment-relative name/descriptor offsets remain distinct.",
                    "Follow the Item PTR -> PTR -> USERDEFINED chains to the independently named interface targets. Read _NewEnum as one explicit PTR -> UNKNOWN descriptor, without inventing an extra pointer or concrete enumerator target.",
                    "Compare the eight hash-pinned property snapshots and two interface summaries with com-api.json offline. The summaries supply the read-only labels; the Item pages supply one-based indexing. One-based dump anchors refer to the hash-pinned dump.",
                    "Only declaration metadata is retained. No application, installer, native library or COM object is loaded or executed."]},
            "targets":[{"name":"IAppOutputLine","type_info_index":15,"kind":"TKIND_DISPATCH","name_offset":3456},
                {"name":"IErrorMarker","type_info_index":25,"kind":"TKIND_DISPATCH","name_offset":6824}],
            "descriptors":[
                {"descriptor_offset":8,"descriptor_word":"4009001a","target_word":"80090009","vartype":"PTR","target_encoded":"80090009"},
                {"descriptor_offset":128,"descriptor_word":"4003001a","target_word":"80030003","vartype":"PTR","target_encoded":"80030003"},
                {"descriptor_offset":224,"descriptor_word":"7fff001d","target_word":"000005dc","vartype":"USERDEFINED","type_info_index":15},
                {"descriptor_offset":232,"descriptor_word":"7fff001a","target_word":"000000e0","vartype":"PTR","target_descriptor_offset":224},
                {"descriptor_offset":240,"descriptor_word":"7fff001a","target_word":"000000e8","vartype":"PTR","target_descriptor_offset":232},
                {"descriptor_offset":248,"descriptor_word":"400d001a","target_word":"800d000d","vartype":"PTR","target_encoded":"800d000d"},
                {"descriptor_offset":472,"descriptor_word":"7fff001d","target_word":"000009c4","vartype":"USERDEFINED","type_info_index":25},
                {"descriptor_offset":480,"descriptor_word":"7fff001a","target_word":"000001d8","vartype":"PTR","target_descriptor_offset":472},
                {"descriptor_offset":488,"descriptor_word":"7fff001a","target_word":"000001e0","vartype":"PTR","target_descriptor_offset":480}],
            "catalog_summaries":[],"getters":[],
            "comparison_boundary":{
                "return_notation":"Online property types are higher-level declarations; raw getters return HRESULT with an out/retval parameter. This is not a contradiction or ABI equivalence claim.",
                "navigation":"Online Application is Application for both owners; Parent is AppOutputLine for AppOutputLines but Application for ErrorMarkers. All four raw navigation parameters retain generic PTR(DISPATCH), not concrete interface targets; neither representation replaces the other.",
                "access":"The eight online properties are labeled read_only in the pinned interface summaries. These release-unbound labels and the raw property_get records do not establish runtime mutability.",
                "index_and_errors":"Online Item states one-based indexing and uses Integer notation; raw Item declares direct I4 input. Online Count also uses Integer, separate from raw PTR(I4). The 4000/4001 and 1800/1801 error lists remain online source claims, not observed errors or HRESULT mappings.",
                "enumeration":"Both _NewEnum records are type-library-only, with signed member ID -4, function flags zero and one explicit PTR(UNKNOWN) descriptor. No hidden/restricted flag, extra pointer, IEnumVARIANT target, enumeration protocol, traversal order or enumerator behavior is inferred.",
                "qualification":"Declaration metadata does not establish ownership, lifetime, null/error behavior, runtime defaults, coercion, marshaling, calling convention, ABI or backend support. HTTPS acquisition and byte identity do not establish Authenticode integrity or trust."},
            "counts":{"interfaces":2,"property_gets":10,"parameters":12,"in_parameters":2,"out_retval_parameters":10,"catalog_properties":8,"type_library_only_getters":2},
            "remaining":["Other COM signatures remain outside this bounded fragment; inventory_complete remains false.",
                "The pinned online catalog's release binding remains unestablished.",
                "No activation or execution evidence is supplied; canonical capability assessments are unchanged."]
        });
        value["getters"] = json!((0..2).flat_map(|o| (0..5).map(move |i| getter(o,i))).collect::<Vec<_>>());
        value["catalog_summaries"] = json!([
            {"owner":OWNERS[0],"source":source("mfapi_interfaces_appoutputlines.html","e8ffdc28052f7d1ffe7423fb30682c474daa55942acb99e58a4ad8a0365f2898"),"property_access":"read_only"},
            {"owner":OWNERS[1],"source":source("mfapi_interfaces_errormarkers.html","598da1c2799eb54c9f84778a9232bb0498d5e50e5a4fbea5e67ca5a37a0f85b1"),"property_access":"read_only"}]);
        value
    })
}

fn check(value: &Value) {
    source_only(value);
    assert_eq!(value, expected());
}

fn rejects(value: &Value, label: &str) {
    assert!(
        std::panic::catch_unwind(|| check(value)).is_err(),
        "accepted mutation: {label}"
    );
}

#[test]
fn collection_getters_match_independent_declarations_and_adopted_sources() {
    let inventory = load("com-collection-getters.json");
    check(&inventory);
    let online = load("com-api.json");
    assert_eq!(online["release_binding"], "unestablished");
    for getter in inventory["getters"].as_array().unwrap() {
        if getter["catalog"].is_null() {
            let id = format!(
                "provisional.com.{}._NewEnum",
                &getter["owner"].as_str().unwrap()[1..]
            );
            assert!(
                !online["members"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["id"] == id)
            );
            continue;
        }
        let c = &getter["catalog"];
        let adopted = online["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == c["id"])
            .unwrap();
        assert_eq!(adopted["source"], c["source"]);
        assert_eq!(adopted["constraints"], c["constraints"]);
        assert_eq!(
            adopted["documented_error_codes"],
            c["documented_error_codes"]
        );
        assert_eq!(
            adopted["contract"],
            json!({"arity":c["parameters"].as_array().unwrap().len(),
            "declared_return_or_property_type":c["displayed_property_type"],"declared_signature_or_token":c["displayed_signature"],
            "parameters":c["parameters"],"property_access":"read_only","return_contract_state":"displayed_type",
            "runtime_proof":false,"signature_state":"typed_displayed_declaration"})
        );
    }
    for summary in inventory["catalog_summaries"].as_array().unwrap() {
        let adopted = online["source_documents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["source_file"] == summary["source"]["source_file"])
            .unwrap();
        for (key, value) in summary["source"].as_object().unwrap() {
            assert_eq!(&adopted[key], value);
        }
        assert_eq!(adopted["title"], &summary["owner"].as_str().unwrap()[1..]);
    }
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
    for owner in inventory["readback"]["owners"].as_array().unwrap() {
        let aligned = alignment["interfaces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["type_library_name"] == owner["name"])
            .unwrap();
        assert_eq!(aligned["type_info_index"], owner["type_info_index"]);
        assert_eq!(aligned["record_count"], 5);
        assert_eq!(aligned["type_library_only"], json!(["_NewEnum"]));
    }
    assert_eq!(artifacts["release"]["release"], "2026r2");
    let canonical: Value =
        serde_json::from_str(include_str!("../../../../conformance/mfd-2026r2.json")).unwrap();
    assert_eq!(canonical["inventory_complete"], false);
}

// Every leaf includes signed IDs, parameter flags/types, hashes, source anchors
// and boundary wording. Full-value equality also rejects unsupported extra keys.
fn mutate_leaves(original: &Value, node: &Value, path: &str) {
    match node {
        Value::Object(fields) => {
            for (key, child) in fields {
                mutate_leaves(original, child, &format!("{path}/{key}"));
            }
        }
        Value::Array(values) if !values.is_empty() => {
            for (i, child) in values.iter().enumerate() {
                mutate_leaves(original, child, &format!("{path}/{i}"));
            }
        }
        _ => {
            let wrong = match node {
                Value::Bool(b) => json!(!b),
                Value::Number(n) => json!(n.as_i64().unwrap() + 1),
                Value::String(_) => json!("wrong"),
                Value::Null => json!(0),
                Value::Array(_) => json!(["unsupported"]),
                _ => unreachable!(),
            };
            let mut changed = original.clone();
            *changed.pointer_mut(path).unwrap() = wrong;
            rejects(&changed, path);
        }
    }
}

// Structural controls keep the cohort's redundant counts consistent, so a
// rejected omission/duplicate is not just a bookkeeping mismatch.
fn recount(value: &mut Value) {
    let getters = value["getters"].as_array().unwrap();
    let params: Vec<_> = getters
        .iter()
        .flat_map(|g| g["parameters"].as_array().unwrap())
        .collect();
    let catalog = getters.iter().filter(|g| !g["catalog"].is_null()).count();
    value["counts"] = json!({"interfaces":2,"property_gets":getters.len(),"parameters":params.len(),
        "in_parameters":params.iter().filter(|p| p["parameter_flags"] == 1).count(),
        "out_retval_parameters":params.iter().filter(|p| p["parameter_flags"] == 10).count(),
        "catalog_properties":catalog,"type_library_only_getters":getters.len()-catalog});
}

#[test]
fn collection_getters_reject_every_leaf_missing_duplicates_and_order() {
    let original = load("com-collection-getters.json");
    mutate_leaves(&original, &original, "");
    for i in 0..10 {
        let mut changed = original.clone();
        changed["getters"].as_array_mut().unwrap().remove(i);
        recount(&mut changed);
        rejects(&changed, "missing record with adjusted counts");
        let mut changed = original.clone();
        changed["getters"][(i + 1) % 10] = original["getters"][i].clone();
        recount(&mut changed);
        rejects(&changed, "same-count duplicate");
        let mut changed = original.clone();
        changed["getters"]
            .as_array_mut()
            .unwrap()
            .swap(i, (i + 1) % 10);
        rejects(&changed, "reordered records");
        let mut changed = original.clone();
        changed["getters"][i]["parameters"]
            .as_array_mut()
            .unwrap()
            .pop();
        recount(&mut changed);
        rejects(&changed, "missing parameter with adjusted counts");
        let mut changed = original.clone();
        changed["getters"][i]["parameters"]
            .as_array_mut()
            .unwrap()
            .push(original["getters"][i]["parameters"][0].clone());
        recount(&mut changed);
        rejects(&changed, "duplicate parameter");
    }
    for i in [3, 6] {
        let mut changed = original.clone();
        changed["getters"][i]["parameters"]
            .as_array_mut()
            .unwrap()
            .swap(0, 1);
        for p in 0..2 {
            changed["getters"][i]["parameters"][p]["index"] = json!(p);
            changed["getters"][i]["parameters"][p]["record_offset"] =
                original["getters"][i]["parameters"][p]["record_offset"].clone();
        }
        rejects(&changed, "coherent parameter order substitution");
    }
}

#[test]
fn collection_getters_reject_coherent_ids_flags_owners_and_pointer_targets() {
    let original = load("com-collection-getters.json");
    let mut changed = original.clone();
    changed["getters"][2]["dispatch_member_id"] = json!(0x60020000);
    changed["getters"][5]["dispatch_member_id"] = json!(0x60020002);
    rejects(&changed, "cross-owner Count IDs");
    let mut changed = original.clone();
    for i in 0..10 {
        changed["getters"][i]["owner"] = json!(OWNERS[usize::from(i < 5)]);
    }
    changed["readback"]["owners"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    rejects(&changed, "coherent owner swap");
    for value in [json!(4294967292_u64), json!(4)] {
        let mut changed = original.clone();
        for i in [4, 7] {
            changed["getters"][i]["dispatch_member_id"] = value.clone();
        }
        rejects(&changed, "lost signed negative DISPID");
    }
    for flags in [1, 64, 65] {
        let mut changed = original.clone();
        for i in [4, 7] {
            changed["getters"][i]["function_flags"] = json!(flags);
        }
        rejects(&changed, "invented restricted/hidden flags");
    }
    let mut changed = original.clone();
    for (i, other) in [(3, 6), (6, 3)] {
        changed["getters"][i]["parameters"][1]["type"] =
            original["getters"][other]["parameters"][1]["type"].clone();
        changed["getters"][i]["catalog"]["displayed_property_type"] =
            original["getters"][other]["catalog"]["displayed_property_type"].clone();
        changed["getters"][i]["catalog"]["displayed_signature"] =
            original["getters"][other]["catalog"]["displayed_signature"].clone();
    }
    changed["targets"].as_array_mut().unwrap().swap(0, 1);
    rejects(&changed, "coherent cross-owner Item targets");
    for i in [3, 6] {
        let mut changed = original.clone();
        let (ty, encoded, chain, target) = if i == 3 {
            ("IAppOutputLine*", "000000e8", vec![232, 224], 15)
        } else {
            ("IErrorMarker*", "000001e0", vec![480, 472], 25)
        };
        changed["getters"][i]["parameters"][1]["type"] = json!({"declared_type":ty,"encoded":encoded,"pointer_depth":1,"descriptor_chain":chain,"target_type_info_index":target});
        rejects(&changed, "coherent missing Item pointer layer");
    }
    let mut changed = original.clone();
    changed["descriptors"].as_array_mut().unwrap().push(json!({"descriptor_offset":816,"descriptor_word":"7fff001a","target_word":"000000f8","vartype":"PTR","target_descriptor_offset":248}));
    for i in [4, 7] {
        changed["getters"][i]["parameters"][0]["type"] = json!({"declared_type":"IUnknown**","encoded":"00000330","pointer_depth":2,"descriptor_chain":[816,248],"target_type_info_index":null});
    }
    rejects(&changed, "coherent invented _NewEnum pointer layer");
    let mut changed = original.clone();
    for i in [4, 7] {
        changed["getters"][i]["parameters"][0]["type"]["declared_type"] = json!("IEnumVARIANT*");
        changed["getters"][i]["parameters"][0]["type"]["target_type_info_index"] = json!(26);
        changed["getters"][i]["catalog"] =
            json!({"id":format!("provisional.com.{}._NewEnum",&OWNERS[usize::from(i==7)][1..])});
    }
    rejects(&changed, "invented enumerator target and catalog entries");
}

#[test]
fn collection_getters_reject_online_substitution_and_qualification_promotions() {
    let original = load("com-collection-getters.json");
    for i in [0, 1, 8, 9] {
        let mut changed = original.clone();
        changed["getters"][i]["parameters"][0]["type"] = json!({"declared_type":"IApplication**","encoded":"00000008","pointer_depth":2,"descriptor_chain":[8],"target_type_info_index":0});
        rejects(
            &changed,
            "online concrete object replacing generic dispatch",
        );
    }
    let mut changed = original.clone();
    for i in [2, 5] {
        changed["getters"][i]["parameters"][0]["type"]["declared_type"] = json!("Integer*");
    }
    for i in [3, 6] {
        changed["getters"][i]["parameters"][0]["type"]["declared_type"] = json!("Integer");
    }
    rejects(&changed, "online notation replacing raw I4");
    let mut changed = original.clone();
    for field in [
        "member_block_offset",
        "member_id_table_offset",
        "member_name_table_offset",
        "member_record_table_offset",
    ] {
        changed["readback"]["owners"][0][field] =
            json!(original["readback"]["owners"][0][field].as_u64().unwrap() + 4);
    }
    for i in 0..5 {
        changed["getters"][i]["record_offset"] =
            json!(original["getters"][i]["record_offset"].as_u64().unwrap() + 4);
        for parameter in changed["getters"][i]["parameters"].as_array_mut().unwrap() {
            parameter["record_offset"] = json!(parameter["record_offset"].as_u64().unwrap() + 4);
        }
    }
    rejects(&changed, "coherent raw source offset relocation");
    let mut changed = original.clone();
    changed["catalog_source"]["release_binding"] = json!("2026r2");
    for getter in changed["getters"].as_array_mut().unwrap() {
        if !getter["catalog"].is_null() {
            getter["catalog"]["source"]["release_binding"] = json!("2026r2");
        }
    }
    for summary in changed["catalog_summaries"].as_array_mut().unwrap() {
        summary["source"]["release_binding"] = json!("2026r2");
    }
    rejects(&changed, "coherent online release promotion");
    for field in [
        "abi_verified",
        "calling_convention",
        "backend_support",
        "runtime_default",
        "ownership",
        "lifetime",
        "null_behavior",
        "error_behavior",
        "coercion",
        "marshaling",
        "enumeration_protocol",
        "traversal_order",
        "authenticode_verified",
    ] {
        let mut changed = original.clone();
        changed[field] = json!(true);
        rejects(&changed, field);
        let mut changed = original.clone();
        changed["getters"][4][field] = json!(true);
        rejects(&changed, field);
    }
}
