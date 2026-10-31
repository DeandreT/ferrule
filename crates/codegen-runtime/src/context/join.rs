use crate::{RuntimeError, Value};

use super::{CollectionIdentity, ScopeContext, ScopeFrame};

/// One equality condition between an earlier tuple source and the source
/// introduced by an inner-join stage.
#[derive(Clone, Copy, Debug)]
pub struct InnerJoinKey<'a> {
    pub left_collection: &'a [&'a str],
    pub left_path: &'a [&'a str],
    pub right_path: &'a [&'a str],
}

/// One source after the first source in a left-deep inner join.
#[derive(Clone, Copy, Debug)]
pub struct InnerJoinStage<'a> {
    pub collection: &'a [&'a str],
    pub keys: &'a [InnerJoinKey<'a>],
}

/// One source of a left-deep inner join that holds a computed key source.
#[derive(Clone, Copy, Debug)]
pub enum InnerJoinSource<'a> {
    Collection(&'a [&'a str]),
    /// The computed source, whose single member is the join's key value.
    Computed,
}

/// The left member of one condition of a computed-key inner join.
#[derive(Clone, Copy, Debug)]
pub enum InnerJoinLeft<'a> {
    Collection {
        collection: &'a [&'a str],
        path: &'a [&'a str],
    },
    /// The computed source, which is its own key.
    Computed,
}

/// One equality condition of a computed-key inner join. A computed right
/// source is its own key, so its conditions have an empty right path.
#[derive(Clone, Copy, Debug)]
pub struct InnerJoinCondition<'a> {
    pub left: InnerJoinLeft<'a>,
    pub right_path: &'a [&'a str],
}

/// One source after the first source in a computed-key inner join.
#[derive(Clone, Copy, Debug)]
pub struct InnerJoinSourceStage<'a> {
    pub source: InnerJoinSource<'a>,
    pub conditions: &'a [InnerJoinCondition<'a>],
}

impl<'a> ScopeContext<'a> {
    /// Executes a context-relative left-deep inner join.
    ///
    /// `second` is separate from `rest`, so callers must supply at least two
    /// sources. Source order and duplicate multiplicity are retained.
    pub fn inner_join(
        &self,
        join: u64,
        first: &[&str],
        second: InnerJoinStage<'_>,
        rest: &[InnerJoinStage<'_>],
    ) -> Result<Vec<Self>, RuntimeError> {
        let stages = std::iter::once(second)
            .chain(rest.iter().copied())
            .map(|stage| {
                let conditions = stage
                    .keys
                    .iter()
                    .map(|key| InnerJoinCondition {
                        left: InnerJoinLeft::Collection {
                            collection: key.left_collection,
                            path: key.left_path,
                        },
                        right_path: key.right_path,
                    })
                    .collect::<Vec<_>>();
                (InnerJoinSource::Collection(stage.collection), conditions)
            })
            .collect::<Vec<_>>();
        self.execute_inner_join(join, None, InnerJoinSource::Collection(first), &stages)
    }

    /// Executes a context-relative left-deep inner join holding one computed
    /// source whose value `key` the caller evaluated once in this receiving
    /// context, before this call. A Null-like key gives the computed source
    /// no member, so the join yields no tuples; any other value, `""`
    /// included, is one member that takes its place in plan order and adds
    /// no frame to its tuples.
    pub fn inner_join_with_computed_key(
        &self,
        join: u64,
        key: &Value,
        first: InnerJoinSource<'_>,
        second: InnerJoinSourceStage<'_>,
        rest: &[InnerJoinSourceStage<'_>],
    ) -> Result<Vec<Self>, RuntimeError> {
        let stages = std::iter::once(second)
            .chain(rest.iter().copied())
            .map(|stage| (stage.source, stage.conditions.to_vec()))
            .collect::<Vec<_>>();
        self.execute_inner_join(join, Some(key), first, &stages)
    }

    fn execute_inner_join(
        &self,
        join: u64,
        key: Option<&Value>,
        first: InnerJoinSource<'_>,
        stages: &[(InnerJoinSource<'_>, Vec<InnerJoinCondition<'_>>)],
    ) -> Result<Vec<Self>, RuntimeError> {
        let computed = key.filter(|value| !is_null_like(value));
        let mut rows = self.join_member_rows(join, first, computed);
        for (source, conditions) in stages {
            let right_rows = self.join_member_rows(join, *source, computed);
            let mut joined = Vec::new();
            for left in rows {
                for right in &right_rows {
                    let right = match source {
                        InnerJoinSource::Collection(_) => RightMember::Row(right),
                        InnerJoinSource::Computed => RightMember::Computed,
                    };
                    if join_conditions_match(&left, right, computed, conditions)? {
                        let mut frames = left.frames.clone();
                        if let RightMember::Row(right) = right {
                            frames.extend(right.frames.iter().cloned());
                        }
                        joined.push(JoinRow { frames });
                    }
                }
            }
            rows = joined;
        }

        Ok(rows
            .into_iter()
            .enumerate()
            .map(|(index, mut row)| {
                if let Some(frame) = row.frames.last_mut() {
                    frame.join_position = Some((join, index + 1));
                }
                let mut frames = self.frames.clone();
                frames.extend(row.frames);
                Self {
                    primary_source: self.primary_source,
                    frames,
                    named_inputs: self.named_inputs,
                    execution: self.execution,
                    dynamic_source_loader: self.dynamic_source_loader,
                    filter_map: self.filter_map.clone(),
                    invocation_counters: self.invocation_counters.clone(),
                }
            })
            .collect())
    }

    /// Resolves a scalar from exactly one source frame owned by `join`.
    pub fn resolve_join_scalar(
        &self,
        join: u64,
        collection: &[&str],
        path: &[&str],
    ) -> Result<Value, crate::SourcePathError> {
        self.frames
            .iter()
            .rev()
            .find(|frame| {
                frame.join == Some(join)
                    && frame
                        .collection
                        .as_ref()
                        .is_some_and(|identity| same_collection(identity.path(), collection))
            })
            .and_then(|frame| direct_scalar(frame.instance, path))
            .cloned()
            .ok_or_else(|| crate::SourcePathError::MissingJoinField {
                join,
                collection: collection
                    .iter()
                    .map(|segment| (*segment).to_string())
                    .collect(),
                path: path.iter().map(|segment| (*segment).to_string()).collect(),
            })
    }

    /// Returns the flattened one-based tuple position for `join`.
    pub fn join_position(&self, join: u64) -> Result<usize, crate::SourcePathError> {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| {
                frame
                    .join_position
                    .filter(|(owner, _)| *owner == join)
                    .map(|(_, position)| position)
            })
            .ok_or(crate::SourcePathError::MissingJoinPosition { join })
    }

    /// Rows of one plan member. The computed member is one row without frames
    /// when its value is present, and no row when it is Null-like.
    fn join_member_rows(
        &self,
        join: u64,
        source: InnerJoinSource<'_>,
        computed: Option<&Value>,
    ) -> Vec<JoinRow<'a>> {
        match source {
            InnerJoinSource::Collection(collection) => self.join_source_rows(join, collection),
            InnerJoinSource::Computed => computed
                .map(|_| JoinRow { frames: Vec::new() })
                .into_iter()
                .collect(),
        }
    }

    fn join_source_rows(&self, join: u64, collection: &[&str]) -> Vec<JoinRow<'a>> {
        self.walk_source(collection)
            .into_iter()
            .enumerate()
            .map(|(index, context)| {
                let mut frames = context.frames[self.frames.len()..].to_vec();
                if frames.is_empty() {
                    return JoinRow { frames };
                }
                for frame in &mut frames {
                    frame.join = Some(join);
                    frame.join_position = None;
                }
                if frames.iter().all(|frame| frame.collection.is_none())
                    && let Some(frame) = frames.last_mut()
                {
                    frame.collection = Some(CollectionIdentity::Repeated {
                        path: collection
                            .iter()
                            .map(|segment| (*segment).to_string())
                            .collect(),
                        index: index + 1,
                    });
                }
                JoinRow { frames }
            })
            .collect()
    }
}

struct JoinRow<'a> {
    frames: Vec<ScopeFrame<'a>>,
}

/// The right member of one examined pairing.
#[derive(Clone, Copy)]
enum RightMember<'r, 'a> {
    Row(&'r JoinRow<'a>),
    /// The computed source, whose key is its value.
    Computed,
}

/// `computed` is the plan's present computed value, if any; a computed
/// member absent there contributed no row, so no pairing reads it.
fn join_conditions_match(
    left: &JoinRow<'_>,
    right: RightMember<'_, '_>,
    computed: Option<&Value>,
    conditions: &[InnerJoinCondition<'_>],
) -> Result<bool, RuntimeError> {
    for condition in conditions {
        let left_value = match condition.left {
            InnerJoinLeft::Collection { collection, path } => row_scalar(left, collection, path),
            InnerJoinLeft::Computed => computed,
        };
        let Some(left_value) = left_value else {
            return Ok(false);
        };
        let right_value = match right {
            RightMember::Row(row) => row
                .frames
                .last()
                .and_then(|frame| direct_scalar(frame.instance, condition.right_path)),
            RightMember::Computed => computed,
        };
        let Some(right_value) = right_value else {
            return Ok(false);
        };
        if is_null_like(left_value) || is_null_like(right_value) {
            return Ok(false);
        }
        if functions::call("equal", &[left_value.clone(), right_value.clone()])?
            != Value::Bool(true)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn row_scalar<'a>(row: &'a JoinRow<'_>, collection: &[&str], path: &[&str]) -> Option<&'a Value> {
    row.frames
        .iter()
        .rfind(|frame| {
            frame
                .collection
                .as_ref()
                .is_some_and(|identity| same_collection(identity.path(), collection))
        })
        .and_then(|frame| direct_scalar(frame.instance, path))
}

fn direct_scalar<'a>(instance: &'a crate::Instance, path: &[&str]) -> Option<&'a Value> {
    let mut current = instance;
    for segment in path {
        current = current.field(segment)?;
    }
    current.as_scalar()
}

fn same_collection(path: &[String], expected: &[&str]) -> bool {
    path.len() == expected.len()
        && path
            .iter()
            .zip(expected)
            .all(|(segment, expected)| segment == expected)
}

fn is_null_like(value: &Value) -> bool {
    value.is_null_like()
}
