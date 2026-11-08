//! Serialized shapes that serde's derives cannot express directly: tuple
//! variants written as `kind`-tagged objects, a renamed iteration member,
//! and owner names written without `SourceOwner`'s project-file refusal of
//! empty names.

use mapping::{JoinId, SourceOwner};
use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};

use super::{TraceIteration, TraceTarget, TraceWindow};

pub(super) fn join_position<S: Serializer>(
    position: &Option<(JoinId, usize)>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct JoinPosition {
        join: JoinId,
        index: usize,
    }
    position
        .map(|(join, index)| JoinPosition { join, index })
        .serialize(serializer)
}

/// `{"kind": "primary"}` or `{"kind": "named", "name": ...}`.
fn named<S: Serializer>(name: Option<&str>, serializer: S) -> Result<S::Ok, S::Error> {
    match name {
        None => {
            let mut object = serializer.serialize_struct("Owner", 1)?;
            object.serialize_field("kind", "primary")?;
            object.end()
        }
        Some(name) => {
            let mut object = serializer.serialize_struct("Owner", 2)?;
            object.serialize_field("kind", "named")?;
            object.serialize_field("name", name)?;
            object.end()
        }
    }
}

struct Owner<'a>(&'a SourceOwner);

impl Serialize for Owner<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            SourceOwner::Primary => named(None, serializer),
            SourceOwner::Named { name } => named(Some(name), serializer),
        }
    }
}

impl Serialize for TraceTarget {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Primary => named(None, serializer),
            Self::Named(name) => named(Some(name), serializer),
        }
    }
}

impl Serialize for TraceIteration {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut object = serializer.serialize_struct("TraceIteration", 3)?;
        match self {
            Self::Once => object.serialize_field("kind", "once")?,
            Self::Source { path } => {
                object.serialize_field("kind", "source")?;
                object.serialize_field("path", path)?;
            }
            Self::OwnedSource { source } => {
                object.serialize_field("kind", "owned_source")?;
                object.serialize_field("owner", &Owner(&source.owner))?;
                object.serialize_field("path", &source.path)?;
            }
            Self::DynamicDocuments { source } => {
                object.serialize_field("kind", "dynamic_documents")?;
                object.serialize_field("source", source)?;
            }
            Self::Generated { kind } => {
                object.serialize_field("kind", "generated")?;
                object.serialize_field("sequence", kind)?;
            }
            Self::Join { join } => {
                object.serialize_field("kind", "join")?;
                object.serialize_field("join", join)?;
            }
            Self::Concatenate { segments } => {
                object.serialize_field("kind", "concatenate")?;
                object.serialize_field("segments", segments)?;
            }
            Self::OptionalPresence { condition } => {
                object.serialize_field("kind", "optional_presence")?;
                object.serialize_field("condition", condition)?;
            }
        }
        object.end()
    }
}

impl Serialize for TraceWindow {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut object = serializer.serialize_struct("TraceWindow", 3)?;
        match *self {
            Self::SkipFirst(count) => {
                object.serialize_field("kind", "skip_first")?;
                object.serialize_field("count", &count)?;
            }
            Self::First(count) => {
                object.serialize_field("kind", "first")?;
                object.serialize_field("count", &count)?;
            }
            Self::From(first) => {
                object.serialize_field("kind", "from")?;
                object.serialize_field("first", &first)?;
            }
            Self::FromTo { first, last } => {
                object.serialize_field("kind", "from_to")?;
                object.serialize_field("first", &first)?;
                object.serialize_field("last", &last)?;
            }
            Self::Last(count) => {
                object.serialize_field("kind", "last")?;
                object.serialize_field("count", &count)?;
            }
        }
        object.end()
    }
}
