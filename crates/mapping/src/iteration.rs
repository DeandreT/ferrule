use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    NodeId, OwnedJoinPlan, OwnedSourceAccess, OwnedSourceRef, Scope, SequenceExpr,
    SourceSelectorRef,
};

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

/// Cardinality of one collection source participating in an inner join.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinSourceCardinality {
    #[default]
    Repeating,
    Singleton,
}

/// A join source read from a source collection path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CollectionJoinSource {
    collection: Vec<String>,
    #[serde(skip_serializing_if = "is_repeating_join_source")]
    cardinality: JoinSourceCardinality,
}

impl CollectionJoinSource {
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

/// Plan-local identity of a computed join source, independent of the
/// [`NodeId`] that computes its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ComputedJoinSourceId(u32);

impl ComputedJoinSourceId {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for ComputedJoinSourceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// A join source whose single member is one scalar graph value evaluated in
/// the join's receiving context. It has no collection, so no join read names
/// it and it never correlates with an active frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComputedJoinSource {
    id: ComputedJoinSourceId,
    value: NodeId,
}

impl ComputedJoinSource {
    pub const fn new(id: ComputedJoinSourceId, value: NodeId) -> Self {
        Self { id, value }
    }

    pub const fn id(&self) -> ComputedJoinSourceId {
        self.id
    }

    pub const fn value(&self) -> NodeId {
        self.value
    }
}

/// One source participating in an inner join. Consumers that cannot run a
/// computed source must refuse it rather than treat it as a collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinSource {
    /// Wire form `{"collection": [...]}`, unchanged from collection-only plans.
    Collection(CollectionJoinSource),
    /// Wire form `{"computed": {"id": 1, "value": 12}}`.
    Computed(ComputedJoinSource),
}

impl JoinSource {
    pub fn new(collection: Vec<String>) -> Self {
        Self::Collection(CollectionJoinSource::new(collection))
    }

    pub fn singleton(path: Vec<String>) -> Self {
        Self::Collection(CollectionJoinSource::singleton(path))
    }

    pub const fn computed(id: ComputedJoinSourceId, value: NodeId) -> Self {
        Self::Computed(ComputedJoinSource::new(id, value))
    }

    pub const fn as_collection(&self) -> Option<&CollectionJoinSource> {
        match self {
            Self::Collection(source) => Some(source),
            Self::Computed(_) => None,
        }
    }

    pub const fn as_computed(&self) -> Option<&ComputedJoinSource> {
        match self {
            Self::Computed(source) => Some(source),
            Self::Collection(_) => None,
        }
    }
}

impl From<CollectionJoinSource> for JoinSource {
    fn from(source: CollectionJoinSource) -> Self {
        Self::Collection(source)
    }
}

impl From<ComputedJoinSource> for JoinSource {
    fn from(source: ComputedJoinSource) -> Self {
        Self::Computed(source)
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum JoinSourceRef<'a> {
    Collection(&'a CollectionJoinSource),
    Computed { computed: &'a ComputedJoinSource },
}

impl Serialize for JoinSource {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Collection(source) => JoinSourceRef::Collection(source),
            Self::Computed(computed) => JoinSourceRef::Computed { computed },
        }
        .serialize(serializer)
    }
}

/// Reads a present member as its value, so an explicit `null` is refused
/// rather than read as an absent member.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Collection sources keep their lenient legacy members; the computed form
/// is new and admits no collection members beside it. No member is nullable.
#[derive(Deserialize)]
struct JoinSourceWire {
    #[serde(default, deserialize_with = "present")]
    collection: Option<Vec<String>>,
    #[serde(default, deserialize_with = "present")]
    cardinality: Option<JoinSourceCardinality>,
    #[serde(default, deserialize_with = "present")]
    computed: Option<ComputedJoinSource>,
}

impl<'de> Deserialize<'de> for JoinSource {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = JoinSourceWire::deserialize(deserializer)?;
        match (wire.collection, wire.cardinality, wire.computed) {
            (Some(collection), cardinality, None) => Ok(Self::Collection(CollectionJoinSource {
                collection,
                cardinality: cardinality.unwrap_or_default(),
            })),
            (None, None, Some(computed)) => Ok(Self::Computed(computed)),
            (None, None | Some(_), None) => Err(serde::de::Error::missing_field("collection")),
            (Some(_), _, Some(_)) | (None, Some(_), Some(_)) => Err(serde::de::Error::custom(
                "a computed join source has no `collection` or `cardinality`",
            )),
        }
    }
}

/// The left member of one join condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinKeyLeft {
    /// A collection already in the left tuple and the key path within it.
    Collection {
        collection: Vec<String>,
        path: Vec<String>,
    },
    /// A computed source already in the left tuple; its key is its value.
    Computed(ComputedJoinSourceId),
}

/// One equality condition between a member already in the left tuple and
/// the source introduced by the current join stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinKey {
    left: JoinKeyLeft,
    right_path: Vec<String>,
}

impl JoinKey {
    pub fn new(
        left_collection: Vec<String>,
        left_path: Vec<String>,
        right_path: Vec<String>,
    ) -> Self {
        Self {
            left: JoinKeyLeft::Collection {
                collection: left_collection,
                path: left_path,
            },
            right_path,
        }
    }

    /// A condition whose left member is the computed source `left`.
    pub fn computed(left: ComputedJoinSourceId, right_path: Vec<String>) -> Self {
        Self {
            left: JoinKeyLeft::Computed(left),
            right_path,
        }
    }

    pub const fn left(&self) -> &JoinKeyLeft {
        &self.left
    }

    /// The left collection, or `None` when the left member is computed.
    pub fn left_collection(&self) -> Option<&[String]> {
        match &self.left {
            JoinKeyLeft::Collection { collection, .. } => Some(collection),
            JoinKeyLeft::Computed(_) => None,
        }
    }

    /// The left key path. A computed left member is its own key, so its path
    /// is always empty.
    pub fn left_path(&self) -> &[String] {
        match &self.left {
            JoinKeyLeft::Collection { path, .. } => path,
            JoinKeyLeft::Computed(_) => &[],
        }
    }

    pub fn right_path(&self) -> &[String] {
        &self.right_path
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum JoinKeyRef<'a> {
    Collection {
        left_collection: &'a [String],
        left_path: &'a [String],
        right_path: &'a [String],
    },
    Computed {
        left_computed: ComputedJoinSourceId,
        right_path: &'a [String],
    },
}

impl Serialize for JoinKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let right_path = self.right_path.as_slice();
        match &self.left {
            JoinKeyLeft::Collection { collection, path } => JoinKeyRef::Collection {
                left_collection: collection,
                left_path: path,
                right_path,
            },
            JoinKeyLeft::Computed(left_computed) => JoinKeyRef::Computed {
                left_computed: *left_computed,
                right_path,
            },
        }
        .serialize(serializer)
    }
}

/// No member is nullable, so a `null` beside either form is refused.
#[derive(Deserialize)]
struct JoinKeyWire {
    #[serde(default, deserialize_with = "present")]
    left_collection: Option<Vec<String>>,
    #[serde(default, deserialize_with = "present")]
    left_path: Option<Vec<String>>,
    #[serde(default, deserialize_with = "present")]
    left_computed: Option<ComputedJoinSourceId>,
    right_path: Vec<String>,
}

impl<'de> Deserialize<'de> for JoinKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = JoinKeyWire::deserialize(deserializer)?;
        let left = match (wire.left_collection, wire.left_path, wire.left_computed) {
            (Some(collection), Some(path), None) => JoinKeyLeft::Collection { collection, path },
            (None, None, Some(id)) => JoinKeyLeft::Computed(id),
            (Some(_), None, None) => return Err(serde::de::Error::missing_field("left_path")),
            (None, _, None) => return Err(serde::de::Error::missing_field("left_collection")),
            (Some(_), _, Some(_)) | (None, Some(_), Some(_)) => {
                return Err(serde::de::Error::custom(
                    "a computed join key has no `left_collection` or `left_path`",
                ));
            }
        };
        Ok(Self {
            left,
            right_path: wire.right_path,
        })
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
/// At most one source is computed, and at least one reads a collection.
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
    /// A computed source reuses the id of an earlier one.
    DuplicateComputedSource(ComputedJoinSourceId),
    /// A plan already holds its one computed source.
    MultipleComputedSources(ComputedJoinSourceId),
    /// A condition references a computed source not yet joined.
    UnknownLeftComputed(ComputedJoinSourceId),
    /// A computed right source is its own key, so its conditions take an
    /// empty right path.
    ComputedRightPath {
        source: ComputedJoinSourceId,
        right_path: Vec<String>,
    },
    /// Every source of the plan is computed.
    NoCollectionSource,
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
            Self::DuplicateComputedSource(id) => {
                write!(
                    formatter,
                    "computed join source {id} is used more than once"
                )
            }
            Self::MultipleComputedSources(id) => write!(
                formatter,
                "computed join source {id} is a second computed source; a join holds at most one"
            ),
            Self::UnknownLeftComputed(id) => write!(
                formatter,
                "join condition references computed source {id} before it is joined"
            ),
            Self::ComputedRightPath { source, right_path } => write!(
                formatter,
                "join condition on computed source {source} has right path `{}`; a computed source is its own key",
                right_path.join("/")
            ),
            Self::NoCollectionSource => formatter.write_str("join plan has no collection source"),
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
        validate_stage(&[&first], &second, &conditions)?;
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
        validate_stage(&self.sources().collect::<Vec<_>>(), &source, &conditions)?;
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

    /// The plan's collection sources in plan order, without its computed
    /// source. Analyses of source paths read these; consumers that execute or
    /// rewrite a plan must refuse a computed source instead.
    pub fn collection_sources(&self) -> impl Iterator<Item = &CollectionJoinSource> {
        self.sources().filter_map(JoinSource::as_collection)
    }

    /// The plan's computed source, if it has one.
    pub fn computed_source(&self) -> Option<&ComputedJoinSource> {
        self.sources().find_map(JoinSource::as_computed)
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

/// Checks one stage joining `source` to the already joined `prior` sources:
/// source identity, then the right paths of a computed source, then that
/// every left member is already joined.
fn validate_stage(
    prior: &[&JoinSource],
    source: &JoinSource,
    conditions: &JoinConditions,
) -> Result<(), JoinPlanError> {
    match source {
        JoinSource::Collection(source) => {
            if prior
                .iter()
                .filter_map(|source| source.as_collection())
                .any(|existing| existing.collection == source.collection)
            {
                return Err(JoinPlanError::DuplicateCollection(
                    source.collection.clone(),
                ));
            }
        }
        JoinSource::Computed(computed) => {
            let mut earlier = prior.iter().filter_map(|source| source.as_computed());
            if earlier.clone().any(|existing| existing.id == computed.id) {
                return Err(JoinPlanError::DuplicateComputedSource(computed.id));
            }
            if prior
                .iter()
                .all(|existing| existing.as_collection().is_none())
            {
                return Err(JoinPlanError::NoCollectionSource);
            }
            if earlier.next().is_some() {
                return Err(JoinPlanError::MultipleComputedSources(computed.id));
            }
            if let Some(condition) = conditions
                .iter()
                .find(|condition| !condition.right_path.is_empty())
            {
                return Err(JoinPlanError::ComputedRightPath {
                    source: computed.id,
                    right_path: condition.right_path.clone(),
                });
            }
        }
    }
    for condition in conditions.iter() {
        match &condition.left {
            JoinKeyLeft::Collection { collection, .. } => {
                if !prior
                    .iter()
                    .filter_map(|source| source.as_collection())
                    .any(|source| source.collection == *collection)
                {
                    return Err(JoinPlanError::UnknownLeftCollection(collection.clone()));
                }
            }
            JoinKeyLeft::Computed(id) => {
                if !prior
                    .iter()
                    .filter_map(|source| source.as_computed())
                    .any(|source| source.id == *id)
                {
                    return Err(JoinPlanError::UnknownLeftComputed(*id));
                }
            }
        }
    }
    Ok(())
}

/// The mutually-exclusive way a target scope obtains iteration items.
/// A non-empty ordered composition of independently evaluated target scopes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ScopeIteration {
    #[default]
    None,
    Source(Vec<String>),
    /// Explicit endpoint-relative collection identity; execution remains unavailable.
    OwnedSource {
        source: OwnedSourceRef,
        output_path: Option<NodeId>,
    },
    /// Produces one independently named document per source item.
    DynamicDocuments {
        source: Vec<String>,
        output_path: NodeId,
    },
    Sequence(SequenceExpr),
    InnerJoin {
        id: JoinId,
        plan: JoinPlan,
    },
    /// Inner join over explicit owner and collection identities.
    OwnedInnerJoin {
        id: JoinId,
        plan: OwnedJoinPlan,
    },
    Concatenate(ScopeSequence),
    /// Evaluates one Boolean presence expression once in the unchanged parent
    /// context and constructs zero or one group. It adds no iteration frame,
    /// so expressions keep resolving the parent item and its positions.
    OptionalPresence {
        condition: NodeId,
    },
}

impl ScopeIteration {
    pub fn source_selector(&self) -> Option<SourceSelectorRef<'_>> {
        match self {
            Self::Source(path) | Self::DynamicDocuments { source: path, .. } => {
                Some(SourceSelectorRef::Legacy(path))
            }
            Self::OwnedSource { source, .. } => Some(SourceSelectorRef::Owned(source)),
            Self::None
            | Self::Sequence(_)
            | Self::InnerJoin { .. }
            | Self::OwnedInnerJoin { .. }
            | Self::Concatenate(_)
            | Self::OptionalPresence { .. } => None,
        }
    }

    /// Legacy path access refuses an owned selector instead of dropping its owner.
    pub fn source(&self) -> Result<Option<&[String]>, OwnedSourceAccess> {
        match self.source_selector() {
            Some(SourceSelectorRef::Legacy(path)) => Ok(Some(path)),
            Some(SourceSelectorRef::Owned(_)) => Err(OwnedSourceAccess),
            None => Ok(None),
        }
    }

    pub fn sequence(&self) -> Option<&SequenceExpr> {
        match self {
            Self::Sequence(sequence) => Some(sequence),
            Self::None
            | Self::Source(_)
            | Self::OwnedSource { .. }
            | Self::DynamicDocuments { .. }
            | Self::InnerJoin { .. }
            | Self::OwnedInnerJoin { .. }
            | Self::Concatenate(_)
            | Self::OptionalPresence { .. } => None,
        }
    }

    pub fn join(&self) -> Option<(JoinId, &JoinPlan)> {
        match self {
            Self::InnerJoin { id, plan } => Some((*id, plan)),
            Self::None
            | Self::Source(_)
            | Self::OwnedSource { .. }
            | Self::DynamicDocuments { .. }
            | Self::Sequence(_)
            | Self::OwnedInnerJoin { .. }
            | Self::Concatenate(_)
            | Self::OptionalPresence { .. } => None,
        }
    }

    pub fn owned_join(&self) -> Option<(JoinId, &OwnedJoinPlan)> {
        match self {
            Self::OwnedInnerJoin { id, plan } => Some((*id, plan)),
            Self::None
            | Self::Source(_)
            | Self::OwnedSource { .. }
            | Self::DynamicDocuments { .. }
            | Self::Sequence(_)
            | Self::InnerJoin { .. }
            | Self::Concatenate(_)
            | Self::OptionalPresence { .. } => None,
        }
    }

    pub fn concatenated(&self) -> Option<&ScopeSequence> {
        match self {
            Self::Concatenate(sequence) => Some(sequence),
            Self::None
            | Self::Source(_)
            | Self::OwnedSource { .. }
            | Self::DynamicDocuments { .. }
            | Self::Sequence(_)
            | Self::InnerJoin { .. }
            | Self::OwnedInnerJoin { .. }
            | Self::OptionalPresence { .. } => None,
        }
    }

    pub fn concatenated_mut(&mut self) -> Option<&mut ScopeSequence> {
        match self {
            Self::Concatenate(sequence) => Some(sequence),
            Self::None
            | Self::Source(_)
            | Self::OwnedSource { .. }
            | Self::DynamicDocuments { .. }
            | Self::Sequence(_)
            | Self::InnerJoin { .. }
            | Self::OwnedInnerJoin { .. }
            | Self::OptionalPresence { .. } => None,
        }
    }

    /// The Boolean presence expression of an optional-presence scope.
    pub fn presence(&self) -> Option<NodeId> {
        match self {
            Self::OptionalPresence { condition } => Some(*condition),
            Self::None
            | Self::Source(_)
            | Self::OwnedSource { .. }
            | Self::DynamicDocuments { .. }
            | Self::Sequence(_)
            | Self::InnerJoin { .. }
            | Self::OwnedInnerJoin { .. }
            | Self::Concatenate(_) => None,
        }
    }

    pub fn iterates(&self) -> bool {
        !matches!(self, Self::None)
    }
}
