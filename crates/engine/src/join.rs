use ir::{Instance, Value};
use mapping::{JoinConditions, JoinId, JoinPlan, JoinSource};

use super::source_iteration::{PositionFrame, WalkExtension, walk};
use super::{EngineError, field_scalar};

pub(super) struct JoinedRow<'a> {
    instances: Vec<&'a Instance>,
    positions: Vec<PositionFrame>,
}

pub(super) fn execute<'a>(
    context: &[&'a Instance],
    join: JoinId,
    plan: &JoinPlan,
) -> Result<Vec<JoinedRow<'a>>, EngineError> {
    let mut sources = plan.sources();
    let Some(first) = sources.next() else {
        return Ok(Vec::new());
    };
    let mut rows = source_rows(context, join, first);
    for (right_source, conditions) in plan.stages() {
        let right_rows = source_rows(context, join, right_source);
        let mut joined = Vec::new();
        for left in rows {
            for right in &right_rows {
                if conditions_match(&left, right, conditions)? {
                    let mut instances = left.instances.clone();
                    instances.extend(right.instances.iter().copied());
                    let mut positions = left.positions.clone();
                    positions.extend(right.positions.iter().cloned());
                    joined.push(JoinedRow {
                        instances,
                        positions,
                    });
                }
            }
        }
        rows = joined;
    }
    for (index, row) in rows.iter_mut().enumerate() {
        if let Some(position) = row.positions.last_mut() {
            position.join_position = Some((join, index + 1));
        }
    }
    Ok(rows)
}

pub(super) fn extensions<'a>(rows: &[JoinedRow<'a>]) -> Vec<WalkExtension<'a>> {
    rows.iter()
        .map(|row| WalkExtension {
            instances: row.instances.clone(),
            positions: row.positions.clone(),
        })
        .collect()
}

fn source_rows<'a>(
    context: &[&'a Instance],
    join: JoinId,
    source: &JoinSource,
) -> Vec<JoinedRow<'a>> {
    let collection = source.collection();
    context
        .iter()
        .rev()
        .find(|frame| match collection.first() {
            Some(first) => frame.field(first).is_some(),
            None => true,
        })
        .copied()
        .or_else(|| context.last().copied())
        .map_or_else(Vec::new, |base| {
            walk(base, collection, &[], &[], &[])
                .into_iter()
                .enumerate()
                .map(|(index, extension)| {
                    let mut positions = extension.positions;
                    if positions.is_empty() {
                        positions.push(PositionFrame {
                            collection: collection.to_vec(),
                            index: index + 1,
                            grouped: false,
                            join: Some(join),
                            join_position: None,
                        });
                    } else {
                        for position in &mut positions {
                            position.join = Some(join);
                        }
                    }
                    JoinedRow {
                        instances: extension.instances,
                        positions,
                    }
                })
                .collect()
        })
}

fn conditions_match(
    left: &JoinedRow<'_>,
    right: &JoinedRow<'_>,
    conditions: &JoinConditions,
) -> Result<bool, EngineError> {
    for condition in conditions.iter() {
        let Some(left_value) = row_value(left, condition.left_collection(), condition.left_path())
        else {
            return Ok(false);
        };
        let Some(right_value) = right
            .instances
            .last()
            .and_then(|item| field_scalar(item, condition.right_path()))
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

fn row_value<'a>(
    row: &'a JoinedRow<'_>,
    collection: &[String],
    path: &[String],
) -> Option<&'a Value> {
    let index = row
        .positions
        .iter()
        .rposition(|position| position.collection == collection)?;
    row.instances
        .get(index)
        .and_then(|item| field_scalar(item, path))
}

fn is_null_like(value: &Value) -> bool {
    *value == Value::Null || value.is_xml_nil()
}
