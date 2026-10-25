//! Wire identities never pass through a JavaScript number.
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

macro_rules! identity {
    ($name:ident, $inner:ty) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub $inner);
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0.to_string())
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                if text.is_empty()
                    || (text.len() > 1 && text.starts_with('0'))
                    || !text.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return Err(de::Error::custom(
                        "expected a canonical unsigned decimal string",
                    ));
                }
                text.parse().map(Self).map_err(de::Error::custom)
            }
        }
    };
}
identity!(NodeId, u32);
identity!(FunctionId, u64);
// A fixed wire width avoids target-dependent acceptance on wasm32.
identity!(Index, u32);
