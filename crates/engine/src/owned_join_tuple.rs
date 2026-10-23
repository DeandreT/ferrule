//! Owned inner-join tuples.
//!
//! Each source contributes members only from its declared endpoint. The latest
//! active frame of the same owner whose collection prefixes the source anchors
//! its walk, so a source equal to an active owned row is that one row. Tuples
//! keep left-major order and duplicate multiplicity; a member keeps its raw
//! source position while the tuple carries its separate flattened position.

use std::rc::Rc;

use ir::Value;
use mapping::{
    JoinId, JoinSourceCardinality, NodeId, OwnedJoinConditions, OwnedJoinField, OwnedJoinSource,
    OwnedJoinSourcePosition, OwnedSourceField, OwnedSourceRef,
};

use crate::eval_expr::EvalProgram;
use crate::frame_view::latest_owned;
use crate::owned_context::{OccurrenceKind, OwnedContext, WorkBudget};
use crate::source_iteration::{PositionFrame, walk_observed};
use crate::{EngineError, OwnedSourceError};

enum MemberRead<'a> {
    /// The member's own observed occurrence: its frames read the item.
    Item(OwnedContext<'a>),
    /// A singleton scalar read once in the receiving context.
    Value(Value),
}

pub(crate) struct OwnedJoinMember<'a> {
    collection: OwnedSourceRef,
    read: MemberRead<'a>,
    raw: usize,
}

impl OwnedJoinMember<'_> {
    /// An owned read relative to this member. A singleton member is read with
    /// an empty path, which validation requires.
    fn value(&self, relative: &[String]) -> Result<Value, OwnedSourceError> {
        match &self.read {
            MemberRead::Item(context) => {
                let mut path = self.collection.path.clone();
                path.extend(relative.iter().cloned());
                context.field(&OwnedSourceField {
                    source: OwnedSourceRef {
                        owner: self.collection.owner.clone(),
                        path,
                    },
                    frame: None,
                })
            }
            MemberRead::Value(value) if relative.is_empty() => Ok(value.clone()),
            MemberRead::Value(_) => Ok(Value::Null),
        }
    }
}

/// One joined tuple: members in plan order and the flattened one-based tuple
/// position, renumbered by sorting and compacted on output.
#[derive(Clone)]
pub(crate) struct OwnedJoinTuple<'a> {
    join: JoinId,
    members: Vec<Rc<OwnedJoinMember<'a>>>,
    position: usize,
}

impl<'a> OwnedJoinTuple<'a> {
    pub(crate) fn with_position(&self, position: usize) -> Self {
        Self {
            join: self.join,
            members: self.members.clone(),
            position,
        }
    }

    pub(crate) fn position(&self, join: JoinId) -> Option<usize> {
        (self.join == join).then_some(self.position)
    }

    fn member(&self, join: JoinId, collection: &OwnedSourceRef) -> Option<&OwnedJoinMember<'a>> {
        (self.join == join)
            .then(|| {
                self.members
                    .iter()
                    .find(|member| member.collection == *collection)
            })
            .flatten()
            .map(Rc::as_ref)
    }
}

/// Members of one source, admitted completely before any key comparison.
fn members<'a>(
    state: &OwnedContext<'a>,
    positions: &[PositionFrame],
    source: &OwnedJoinSource,
) -> Result<Vec<Rc<OwnedJoinMember<'a>>>, OwnedSourceError> {
    let collection = source.collection();
    if source.cardinality() == JoinSourceCardinality::Singleton {
        let value = state.field(&OwnedSourceField {
            source: collection.clone(),
            frame: None,
        })?;
        return Ok(vec![Rc::new(OwnedJoinMember {
            collection: collection.clone(),
            read: MemberRead::Value(value),
            raw: 1,
        })]);
    }
    let (owner, base, prefix, tail, work) = state.select(collection)?;
    let rows = walk_observed(
        base,
        tail,
        prefix,
        prefix,
        Some(owner),
        state,
        positions.len(),
        true,
        &work,
    )?;
    // An identity selection is the active occurrence itself, at its current
    // position; an endpoint root identity is position one.
    let identity = latest_owned(&state.frames, positions, |frame| {
        frame.owner == owner
            && frame.collection == collection.path
            && frame.kind == OccurrenceKind::Item
    })
    .and_then(|(_, position)| position)
    .map_or(1, |position| position.index);
    Ok(rows
        .into_iter()
        .map(|row| {
            let raw = row
                .extension
                .positions
                .last()
                .map_or(identity, |position| position.index);
            Rc::new(OwnedJoinMember {
                collection: collection.clone(),
                read: MemberRead::Item(OwnedContext {
                    roots: state.roots,
                    frames: row.frames,
                    origins: row.origins,
                    owned_ancestry: state.owned_ancestry,
                }),
                raw,
            })
        })
        .collect())
}

fn conditions_match(
    left: &[Rc<OwnedJoinMember<'_>>],
    right: &OwnedJoinMember<'_>,
    conditions: &OwnedJoinConditions,
    access: &impl Fn(OwnedSourceError) -> EngineError,
) -> Result<bool, EngineError> {
    for condition in conditions.iter() {
        // Plan construction guarantees the left identity is already joined.
        let Some(left) = left
            .iter()
            .find(|member| member.collection == *condition.left_collection())
        else {
            return Ok(false);
        };
        let left = left.value(condition.left_path()).map_err(access)?;
        let right = right.value(condition.right_path()).map_err(access)?;
        if left.is_null_like() || right.is_null_like() {
            return Ok(false);
        }
        if functions::call("equal", &[left, right])? != Value::Bool(true) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Executes one owned plan in the receiving owned context. Every source is
/// selected before the first comparison of its stage, and the tuple count is
/// bounded by the owned walk-context limit.
pub(crate) fn execute<'a>(
    state: &OwnedContext<'a>,
    positions: &[PositionFrame],
    join: JoinId,
    plan: &mapping::OwnedJoinPlan,
    budget: &WorkBudget,
    access: impl Fn(OwnedSourceError) -> EngineError,
) -> Result<Vec<OwnedJoinTuple<'a>>, EngineError> {
    let mut sources = plan.sources();
    let Some(first) = sources.next() else {
        return Ok(Vec::new());
    };
    let mut rows: Vec<Vec<Rc<OwnedJoinMember<'a>>>> = members(state, positions, first)
        .map_err(&access)?
        .into_iter()
        .map(|member| vec![member])
        .collect();
    for (source, conditions) in plan.stages() {
        let right = members(state, positions, source).map_err(&access)?;
        let mut joined = Vec::new();
        for left in rows {
            for member in &right {
                if conditions_match(&left, member, conditions, &access)? {
                    budget.emit().map_err(&access)?;
                    let mut tuple = left.clone();
                    tuple.push(Rc::clone(member));
                    joined.push(tuple);
                }
            }
        }
        rows = joined;
    }
    Ok(rows
        .into_iter()
        .enumerate()
        .map(|(index, members)| OwnedJoinTuple {
            join,
            members,
            position: index + 1,
        })
        .collect())
}

fn tuple_member<'p>(
    program: EvalProgram<'p>,
    node: NodeId,
    join: JoinId,
    collection: &OwnedSourceRef,
) -> Result<&'p OwnedJoinMember<'p>, EngineError> {
    program
        .owned_join
        .and_then(|tuple| tuple.member(join, collection))
        .ok_or_else(|| match program.owned_join {
            Some(tuple) if tuple.join == join => EngineError::OwnedSourceAccess {
                node: Some(node),
                scope: None,
                source: OwnedSourceError::InactiveSourceFrame {
                    owner: collection.owner.clone(),
                    collection: Some(collection.path.clone()),
                },
            },
            _ => EngineError::MissingJoinContext { join },
        })
}

pub(crate) fn eval_field(
    program: EvalProgram<'_>,
    node: NodeId,
    field: &OwnedJoinField,
) -> Result<Value, EngineError> {
    tuple_member(program, node, field.join, &field.collection)?
        .value(&field.path)
        .map_err(|source| EngineError::OwnedSourceAccess {
            node: Some(node),
            scope: None,
            source,
        })
}

pub(crate) fn eval_source_position(
    program: EvalProgram<'_>,
    node: NodeId,
    position: &OwnedJoinSourcePosition,
) -> Result<Value, EngineError> {
    let member = tuple_member(program, node, position.join, &position.collection)?;
    i64::try_from(member.raw)
        .map(Value::Int)
        .map_err(|_| EngineError::OwnedSourceAccess {
            node: Some(node),
            scope: None,
            source: OwnedSourceError::ResourceLimit {
                resource: "position integer",
                limit: i64::MAX as usize,
            },
        })
}
