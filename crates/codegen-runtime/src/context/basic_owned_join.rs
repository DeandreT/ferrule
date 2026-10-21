//! Owned inner joins over the paired Basic context.
//!
//! Every source walks exactly its declared endpoint with the owned cohort walk,
//! so the latest active same-owner frame anchors it and a source equal to an
//! active owned row is that row. Tuples keep left-major order and duplicate
//! multiplicity. Each member keeps its raw source position; the tuple carries
//! a separate flattened position that sorting renumbers and output compacts.

use super::*;
use std::rc::Rc;

/// One source of an owned join plan, in plan order.
#[derive(Clone, Copy, Debug)]
pub struct OwnedJoinSource<'a> {
    pub owner: SourceOwnerRef<'a>,
    pub collection: &'a [&'a str],
    pub singleton: bool,
}

/// One equality between an earlier member, by plan index, and the member
/// introduced by a stage. Paths are relative to their members.
#[derive(Clone, Copy, Debug)]
pub struct OwnedJoinKey<'a> {
    pub left: usize,
    pub left_path: &'a [&'a str],
    pub right_path: &'a [&'a str],
}

/// One source after the first in a left-deep owned join.
#[derive(Clone, Copy, Debug)]
pub struct OwnedJoinStage<'a> {
    pub source: OwnedJoinSource<'a>,
    pub keys: &'a [OwnedJoinKey<'a>],
}

enum MemberRead<'ctx> {
    /// The member's own walked occurrence.
    Item(Box<BasicScopeContext<'ctx>>),
    /// A singleton scalar read once in the receiving context.
    Value(Value),
}

pub(super) struct OwnedJoinMember<'ctx> {
    owner: Option<String>,
    collection: Vec<String>,
    read: MemberRead<'ctx>,
    raw: usize,
}

impl OwnedJoinMember<'_> {
    fn owner(&self) -> SourceOwnerRef<'_> {
        self.owner
            .as_deref()
            .map_or(SourceOwnerRef::Primary, SourceOwnerRef::Named)
    }

    fn is(&self, owner: SourceOwnerRef<'_>, collection: &[&str]) -> bool {
        let same_owner = match owner {
            SourceOwnerRef::Primary => self.owner.is_none(),
            SourceOwnerRef::Named(name) => self.owner.as_deref() == Some(name),
        };
        same_owner
            && self.collection.len() == collection.len()
            && self
                .collection
                .iter()
                .zip(collection)
                .all(|(segment, expected)| segment == expected)
    }

    /// An owned read relative to this member; a singleton is read with an
    /// empty path.
    fn value(&self, relative: &[&str]) -> Result<Value, RuntimeError> {
        match &self.read {
            MemberRead::Item(context) => {
                let path = self
                    .collection
                    .iter()
                    .map(String::as_str)
                    .chain(relative.iter().copied())
                    .collect::<Vec<_>>();
                context.owned_field(self.owner(), &path, None)
            }
            MemberRead::Value(value) if relative.is_empty() => Ok(value.clone()),
            MemberRead::Value(_) => Ok(Value::Null),
        }
    }
}

pub(super) struct OwnedJoinTuple<'ctx> {
    join: u64,
    members: Vec<Rc<OwnedJoinMember<'ctx>>>,
    position: usize,
}

fn is_null_like(value: &Value) -> bool {
    matches!(value, Value::Null | Value::JsonNull(_) | Value::XmlNil(_))
}

fn keys_match(
    left: &[Rc<OwnedJoinMember<'_>>],
    right: &OwnedJoinMember<'_>,
    keys: &[OwnedJoinKey<'_>],
) -> Result<bool, RuntimeError> {
    for key in keys {
        // Emitters index earlier members in plan order.
        let Some(member) = left.get(key.left) else {
            return Ok(false);
        };
        let left = member.value(key.left_path)?;
        let right = right.value(key.right_path)?;
        if is_null_like(&left) || is_null_like(&right) {
            return Ok(false);
        }
        if functions::call("equal", &[left, right])? != Value::Bool(true) {
            return Ok(false);
        }
    }
    Ok(true)
}

impl<'ctx> BasicScopeContext<'ctx> {
    fn owned_join_members(
        &self,
        source: OwnedJoinSource<'_>,
    ) -> Result<Vec<Rc<OwnedJoinMember<'ctx>>>, RuntimeError> {
        let owner = match source.owner {
            SourceOwnerRef::Primary => None,
            SourceOwnerRef::Named(name) => Some(name.to_owned()),
        };
        let collection = source
            .collection
            .iter()
            .map(|segment| (*segment).to_owned())
            .collect::<Vec<_>>();
        if source.singleton {
            let value = self.owned_field(source.owner, source.collection, None)?;
            return Ok(vec![Rc::new(OwnedJoinMember {
                owner,
                collection,
                read: MemberRead::Value(value),
                raw: 1,
            })]);
        }
        let cohort = self.walk_owned_source_cohort(source.owner, source.collection)?;
        // An identity selection is the active occurrence at its current
        // position; an endpoint root identity is position one.
        let identity = || {
            self.owned_position(source.owner, Some(source.collection))
                .unwrap_or(1)
        };
        cohort
            .candidates
            .into_iter()
            .map(|candidate| {
                let raw = candidate.raw_source_ordinal().unwrap_or_else(identity);
                Ok(Rc::new(OwnedJoinMember {
                    owner: owner.clone(),
                    collection: collection.clone(),
                    read: MemberRead::Item(Box::new(candidate.into_context())),
                    raw,
                }))
            })
            .collect()
    }

    /// Executes one owned join in this receiving context. Every source of a
    /// stage is walked before its first comparison; the tuple count shares
    /// the walk-context limit.
    pub fn owned_inner_join(
        &self,
        join: u64,
        first: OwnedJoinSource<'_>,
        stages: &[OwnedJoinStage<'_>],
    ) -> Result<Vec<Self>, RuntimeError> {
        let mut rows = self
            .owned_join_members(first)?
            .into_iter()
            .map(|member| vec![member])
            .collect::<Vec<_>>();
        let limit = self.owned.registry.limits.max_walk_contexts;
        let mut produced = 0;
        for stage in stages {
            let right = self.owned_join_members(stage.source)?;
            let mut joined = Vec::new();
            for left in rows {
                for member in &right {
                    if keys_match(&left, member, stage.keys)? {
                        produced =
                            owned_charge(OwnedSourceLimitKind::WalkContexts, produced, 1, limit)?;
                        let mut tuple = left.clone();
                        tuple.push(Rc::clone(member));
                        joined.push(tuple);
                    }
                }
            }
            rows = joined;
        }
        rows.into_iter()
            .enumerate()
            .map(|(index, members)| {
                let mut context = self.try_clone_view()?;
                context.join = Some(Rc::new(OwnedJoinTuple {
                    join,
                    members,
                    position: index + 1,
                }));
                Ok(context)
            })
            .collect()
    }

    fn owned_join_member(
        &self,
        join: u64,
        owner: SourceOwnerRef<'_>,
        collection: &[&str],
        path: &[&str],
    ) -> Result<&OwnedJoinMember<'ctx>, RuntimeError> {
        self.join
            .as_deref()
            .filter(|tuple| tuple.join == join)
            .and_then(|tuple| {
                tuple
                    .members
                    .iter()
                    .find(|member| member.is(owner, collection))
            })
            .map(Rc::as_ref)
            .ok_or_else(|| {
                crate::SourcePathError::MissingJoinField {
                    join,
                    collection: collection
                        .iter()
                        .map(|segment| (*segment).to_owned())
                        .collect(),
                    path: path.iter().map(|segment| (*segment).to_owned()).collect(),
                }
                .into()
            })
    }

    /// Reads one exact tuple member, bound by its owner and collection.
    pub fn owned_join_field(
        &self,
        join: u64,
        owner: SourceOwnerRef<'_>,
        collection: &[&str],
        path: &[&str],
    ) -> Result<Value, RuntimeError> {
        self.owned_join_member(join, owner, collection, path)?
            .value(path)
    }

    /// The raw one-based source position of one exact tuple member.
    pub fn owned_join_source_position(
        &self,
        join: u64,
        owner: SourceOwnerRef<'_>,
        collection: &[&str],
    ) -> Result<usize, RuntimeError> {
        Ok(self.owned_join_member(join, owner, collection, &[])?.raw)
    }

    /// The flattened one-based tuple position of the active owned join.
    pub fn owned_join_position(&self, join: u64) -> Result<usize, RuntimeError> {
        self.join
            .as_deref()
            .filter(|tuple| tuple.join == join)
            .map(|tuple| tuple.position)
            .ok_or_else(|| crate::SourcePathError::MissingJoinPosition { join }.into())
    }

    /// A view whose flattened tuple position is replaced; members and their
    /// raw positions are shared unchanged.
    pub fn with_owned_join_position(
        &self,
        join: u64,
        position: usize,
    ) -> Result<Self, RuntimeError> {
        let tuple = self
            .join
            .as_deref()
            .filter(|tuple| tuple.join == join)
            .ok_or(crate::SourcePathError::MissingJoinPosition { join })?;
        let members = tuple.members.clone();
        let mut context = self.try_clone_view()?;
        context.join = Some(Rc::new(OwnedJoinTuple {
            join,
            members,
            position,
        }));
        Ok(context)
    }
}

#[cfg(test)]
#[path = "basic_owned_join_tests.rs"]
mod tests;
