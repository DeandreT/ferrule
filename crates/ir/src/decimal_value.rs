//! Lossless, strictly tagged decimal scalar persistence.
use serde::{Deserialize, Serialize};

use crate::Decimal96;

pub(super) fn serialize<S>(value: &Decimal96, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    #[derive(Serialize)]
    struct Repr {
        #[serde(rename = "$decimal96")]
        value: String,
    }
    Repr {
        value: value.to_string(),
    }
    .serialize(serializer)
}

pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Box<Decimal96>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Repr {
        #[serde(rename = "$decimal96")]
        value: String,
    }
    let repr = Repr::deserialize(deserializer)?;
    let value = Decimal96::parse_exact(&repr.value).map_err(serde::de::Error::custom)?;
    if value.to_string() != repr.value {
        return Err(serde::de::Error::custom(
            "decimal scalar text must be canonical",
        ));
    }
    Ok(Box::new(value))
}
