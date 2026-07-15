use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{Scope, SequenceExpr};

/// Stable identity of one joined iteration within a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JoinId(u64);

impl JoinId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Cardinality of one source participating in an inner join.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinSourceCardinality {
    #[default]
    Repeating,
    Singleton,
}

/// One source participating in an inner join.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinSource {
    collection: Vec<String>,
    #[serde(default, skip_serializing_if = "is_repeating_join_source")]
    cardinality: JoinSourceCardinality,
}

impl JoinSource {
    pub fn new(collection: Vec<String>) -> Self {
        Self {
            collection,
            cardinality: JoinSourceCardinality::Repeating,
        }
    }

    pub fn singleton(path: Vec<String>) -> Self {
        Self {
            collection: path,
            cardinality: JoinSourceCardinality::Singleton,
        }
    }

    pub fn collection(&self) -> &[String] {
        &self.collection
    }

    pub const fn cardinality(&self) -> JoinSourceCardinality {
        self.cardinality
    }
}

fn is_repeating_join_source(cardinality: &JoinSourceCardinality) -> bool {
    *cardinality == JoinSourceCardinality::Repeating
}

/// One equality condition between a collection already in the left tuple
/// and the collection introduced by the current join stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinKey {
    left_collection: Vec<String>,
    left_path: Vec<String>,
    right_path: Vec<String>,
}

impl JoinKey {
    pub fn new(
        left_collection: Vec<String>,
        left_path: Vec<String>,
        right_path: Vec<String>,
    ) -> Self {
        Self {
            left_collection,
            left_path,
            right_path,
        }
    }

    pub fn left_collection(&self) -> &[String] {
        &self.left_collection
    }

    pub fn left_path(&self) -> &[String] {
        &self.left_path
    }

    pub fn right_path(&self) -> &[String] {
        &self.right_path
    }
}

/// Non-empty set of equality conditions for one join stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinConditions {
    first: JoinKey,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    rest: Vec<JoinKey>,
}

impl JoinConditions {
    pub fn new(first: JoinKey) -> Self {
        Self {
            first,
            rest: Vec::new(),
        }
    }

    pub fn and(mut self, condition: JoinKey) -> Self {
        self.rest.push(condition);
        self
    }

    pub fn iter(&self) -> impl Iterator<Item = &JoinKey> {
        std::iter::once(&self.first).chain(&self.rest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct JoinStep {
    source: JoinSource,
    conditions: JoinConditions,
}

/// Left-deep inner-join plan. Its private `second` field makes every plan
/// contain at least two inputs, while each stage owns non-empty conditions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JoinPlan {
    first: JoinSource,
    second: JoinStep,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    rest: Vec<JoinStep>,
}

#[derive(Deserialize)]
struct JoinPlanWire {
    first: JoinSource,
    second: JoinStep,
    #[serde(default)]
    rest: Vec<JoinStep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinPlanError {
    DuplicateCollection(Vec<String>),
    UnknownLeftCollection(Vec<String>),
}

impl fmt::Display for JoinPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateCollection(collection) => write!(
                formatter,
                "join collection `{}` is used more than once",
                collection.join("/")
            ),
            Self::UnknownLeftCollection(collection) => write!(
                formatter,
                "join condition references collection `{}` before it is joined",
                collection.join("/")
            ),
        }
    }
}

impl std::error::Error for JoinPlanError {}

impl JoinPlan {
    pub fn new(
        first: JoinSource,
        second: JoinSource,
        conditions: JoinConditions,
    ) -> Result<Self, JoinPlanError> {
        if first.collection == second.collection {
            return Err(JoinPlanError::DuplicateCollection(second.collection));
        }
        validate_left_collections(&conditions, std::slice::from_ref(&first))?;
        Ok(Self {
            first,
            second: JoinStep {
                source: second,
                conditions,
            },
            rest: Vec::new(),
        })
    }

    pub fn then(
        mut self,
        source: JoinSource,
        conditions: JoinConditions,
    ) -> Result<Self, JoinPlanError> {
        if self
            .sources()
            .any(|existing| existing.collection == source.collection)
        {
            return Err(JoinPlanError::DuplicateCollection(source.collection));
        }
        validate_left_collections(&conditions, self.sources())?;
        self.rest.push(JoinStep { source, conditions });
        Ok(self)
    }

    pub fn sources(&self) -> impl Iterator<Item = &JoinSource> {
        std::iter::once(&self.first)
            .chain(std::iter::once(&self.second.source))
            .chain(self.rest.iter().map(|step| &step.source))
    }

    pub fn stages(&self) -> impl Iterator<Item = (&JoinSource, &JoinConditions)> {
        std::iter::once((&self.second.source, &self.second.conditions)).chain(
            self.rest
                .iter()
                .map(|step| (&step.source, &step.conditions)),
        )
    }
}

impl<'de> Deserialize<'de> for JoinPlan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = JoinPlanWire::deserialize(deserializer)?;
        let mut plan = Self::new(wire.first, wire.second.source, wire.second.conditions)
            .map_err(serde::de::Error::custom)?;
        for step in wire.rest {
            plan = plan
                .then(step.source, step.conditions)
                .map_err(serde::de::Error::custom)?;
        }
        Ok(plan)
    }
}

fn validate_left_collections<'a>(
    conditions: &JoinConditions,
    sources: impl IntoIterator<Item = &'a JoinSource>,
) -> Result<(), JoinPlanError> {
    let collections: Vec<_> = sources
        .into_iter()
        .map(|source| source.collection.as_slice())
        .collect();
    for condition in conditions.iter() {
        if !collections.contains(&condition.left_collection.as_slice()) {
            return Err(JoinPlanError::UnknownLeftCollection(
                condition.left_collection.clone(),
            ));
        }
    }
    Ok(())
}

/// The mutually-exclusive way a target scope obtains iteration items.
/// A non-empty ordered composition of independently evaluated target scopes.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct ScopeSequence {
    segments: Vec<Scope>,
}

impl ScopeSequence {
    pub fn new(first: Scope, rest: Vec<Scope>) -> Self {
        let mut segments = Vec::with_capacity(rest.len() + 1);
        segments.push(first);
        segments.extend(rest);
        Self { segments }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Scope> {
        self.segments.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Scope> {
        self.segments.iter_mut()
    }

    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// A scope sequence is non-empty by construction and deserialization.
    pub const fn is_empty(&self) -> bool {
        false
    }
}

impl<'de> Deserialize<'de> for ScopeSequence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let segments = Vec::<Scope>::deserialize(deserializer)?;
        if segments.is_empty() {
            return Err(serde::de::Error::custom(
                "scope sequence must contain at least one segment",
            ));
        }
        Ok(Self { segments })
    }
}

#[derive(Debug, Clone, Default)]
pub enum ScopeIteration {
    #[default]
    None,
    Source(Vec<String>),
    Sequence(SequenceExpr),
    InnerJoin {
        id: JoinId,
        plan: JoinPlan,
    },
    Concatenate(ScopeSequence),
}

impl ScopeIteration {
    pub fn source(&self) -> Option<&[String]> {
        match self {
            Self::Source(path) => Some(path),
            Self::None | Self::Sequence(_) | Self::InnerJoin { .. } | Self::Concatenate(_) => None,
        }
    }

    pub fn sequence(&self) -> Option<&SequenceExpr> {
        match self {
            Self::Sequence(sequence) => Some(sequence),
            Self::None | Self::Source(_) | Self::InnerJoin { .. } | Self::Concatenate(_) => None,
        }
    }

    pub fn join(&self) -> Option<(JoinId, &JoinPlan)> {
        match self {
            Self::InnerJoin { id, plan } => Some((*id, plan)),
            Self::None | Self::Source(_) | Self::Sequence(_) | Self::Concatenate(_) => None,
        }
    }

    pub fn concatenated(&self) -> Option<&ScopeSequence> {
        match self {
            Self::Concatenate(sequence) => Some(sequence),
            Self::None | Self::Source(_) | Self::Sequence(_) | Self::InnerJoin { .. } => None,
        }
    }

    pub fn concatenated_mut(&mut self) -> Option<&mut ScopeSequence> {
        match self {
            Self::Concatenate(sequence) => Some(sequence),
            Self::None | Self::Source(_) | Self::Sequence(_) | Self::InnerJoin { .. } => None,
        }
    }

    pub fn iterates(&self) -> bool {
        !matches!(self, Self::None)
    }
}
