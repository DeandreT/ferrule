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
        let mut rows = self.join_source_rows(join, first);
        for stage in std::iter::once(second).chain(rest.iter().copied()) {
            let right_rows = self.join_source_rows(join, stage.collection);
            let mut joined = Vec::new();
            for left in rows {
                for right in &right_rows {
                    if join_keys_match(&left, right, stage.keys)? {
                        let mut frames = left.frames.clone();
                        frames.extend(right.frames.iter().cloned());
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
                    frames,
                    named_inputs: self.named_inputs,
                    execution: self.execution,
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

fn join_keys_match(
    left: &JoinRow<'_>,
    right: &JoinRow<'_>,
    keys: &[InnerJoinKey<'_>],
) -> Result<bool, RuntimeError> {
    for key in keys {
        let Some(left_value) = row_scalar(left, key.left_collection, key.left_path) else {
            return Ok(false);
        };
        let Some(right_value) = right
            .frames
            .last()
            .and_then(|frame| direct_scalar(frame.instance, key.right_path))
        else {
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
    matches!(value, Value::Null | Value::XmlNil(_))
}
