//! Lossless, strictly tagged decimal scalar persistence.
use serde::{
    Serialize,
    de::{self, MapAccess, Visitor},
};
use std::fmt;

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
    struct DecimalVisitor;

    impl<'de> Visitor<'de> for DecimalVisitor {
        type Value = Box<Decimal96>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an object containing only a canonical $decimal96 string")
        }

        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: MapAccess<'de>,
        {
            let mut lexical = None;
            while let Some(key) = map.next_key::<String>()? {
                if key != "$decimal96" {
                    return Err(de::Error::unknown_field(&key, &["$decimal96"]));
                }
                if lexical.is_some() {
                    return Err(de::Error::duplicate_field("$decimal96"));
                }
                lexical = Some(map.next_value::<String>()?);
            }
            let lexical = lexical.ok_or_else(|| de::Error::missing_field("$decimal96"))?;
            let value = Decimal96::parse_exact(&lexical).map_err(de::Error::custom)?;
            if value.to_string() != lexical {
                return Err(de::Error::custom("decimal scalar text must be canonical"));
            }
            Ok(Box::new(value))
        }
    }

    // A derived struct deserializer also accepts positional sequences. The
    // decimal carrier requires its explicit object tag, not a one-item array.
    deserializer.deserialize_map(DecimalVisitor)
}
