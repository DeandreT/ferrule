//! Exact metadata/column contract: unknown fields cannot add qualification claims.

use std::sync::OnceLock;

use serde_json::{Value, json};

pub fn expected() -> &'static Value {
    static VALUE: OnceLock<Value> = OnceLock::new();
    VALUE.get_or_init(|| {
        json!({
            "format": "mapforce-enterprise-2026r2-com-type-relationships",
            "format_version": 1,
            "assessment": {
                "inventory_complete": false, "runtime_proof": false,
                "status": "unassessed", "applicability": "unknown", "execution_evidence": []
            },
            "sources": {
                "observation": "artifact-observations.json", "member": "MapForce.tlb",
                "cab_member": "MapForce.tlb_1193", "bytes": 54108,
                "sha256": "785c484fbaa882cae724a3568b04650300eb18941084ac23eb2d041dfb4a2789",
                "dump_sha256": "672097183d6a6b0d80f3aa5f92d02172bb3a588609cb743bb82b73c342192bb4",
                "release_binding": "2026r2", "type_info_table": [624,7500,100],
                "guid_table": [8252,1488,24], "reference_table": [9740,512,16],
                "name_table": [10804,13724,453], "import_info": [10252,12], "import_file": [10264,28]
            },
            "columns": {
                "types": ["index","name","kind","name_offset","guid_offset_word","nullable_guid",
                    "type_flags","version_word","c_impl_types","datatype1_word","datatype2_word",
                    "function_variable_counts","header_extra","source_anchor"],
                "header_extra": ["alignment","vtable_bytes","instance_bytes","custom_data_word"],
                "type_source_anchor": ["header_file_offset","header_dump_first","header_dump_last",
                    "name_record_index","name_dump_first","name_dump_last"],
                "guids": ["index","guid","hreftype_word","next_hash_word","role","source_anchor"],
                "guid_source_anchor": ["file_offset","dump_first","dump_last"],
                "references": ["segment_offset","owner_type_index","owner_ordinal","target_hreftype_word",
                    "target_identity","impl_flags","custom_data_word","next_word","source_anchor"],
                "target_identity": ["type_index","name","guid"],
                "reference_source_anchor": ["file_offset","global_dump_line"]
            },
            "library": {
                "name": "MapForceLib", "guid": "bda2fca6-c4e9-46fb-b157-80980a86d686", "guid_index": 0,
                "version": "4.1", "platform": "SYS_WIN64", "varflags_word": "00000043",
                "lcid_word": "00000409", "flags_word": "00000000",
                "header_dump_lines": [3,25], "name_dump_lines": [2693,2698]
            },
            "import": {
                "flags_word": "03010000", "file_offset_word": "00000000",
                "interface_guid_offset_word": "00000078", "interface_guid": "00020400-0000-0000-c000-000000000046",
                "library_guid_offset_word": "00000060", "library_guid": "00020430-0000-0000-c000-000000000046",
                "lcid_word": "00000000", "version_word": "00000002", "file_name": "stdole2.tlb",
                "dump_lines": [2648,2658], "dependency_availability": "unassessed"
            },
            "counts": {
                "existing_type_identities": 75, "ordinary_dispatch": 22, "event_dispatch": 9,
                "dummy_dispatch": 1, "coclasses": 23, "enums": 20, "guid_present": 55,
                "guid_absent": 20, "guid_table_records": 62, "reference_records": 32,
                "default_interfaces": 23, "default_sources": 9, "zero_callable_types": 29,
                "ordinary_callable_records": 317, "event_callable_records": 8, "enum_variable_records": 130
            },
            "boundaries": [
                "HTTPS acquisition and byte identity bind the source; Authenticode integrity and trust are unverified.",
                "These 75 existing type identities are enriched metadata, not 75 additional capabilities; callable and enum declarations are joined, not duplicated.",
                "RefTab global rows at dump lines 2615-2646 are anchored by segment-relative ordinal: printed first address 0000280c is displaced from actual segment offset 0000260c.",
                "Late TypeInfo/RefRecord renderings, including lines 15148-15161 and 15239-15254, are invalid coclass-list evidence; use the global RefTab rows.",
                "Event datatype1=ffffffff remains unresolved even with cImplTypes=1; ordinary/dummy datatype1=00000001 is retained separately, not converted to invented event-base edges.",
                "Dummy and source headers do not establish an intended public-use contract. Library platform/version and GUIDs do not qualify activation or ABI.",
                "Import identities and stdole2.tlb are source references, not dependency availability. No native execution, marshaling, ownership, lifetime, defaults or runtime behavior is qualified."
            ]
        })
    })
}

pub fn check(value: &Value) {
    let fields = value.as_object().unwrap();
    assert_eq!(fields.len(), expected().as_object().unwrap().len() + 3);
    for (key, expected) in expected().as_object().unwrap() {
        assert_eq!(&value[key], expected, "metadata {key}");
    }
    for key in ["types", "guids", "references"] {
        assert!(fields.contains_key(key));
    }
}
