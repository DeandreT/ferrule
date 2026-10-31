use std::fmt;

pub use mapping::ComputedJoinSourceId;
use mapping::NodeId;

/// Stable identity of one inner-join iteration in a generated program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JoinId(u64);

impl JoinId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<mapping::JoinId> for JoinId {
    fn from(value: mapping::JoinId) -> Self {
        Self(value.get())
    }
}

/// Cardinality of one source participating in an inner join.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum JoinSourceCardinality {
    #[default]
    Repeating,
    Singleton,
}

impl From<mapping::JoinSourceCardinality> for JoinSourceCardinality {
    fn from(value: mapping::JoinSourceCardinality) -> Self {
        match value {
            mapping::JoinSourceCardinality::Repeating => Self::Repeating,
            mapping::JoinSourceCardinality::Singleton => Self::Singleton,
        }
    }
}

/// One statically resolved collection source in an inner-join plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionJoinSource {
    collection: Vec<String>,
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

impl From<&mapping::CollectionJoinSource> for CollectionJoinSource {
    fn from(value: &mapping::CollectionJoinSource) -> Self {
        Self {
            collection: value.collection().to_vec(),
            cardinality: value.cardinality().into(),
        }
    }
}

/// A single-valued computed join source: one scalar expression evaluated
/// once per join execution in the join's receiving context. Generated code
/// has no sequence-valued form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// One source of an inner-join plan. A computed source's single member is
/// its key value; it has no collection, so no join read names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinSource {
    Collection(CollectionJoinSource),
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

impl JoinSource {
    /// `None` for a sequence-valued computed source.
    fn from_mapping(value: &mapping::JoinSource) -> Option<Self> {
        Some(match value {
            mapping::JoinSource::Collection(source) => Self::Collection(source.into()),
            mapping::JoinSource::Computed(source) => Self::computed(source.id(), source.scalar()?),
        })
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
    /// The computed source already in the left tuple; its key is its value.
    Computed(ComputedJoinSourceId),
}

/// One equality key joining the new right source to an existing left member.
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

    /// A key whose left member is the computed source `left`.
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

    /// The left key path; a computed left member is its own key, so its path
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

impl From<&mapping::JoinKey> for JoinKey {
    fn from(value: &mapping::JoinKey) -> Self {
        let left = match value.left() {
            mapping::JoinKeyLeft::Collection { collection, path } => JoinKeyLeft::Collection {
                collection: collection.clone(),
                path: path.clone(),
            },
            mapping::JoinKeyLeft::Computed(id) => JoinKeyLeft::Computed(*id),
        };
        Self {
            left,
            right_path: value.right_path().to_vec(),
        }
    }
}

/// A nonempty composite equality condition for one left-deep join stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinConditions {
    first: JoinKey,
    rest: Vec<JoinKey>,
}

impl JoinConditions {
    pub fn new(first: JoinKey) -> Self {
        Self {
            first,
            rest: Vec::new(),
        }
    }

    pub fn and(mut self, key: JoinKey) -> Self {
        self.rest.push(key);
        self
    }

    pub fn iter(&self) -> impl Iterator<Item = &JoinKey> {
        std::iter::once(&self.first).chain(&self.rest)
    }
}

impl From<&mapping::JoinConditions> for JoinConditions {
    fn from(value: &mapping::JoinConditions) -> Self {
        let mut keys = value.iter();
        let Some(first) = keys.next() else {
            unreachable!("mapping join conditions are nonempty by construction");
        };
        Self {
            first: first.into(),
            rest: keys.map(JoinKey::from).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JoinStage {
    source: JoinSource,
    conditions: JoinConditions,
}

/// A left-deep join with at least two distinct sources and one key per stage.
/// At most one source is computed, and at least one reads a collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinPlan {
    first: JoinSource,
    second: JoinStage,
    rest: Vec<JoinStage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinPlanError {
    DuplicateCollection(Vec<String>),
    UnknownLeftCollection(Vec<String>),
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

impl JoinPlan {
    pub fn new(
        first: JoinSource,
        second: JoinSource,
        conditions: JoinConditions,
    ) -> Result<Self, JoinPlanError> {
        validate_stage(&[&first], &second, &conditions)?;
        Ok(Self {
            first,
            second: JoinStage {
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
        self.rest.push(JoinStage { source, conditions });
        Ok(self)
    }

    pub fn sources(&self) -> impl Iterator<Item = &JoinSource> {
        std::iter::once(&self.first)
            .chain(std::iter::once(&self.second.source))
            .chain(self.rest.iter().map(|stage| &stage.source))
    }

    pub fn stages(&self) -> impl Iterator<Item = (&JoinSource, &JoinConditions)> {
        std::iter::once((&self.second.source, &self.second.conditions)).chain(
            self.rest
                .iter()
                .map(|stage| (&stage.source, &stage.conditions)),
        )
    }

    /// The plan's collection sources in plan order, without its computed
    /// source.
    pub fn collection_sources(&self) -> impl Iterator<Item = &CollectionJoinSource> {
        self.sources().filter_map(JoinSource::as_collection)
    }

    /// The plan's computed source, if it has one.
    pub fn computed_source(&self) -> Option<&ComputedJoinSource> {
        self.sources().find_map(JoinSource::as_computed)
    }

    /// Mirrors a plan that mapping construction already validated, or
    /// `None` when it holds a sequence-valued computed source, which
    /// generated code does not execute.
    pub(crate) fn from_mapping(value: &mapping::JoinPlan) -> Option<Self> {
        let mut sources = value.sources();
        let Some(first) = sources.next() else {
            unreachable!("mapping join plans contain a first source");
        };
        let mut stages = value.stages().map(|(source, conditions)| {
            Some(JoinStage {
                source: JoinSource::from_mapping(source)?,
                conditions: conditions.into(),
            })
        });
        let Some(second) = stages.next() else {
            unreachable!("mapping join plans contain a second source");
        };
        Some(Self {
            first: JoinSource::from_mapping(first)?,
            second: second?,
            rest: stages.collect::<Option<_>>()?,
        })
    }
}

/// Checks one stage joining `source` to the already joined `prior` sources,
/// with the same rules as mapping plan construction.
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
            if prior
                .iter()
                .all(|existing| existing.as_collection().is_none())
            {
                return Err(JoinPlanError::NoCollectionSource);
            }
            if prior
                .iter()
                .any(|existing| existing.as_computed().is_some())
            {
                return Err(JoinPlanError::MultipleComputedSources(computed.id()));
            }
            if let Some(condition) = conditions
                .iter()
                .find(|condition| !condition.right_path.is_empty())
            {
                return Err(JoinPlanError::ComputedRightPath {
                    source: computed.id(),
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
                    .any(|source| source.id() == *id)
                {
                    return Err(JoinPlanError::UnknownLeftComputed(*id));
                }
            }
        }
    }
    Ok(())
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

/// One owned inner-join candidate source for a target scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InnerJoin {
    id: JoinId,
    plan: JoinPlan,
}

impl InnerJoin {
    pub fn new(id: JoinId, plan: JoinPlan) -> Self {
        Self { id, plan }
    }

    pub const fn id(&self) -> JoinId {
        self.id
    }

    pub const fn plan(&self) -> &JoinPlan {
        &self.plan
    }
}

/// One owned inner-join candidate source for a target scope. Its sources and
/// left keys keep full owner and collection identity; the JoinId stays
/// independent of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedInnerJoin {
    id: JoinId,
    plan: mapping::OwnedJoinPlan,
}

impl OwnedInnerJoin {
    pub fn new(id: JoinId, plan: mapping::OwnedJoinPlan) -> Self {
        Self { id, plan }
    }

    pub const fn id(&self) -> JoinId {
        self.id
    }

    pub const fn plan(&self) -> &mapping::OwnedJoinPlan {
        &self.plan
    }
}
