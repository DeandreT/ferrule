//! Explicit MapForce EDI scalar formatting metadata, separate from strict X12.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapForceEdiScalarKind {
    Text,
    Decimal,
    Date,
    Time,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapForceEdiScalar {
    pub kind: MapForceEdiScalarKind,
    pub min_chars: u32,
    pub max_chars: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapForceEdiMinimum {
    pub path: Vec<String>,
    pub minimum: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapForceX12WriterDescriptor {
    pub boundary: String,
    pub minimums: Vec<MapForceEdiMinimum>,
}
