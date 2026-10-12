//! Explicit source identity DTOs, independent of project and runtime admission.
//!
//! Owned paths are relative to their selected endpoint. Legacy path arrays keep
//! their existing representation and are never automatically assigned an owner.

use std::fmt;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The endpoint owning a source reference.
///
/// A named owner retains the exact stored declaration name: no trimming or case
/// folding is performed. Empty names are refused on both read and write. Whether
/// the name is declared in a project belongs to later project admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceOwner {
    Primary,
    Named {
        #[serde(serialize_with = "serialize_named_owner_name")]
        name: String,
    },
}

fn serialize_named_owner_name<S>(name: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if name.is_empty() {
        return Err(serde::ser::Error::custom(
            "named source owner name cannot be empty",
        ));
    }
    serializer.serialize_str(name)
}

struct SourceOwnerVisitor;

impl<'de> Visitor<'de> for SourceOwnerVisitor {
    type Value = SourceOwner;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a primary or named source owner object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut kind = None;
        let mut name = None;
        while let Some(field) = map.next_key::<String>()? {
            match field.as_str() {
                "kind" => {
                    if kind.is_some() {
                        return Err(de::Error::duplicate_field("kind"));
                    }
                    kind = Some(map.next_value::<String>()?);
                }
                "name" => {
                    if name.is_some() {
                        return Err(de::Error::duplicate_field("name"));
                    }
                    name = Some(map.next_value::<String>()?);
                }
                _ => return Err(de::Error::unknown_field(&field, &["kind", "name"])),
            }
        }
        match kind
            .as_deref()
            .ok_or_else(|| de::Error::missing_field("kind"))?
        {
            "primary" => {
                if name.is_some() {
                    return Err(de::Error::unknown_field("name", &["kind"]));
                }
                Ok(SourceOwner::Primary)
            }
            "named" => {
                let name = name.ok_or_else(|| de::Error::missing_field("name"))?;
                if name.is_empty() {
                    return Err(de::Error::custom("named source owner name cannot be empty"));
                }
                Ok(SourceOwner::Named { name })
            }
            kind => Err(de::Error::unknown_variant(kind, &["primary", "named"])),
        }
    }
}

impl<'de> Deserialize<'de> for SourceOwner {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(SourceOwnerVisitor)
    }
}

/// An explicit endpoint owner and its unchanged endpoint-relative path.
///
/// `path` is required, including the empty path selecting the endpoint root.
/// Segments are data, with no new path syntax or length restrictions here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OwnedSourceRef {
    pub owner: SourceOwner,
    pub path: Vec<String>,
}

struct OwnedSourceRefVisitor;

impl<'de> Visitor<'de> for OwnedSourceRefVisitor {
    type Value = OwnedSourceRef;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an owned source object with owner and path fields")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut owner = None;
        let mut path = None;
        while let Some(field) = map.next_key::<String>()? {
            match field.as_str() {
                "owner" => {
                    if owner.is_some() {
                        return Err(de::Error::duplicate_field("owner"));
                    }
                    owner = Some(map.next_value::<SourceOwner>()?);
                }
                "path" => {
                    if path.is_some() {
                        return Err(de::Error::duplicate_field("path"));
                    }
                    path = Some(map.next_value::<Vec<String>>()?);
                }
                _ => return Err(de::Error::unknown_field(&field, &["owner", "path"])),
            }
        }
        Ok(OwnedSourceRef {
            owner: owner.ok_or_else(|| de::Error::missing_field("owner"))?,
            path: path.ok_or_else(|| de::Error::missing_field("path"))?,
        })
    }
}

impl<'de> Deserialize<'de> for OwnedSourceRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(OwnedSourceRefVisitor)
    }
}

/// A legacy path array or an explicitly owned source object.
///
/// The two wire forms stay distinct. Nullability belongs to an outer `Option`,
/// rather than a selector variant or an automatic migration of legacy paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum SourceSelector {
    Legacy(Vec<String>),
    Owned(OwnedSourceRef),
}

struct SourceSelectorVisitor;

impl<'de> Visitor<'de> for SourceSelectorVisitor {
    type Value = SourceSelector;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a legacy string path array or an owned source object")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut path = Vec::new();
        while let Some(segment) = sequence.next_element::<String>()? {
            path.push(segment);
        }
        Ok(SourceSelector::Legacy(path))
    }

    fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        OwnedSourceRef::deserialize(de::value::MapAccessDeserializer::new(map))
            .map(SourceSelector::Owned)
    }
}

impl<'de> Deserialize<'de> for SourceSelector {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(SourceSelectorVisitor)
    }
}
