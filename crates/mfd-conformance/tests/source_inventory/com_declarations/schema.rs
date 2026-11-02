//! Shared supplemental declaration schema, not a runtime or ABI model.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

// Missing is not the same as explicitly unavailable. Require nullable source
// fields to be present, including unmatched catalog links and missing anchors.
fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    pub inventory_complete: bool,
    pub runtime_proof: bool,
    pub status: String,
    pub applicability: String,
    pub execution_evidence: Vec<Value>,
}

impl Assessment {
    pub fn check(&self) {
        assert!(!self.inventory_complete && !self.runtime_proof);
        assert_eq!(self.status, "unassessed");
        assert_eq!(self.applicability, "unknown");
        assert!(self.execution_evidence.is_empty());
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Sources {
    pub observation: String,
    pub member: String,
    pub cab_member: String,
    pub bytes: u32,
    pub sha256: String,
    pub library: String,
    pub version: String,
    pub platform: String,
    pub dump_sha256: String,
    pub release_binding: String,
    pub catalog_file: String,
    pub catalog_sha256: String,
    pub catalog_release_binding: String,
    pub type_info_table_offset: u32,
    pub type_info_record_bytes: u32,
    pub type_info_records: u32,
    pub name_table_offset: u32,
    pub name_table_bytes: u32,
    pub descriptor_table_offset: u32,
    pub descriptor_table_bytes: u32,
    pub descriptor_record_bytes: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum TypeKind {
    #[serde(rename = "TKIND_DISPATCH")]
    Dispatch,
    #[serde(rename = "TKIND_ENUM")]
    Enum,
    #[serde(rename = "TKIND_COCLASS")]
    Coclass,
}

impl TypeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dispatch => "TKIND_DISPATCH",
            Self::Enum => "TKIND_ENUM",
            Self::Coclass => "TKIND_COCLASS",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TypeInfo {
    pub type_info_index: u32,
    pub name: String,
    pub kind: TypeKind,
    pub name_offset: u32,
    pub header_dump_lines: [u32; 2],
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub descriptor_offset: u32,
    pub descriptor_word: String,
    pub target_word: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub name: String,
    pub type_info_index: u32,
    pub type_flags: u32,
    pub member_block_offset: u32,
    pub member_data_bytes: u32,
    pub function_records: u32,
    pub member_id_table_offset: u32,
    pub member_name_table_offset: u32,
    pub member_record_table_offset: u32,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Leaf {
    Primitive {
        name: Primitive,
    },
    UserDefined {
        name: String,
        type_info_index: u32,
        type_kind: TypeKind,
    },
}

impl Leaf {
    pub fn tuple(&self) -> String {
        match self {
            Self::Primitive { name } => name.label().into(),
            Self::UserDefined {
                name,
                type_info_index,
                type_kind,
            } => format!("{}:{type_info_index}:{name}", type_kind.label()),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Primitive {
    I4,
    Bstr,
    Dispatch,
    Bool,
    Variant,
    Unknown,
    Int,
    Uint,
    Void,
    Hresult,
}

impl Primitive {
    pub fn from_tag(tag: u32) -> Self {
        match tag {
            3 => Self::I4,
            8 => Self::Bstr,
            9 => Self::Dispatch,
            11 => Self::Bool,
            12 => Self::Variant,
            13 => Self::Unknown,
            22 => Self::Int,
            23 => Self::Uint,
            24 => Self::Void,
            25 => Self::Hresult,
            _ => panic!("unreviewed primitive tag: {tag}"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::I4 => "I4",
            Self::Bstr => "BSTR",
            Self::Dispatch => "DISPATCH",
            Self::Bool => "BOOL",
            Self::Variant => "VARIANT",
            Self::Unknown => "UNKNOWN",
            Self::Int => "INT",
            Self::Uint => "UINT",
            Self::Void => "VOID",
            Self::Hresult => "HRESULT",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DataType {
    pub encoded: String,
    pub pointer_depth: u32,
    pub leaf: Leaf,
    pub descriptor_chain: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Invocation {
    Function,
    PropertyGet,
    PropertyPut,
}

impl Invocation {
    pub fn value(self) -> u32 {
        match self {
            Self::Function => 1,
            Self::PropertyGet => 2,
            Self::PropertyPut => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    In,
    Out,
    Retval,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub index: u32,
    #[serde(deserialize_with = "required_nullable")]
    pub name: Option<String>,
    pub name_offset: u32,
    pub record_offset: u32,
    pub parameter_flags: u32,
    pub direction: Vec<Direction>,
    #[serde(rename = "type")]
    pub datatype: DataType,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub function_dump_lines: [u32; 2],
    pub member_id_dump_line: u32,
    pub member_name_dump_line: u32,
    pub member_offset_dump_line: u32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub owner: String,
    pub type_info_index: u32,
    pub name: String,
    pub record_index: u32,
    pub dispatch_member_id: i32,
    pub raw_member_id: String,
    pub name_offset: u32,
    pub record_offset: u32,
    pub record_bytes: u32,
    pub invocation_kind: Invocation,
    pub invocation_kind_value: u32,
    pub function_flags: u32,
    pub optional_argument_count: u32,
    pub return_type: DataType,
    pub parameters: Vec<Parameter>,
    pub source_anchor: Anchor,
    #[serde(deserialize_with = "required_nullable")]
    pub catalog_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Declarations {
    pub format: String,
    pub format_version: u32,
    pub cohort: String,
    pub assessment: Assessment,
    pub sources: Sources,
    pub types: Vec<TypeInfo>,
    pub descriptors: Vec<Descriptor>,
    pub owners: Vec<Owner>,
    pub records: Vec<Record>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageCounts {
    pub type_infos: u32,
    pub ordinary_records: u32,
    pub event_records: u32,
    pub adopted_records: u32,
    pub remaining_records: u32,
    pub enum_types: u32,
    pub enum_variables: u32,
    pub zero_callable_types: u32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Cohort {
    pub id: String,
    pub state: String,
    #[serde(deserialize_with = "required_nullable")]
    pub fragment: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub record_field: Option<String>,
    pub record_count: u32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeclarationCount {
    pub type_info_index: u32,
    pub name: String,
    pub kind: TypeKind,
    pub function_records: u32,
    pub variable_records: u32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageRecord {
    pub type_info_index: u32,
    pub owner: String,
    pub record_index: u32,
    pub name: String,
    pub invocation_kind: Invocation,
    pub dispatch_member_id: i32,
    pub record_offset: u32,
    pub record_bytes: u32,
    #[serde(deserialize_with = "required_nullable")]
    pub function_dump_lines: Option<[u32; 2]>,
    pub cohort: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    pub format: String,
    pub format_version: u32,
    pub assessment: Assessment,
    pub source: String,
    pub counts: CoverageCounts,
    pub cohorts: Vec<Cohort>,
    pub declarations: Vec<DeclarationCount>,
    pub records: Vec<CoverageRecord>,
}
