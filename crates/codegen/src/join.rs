use std::fmt;

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

/// One statically resolved source in an inner-join plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinSource {
    collection: Vec<String>,
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

impl From<&mapping::JoinSource> for JoinSource {
    fn from(value: &mapping::JoinSource) -> Self {
        Self {
            collection: value.collection().to_vec(),
            cardinality: value.cardinality().into(),
        }
    }
}

/// One equality key joining the new right source to an existing left source.
#[derive(Debug, Clone, PartialEq, Eq)]
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

impl From<&mapping::JoinKey> for JoinKey {
    fn from(value: &mapping::JoinKey) -> Self {
        Self::new(
            value.left_collection().to_vec(),
            value.left_path().to_vec(),
            value.right_path().to_vec(),
        )
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

    fn from_mapping(value: &mapping::JoinConditions) -> Self {
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
}

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
        if self
            .sources()
            .any(|existing| existing.collection == source.collection)
        {
            return Err(JoinPlanError::DuplicateCollection(source.collection));
        }
        validate_left_collections(&conditions, self.sources())?;
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

    pub(crate) fn from_mapping(value: &mapping::JoinPlan) -> Self {
        let mut sources = value.sources();
        let Some(first) = sources.next() else {
            unreachable!("mapping join plans contain a first source");
        };
        let mut stages = value.stages();
        let Some((second, conditions)) = stages.next() else {
            unreachable!("mapping join plans contain a second source");
        };
        let mut plan = Self {
            first: first.into(),
            second: JoinStage {
                source: second.into(),
                conditions: JoinConditions::from_mapping(conditions),
            },
            rest: Vec::new(),
        };
        plan.rest
            .extend(stages.map(|(source, conditions)| JoinStage {
                source: source.into(),
                conditions: JoinConditions::from_mapping(conditions),
            }));
        plan
    }
}

fn validate_left_collections<'a>(
    conditions: &JoinConditions,
    sources: impl IntoIterator<Item = &'a JoinSource>,
) -> Result<(), JoinPlanError> {
    let collections = sources
        .into_iter()
        .map(|source| source.collection.as_slice())
        .collect::<Vec<_>>();
    for key in conditions.iter() {
        if !collections.contains(&key.left_collection.as_slice()) {
            return Err(JoinPlanError::UnknownLeftCollection(
                key.left_collection.clone(),
            ));
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
