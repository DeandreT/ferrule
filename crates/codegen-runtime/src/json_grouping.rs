//! Pure preparation of one ordered JSON document for multi-key grouping.
use crate::{Instance, JsonProjectionLimits, Value};
use mapping::{JsonGroupingPreparation, JsonProjectionPlanError, JsonSourceShape};

pub const MAX_JSON_GROUPED_TEXT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub enum JsonGroupingError {
    Plan(JsonProjectionPlanError),
    Descriptor,
    InvalidLimits,
    Shape,
    Values,
    Names,
    Text,
}
impl std::fmt::Display for JsonGroupingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JSON grouping preparation refused: {self:?}")
    }
}
impl std::error::Error for JsonGroupingError {}

pub fn parse_json_grouping_preparation(
    text: &str,
) -> Result<JsonGroupingPreparation, JsonGroupingError> {
    if text.len() > 1024 * 1024 {
        return Err(JsonGroupingError::Descriptor);
    }
    let plan: JsonGroupingPreparation =
        serde_json::from_str(text).map_err(|_| JsonGroupingError::Descriptor)?;
    plan.validate_paths().map_err(JsonGroupingError::Plan)?;
    Ok(plan)
}

/// Each distinct key on an object creates a candidate containing that unchanged
/// object. Original key occurrences and first-seen membership order are retained.
pub fn prepare_json_grouping_document(
    plan: &JsonGroupingPreparation,
    shape: &JsonSourceShape,
    mut document: Instance,
    limits: JsonProjectionLimits,
) -> Result<Instance, JsonGroupingError> {
    plan.validate(shape).map_err(JsonGroupingError::Plan)?;
    if limits.values > crate::MAX_JSON_PROJECTED_VALUES
        || limits.name_bytes > crate::MAX_JSON_PROJECTED_NAME_BYTES
    {
        return Err(JsonGroupingError::InvalidLimits);
    }
    let mut budget = Budget {
        values: limits.values,
        names: limits.name_bytes,
        text: MAX_JSON_GROUPED_TEXT_BYTES,
    };
    budget.measure(&document, 0)?;
    rewrite(&mut document, &plan.collection, &plan.keys, &mut budget)?;
    Ok(document)
}

struct Budget {
    values: usize,
    names: usize,
    text: usize,
}
impl Budget {
    fn value(&mut self) -> Result<(), JsonGroupingError> {
        self.values = self
            .values
            .checked_sub(1)
            .ok_or(JsonGroupingError::Values)?;
        Ok(())
    }
    fn name(&mut self, value: &str) -> Result<(), JsonGroupingError> {
        self.names = self
            .names
            .checked_sub(value.len())
            .ok_or(JsonGroupingError::Names)?;
        Ok(())
    }
    fn text(&mut self, value: &str) -> Result<(), JsonGroupingError> {
        self.text = self
            .text
            .checked_sub(value.len())
            .ok_or(JsonGroupingError::Text)?;
        Ok(())
    }
    fn measure(&mut self, value: &Instance, depth: usize) -> Result<(), JsonGroupingError> {
        if depth > 128 {
            return Err(JsonGroupingError::Shape);
        }
        self.value()?;
        match value {
            Instance::Group(fields) => {
                for (name, value) in fields.iter() {
                    self.name(name)?;
                    self.measure(value, depth + 1)?;
                }
            }
            Instance::Repeated(items) => {
                for value in items {
                    self.measure(value, depth + 1)?;
                }
            }
            Instance::Scalar(Value::String(text)) => self.text(text)?,
            Instance::Scalar(_) => {}
            _ => return Err(JsonGroupingError::Shape),
        }
        Ok(())
    }
}

fn rewrite(
    value: &mut Instance,
    path: &[String],
    keys: &[String],
    budget: &mut Budget,
) -> Result<(), JsonGroupingError> {
    if path.is_empty() {
        let Instance::Repeated(items) = value else {
            return Err(JsonGroupingError::Shape);
        };
        let originals = std::mem::take(items);
        for original in originals {
            if !matches!(original, Instance::Group(_)) {
                return Err(JsonGroupingError::Shape);
            }
            let mut selected = Vec::new();
            select_keys(&original, keys, &mut selected)?;
            let mut seen = std::collections::BTreeSet::new();
            for key in selected {
                if !seen.insert(key.clone()) {
                    continue;
                }
                budget.value()?;
                budget.name(JsonGroupingPreparation::KEY)?;
                budget.name(JsonGroupingPreparation::MEMBER)?;
                budget.value()?;
                budget.text(&key)?;
                budget.measure(&original, 0)?;
                items.push(Instance::Group(
                    vec![
                        (
                            JsonGroupingPreparation::KEY.into(),
                            Instance::Scalar(Value::String(key)),
                        ),
                        (JsonGroupingPreparation::MEMBER.into(), original.clone()),
                    ]
                    .into(),
                ));
            }
        }
        return Ok(());
    }
    if let Instance::Repeated(items) = value {
        for item in items {
            rewrite(item, path, keys, budget)?;
        }
        return Ok(());
    }
    let Instance::Group(fields) = value else {
        return Err(JsonGroupingError::Shape);
    };
    if fields.iter().filter(|(name, _)| name == &path[0]).count() != 1 {
        return Err(JsonGroupingError::Shape);
    }
    let field = fields
        .iter_mut()
        .find(|(name, _)| name == &path[0])
        .ok_or(JsonGroupingError::Shape)?;
    rewrite(&mut field.1, &path[1..], keys, budget)
}

fn select_keys(
    value: &Instance,
    path: &[String],
    output: &mut Vec<String>,
) -> Result<(), JsonGroupingError> {
    if let Instance::Repeated(items) = value {
        for item in items {
            select_keys(item, path, output)?;
        }
        return Ok(());
    }
    if path.is_empty() {
        let Instance::Group(fields) = value else {
            return Err(JsonGroupingError::Shape);
        };
        if fields.len() != 1 {
            return Err(JsonGroupingError::Shape);
        }
        let Some(Instance::Scalar(Value::String(value))) = value.field("Value") else {
            return Err(JsonGroupingError::Shape);
        };
        output.push(value.clone());
        return Ok(());
    }
    let Instance::Group(fields) = value else {
        return Err(JsonGroupingError::Shape);
    };
    if fields.iter().filter(|(name, _)| name == &path[0]).count() != 1 {
        return Err(JsonGroupingError::Shape);
    }
    select_keys(
        value.field(&path[0]).ok_or(JsonGroupingError::Shape)?,
        &path[1..],
        output,
    )
}

#[cfg(test)]
#[path = "json_grouping_tests.rs"]
mod tests;
