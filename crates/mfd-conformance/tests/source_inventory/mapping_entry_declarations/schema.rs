//! Closed source metadata, not a compiler model or executable API contract.

use serde::{Deserialize, Serialize};

macro_rules! schema {
    ($($name:ident { $($field:ident: $ty:ty),* $(,)? })*) => {$ (
        #[derive(Debug, Deserialize, Serialize)]
        #[serde(deny_unknown_fields)]
        pub(super) struct $name { $(pub(super) $field: $ty),* }
    )*};
}

// Option alone accepts an omitted field. Declaration silence must be explicit.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub(super) struct Nullable<T>(pub(super) Option<T>);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Nullable<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Option::<T>::deserialize(value)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

schema! {
    Inventory {
        format: String,
        format_version: u32,
        inventory_complete: bool,
        runtime_proof: bool,
        status: String,
        applicability: String,
        execution_evidence: Vec<String>,
        execution_receipts: Vec<String>,
        assessment_boundary: Assessment,
        scope: Scope,
        provenance: Provenance,
        oracle: Oracle,
        profiles: Vec<Profile>,
        contract_enrichments: Vec<Contract>,
        io_base_joins: Vec<IoJoin>,
        sources: Vec<Source>,
        owners: Vec<Owner>,
    }
    Assessment {
        runtime_proof: bool,
        status: String,
        applicability: String,
        execution_evidence: Vec<String>,
        execution_receipts: Vec<String>,
    }
    Scope {
        profile_count: usize,
        source_count: usize,
        owner_count: usize,
        direct_member_count: usize,
        parameter_count: usize,
        accessor_count: usize,
        throws_clause_count: usize,
        explicit_constructor_count: usize,
        api_shape: String,
        inherited_members: String,
        implicit_members: String,
        implementation_copied: bool,
        excluded: Vec<String>,
        unqualified: Vec<String>,
    }
    Provenance {
        observation: String,
        wrapper_observation: String,
        generator_version: String,
        generator_sha256: String,
        ordinary_manifest_sha256: String,
        wrapper_manifest_sha256: String,
        release_comparison_sha256: String,
        source_binding: String,
        historical_manifest_caveat: String,
        signature_trust: String,
        profile_byte_identity: bool,
        profile_integration_signatures_equal: bool,
        vendor_generation: bool,
        vendor_compilation: bool,
        vendor_execution: bool,
        inputs: Vec<Input>,
    }
    Input { name: String, role: String, bytes: u64, sha256: String }
    Oracle { path: String, method: String }
    Profile {
        id: String,
        manifest_sha256: String,
        generator_version: String,
        settings: String,
        static_input_count: usize,
        output_count: usize,
        api_scope: String,
    }
    Contract {
        file: String,
        pointer: String,
        identity_field: String,
        identity: String,
        member_refs: Vec<String>,
        relationship: String,
    }
    IoJoin {
        owner: String,
        research_id: String,
        file: String,
        pointer: String,
        relationship: String,
    }
    Source {
        index: usize,
        profile: String,
        language: String,
        path: String,
        bytes: u64,
        sha256: String,
        lf_lines: u32,
        namespace: String,
        namespace_lines: [u32; 2],
        imports: Vec<Import>,
        conditions: Vec<String>,
    }
    Import { name: String, declaration_lines: [u32; 2] }
    Type { spelling: String, resolved: Nullable<String>, binding: String }
    Owner {
        name: String,
        language: String,
        kind: String,
        modifiers: Vec<String>,
        extends: Vec<Type>,
        implements: Vec<Type>,
        anchors: Vec<OwnerAnchor>,
        explicit_constructors: Vec<String>,
        conditions: Vec<String>,
        assessment_ref: String,
        members: Vec<Member>,
    }
    OwnerAnchor {
        source_index: usize,
        declaration_lines: [u32; 2],
        closing_line: u32,
    }
    Anchor { source_index: usize, declaration_lines: [u32; 2] }
    Member {
        index: usize,
        kind: String,
        name: String,
        r#type: Type,
        modifiers: Vec<String>,
        parameters: Vec<Parameter>,
        throws: Vec<Type>,
        accessors: Vec<Accessor>,
        anchors: Vec<Anchor>,
        directly_declared: bool,
        conditions: Vec<String>,
    }
    Parameter {
        index: usize,
        name: String,
        r#type: Type,
        direction: String,
        default: Nullable<String>,
    }
    Accessor {
        name: String,
        visibility: String,
        explicit_visibility: Nullable<String>,
        anchors: Vec<Anchor>,
    }
}
