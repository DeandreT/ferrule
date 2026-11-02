use serde::{Deserialize, Deserializer};

use crate::{
    FiniteF64, GroupAlternative, GroupAlternativeConstraintValue, ScalarType, ScalarTypeSet,
    SchemaKind, SchemaNode,
};

use super::number_serde::NumberDeserializer;

impl<'de> Deserialize<'de> for SchemaKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(
            remote = "SchemaKind",
            rename = "SchemaKind",
            tag = "kind",
            rename_all = "snake_case"
        )]
        enum Repr {
            Scalar {
                ty: ScalarType,
            },
            ScalarUnion {
                types: ScalarTypeSet,
            },
            Group {
                children: Vec<SchemaNode>,
                #[serde(default)]
                alternatives: Vec<GroupAlternative>,
                #[serde(default)]
                required: Vec<String>,
                #[serde(default)]
                xml_restricted_alternatives: Vec<String>,
                #[serde(default)]
                dynamic: Option<Box<SchemaNode>>,
            },
        }

        Repr::deserialize(NumberDeserializer(deserializer))
    }
}

impl<'de> Deserialize<'de> for GroupAlternativeConstraintValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(
            remote = "GroupAlternativeConstraintValue",
            rename = "GroupAlternativeConstraintValue",
            tag = "type",
            content = "value",
            rename_all = "snake_case"
        )]
        enum Repr {
            String(String),
            Int(i64),
            Float(FiniteF64),
            Bool(bool),
            JsonNull,
        }

        Repr::deserialize(NumberDeserializer(deserializer))
    }
}
