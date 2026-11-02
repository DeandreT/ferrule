//! A source-declaration schema, deliberately independent of COM/ABI descriptors.

use serde::{Deserialize, Serialize};

macro_rules! schema {
    ($($name:ident { $($field:ident: $ty:ty),* $(,)? })*) => {$ (
        #[derive(Debug, Deserialize, Serialize)]
        #[serde(deny_unknown_fields)]
        pub(super) struct $name { $(pub(super) $field: $ty),* }
    )*};
}

// Unlike an Option field, this wrapper rejects an omitted field. Null here is
// source silence, never an inferred language or runtime default.
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
        purpose: String,
        assessment_boundary: Assessment,
        scope: Scope,
        provenance: Provenance,
        anchoring: String,
        normalization: String,
        oracle: Oracle,
        manual_comparison: ManualComparison,
        sources: Vec<Source>,
        owners: Vec<Owner>,
    }
    Assessment {
        status: String,
        applicability: String,
        runtime_proof: bool,
        execution_evidence: Vec<String>,
        execution_receipts: Vec<String>,
    }
    Scope {
        owner_count: usize,
        source_count: usize,
        direct_member_count: usize,
        nested_type_count: usize,
        enum_constant_count: usize,
        member_counts: MemberCounts,
        inherited_members: String,
        implicit_members: String,
        implementation_copied: bool,
        excluded: Vec<String>,
        unqualified: Vec<String>,
    }
    MemberCounts {
        constructor: usize,
        method: usize,
        property: usize,
        field: usize,
        constant: usize,
    }
    Provenance {
        observation: String,
        wrapper_observation: String,
        generator_version: String,
        generator_sha256: String,
        ordinary_manifest_sha256: String,
        wrapper_manifest_sha256: String,
        release_comparison_sha256: String,
        inputs: Vec<Input>,
        ordinary_settings: String,
        wrapper_settings: String,
        binding: String,
        audit: String,
        vendor_generation: bool,
        vendor_compilation: bool,
        vendor_execution: bool,
    }
    Input {
        name: String,
        role: String,
        bytes: u64,
        sha256: String,
    }
    Oracle {
        path: String,
        method: String,
    }
    ManualComparison {
        research_id: String,
        manual_signature: String,
        manual_text_lines: [u32; 2],
        generated_public_constructor_parameter_types: Vec<Vec<String>>,
        relationship: String,
    }
    Source {
        index: usize,
        language: String,
        path: String,
        bytes: u64,
        sha256: String,
        wrapper_sha256: String,
        wrapper_byte_identical: bool,
        lf_lines: u32,
        imports: Vec<Import>,
        conditions: Vec<String>,
    }
    Import {
        name: String,
        declaration_lines: [u32; 2],
    }
    Type {
        spelling: String,
        resolved: String,
    }
    Owner {
        research_id: String,
        name: String,
        kind: String,
        language: String,
        source_index: usize,
        source_type_index: usize,
        declaration_lines: [u32; 2],
        modifiers: Vec<String>,
        extends: Vec<Type>,
        implements: Vec<Type>,
        conditions: Vec<String>,
        assessment_ref: String,
        members: Vec<Member>,
        nested_types: Vec<NestedType>,
    }
    Member {
        index: usize,
        declaration_lines: [u32; 2],
        kind: String,
        name: String,
        r#type: Nullable<Type>,
        modifiers: Vec<String>,
        parameters: Vec<Parameter>,
        throws: Vec<Type>,
        accessors: Vec<Accessor>,
        annotations: Vec<String>,
        constant_value: Nullable<String>,
        value_status: String,
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
        declaration_lines: [u32; 2],
    }
    NestedType {
        name: String,
        kind: String,
        modifiers: Vec<String>,
        declaration_lines: [u32; 2],
        underlying_type: Nullable<Type>,
        constants: Vec<EnumConstant>,
        conditions: Vec<String>,
    }
    EnumConstant {
        name: String,
        value: String,
        declaration_lines: [u32; 2],
    }
}
