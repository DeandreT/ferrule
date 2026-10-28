//! Pure, bounded projection of ordered JSON into explicit occurrence collections.
//! No transport, schema validation, numeric conversion or value deduplication.

use crate::{JsonSourceDocument, JsonSourceKind, JsonSourceValue};
use ir::{Instance, Value};
use mapping::{JsonProjectionPlanError, JsonSourceShape};

pub const MAX_JSON_PROJECTED_VALUES: usize = 1_000_000;
pub const MAX_JSON_PROJECTED_NAME_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonProjectionLimits {
    pub values: usize,
    pub name_bytes: usize,
}

impl Default for JsonProjectionLimits {
    fn default() -> Self {
        Self {
            values: MAX_JSON_PROJECTED_VALUES,
            name_bytes: MAX_JSON_PROJECTED_NAME_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonProjectionError {
    Descriptor,
    Plan(JsonProjectionPlanError),
    InvalidLimits,
    Values,
    Names,
}

impl std::fmt::Display for JsonProjectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JSON source projection: {self:?}")
    }
}
impl std::error::Error for JsonProjectionError {}

pub fn parse_json_projection_shape(
    descriptor: &str,
) -> Result<JsonSourceShape, JsonProjectionError> {
    if descriptor.len() > 1024 * 1024 {
        return Err(JsonProjectionError::Descriptor);
    }
    let shape: JsonSourceShape =
        serde_json::from_str(descriptor).map_err(|_| JsonProjectionError::Descriptor)?;
    shape.validate().map_err(JsonProjectionError::Plan)?;
    Ok(shape)
}

/// Keep the physical document even when its root has the wrong JSON kind.
/// Number lexicals are not converted, so an undemanded overflow stays harmless.
pub fn project_json_source_document(
    shape: &JsonSourceShape,
    document: &JsonSourceDocument,
    limits: JsonProjectionLimits,
) -> Result<Instance, JsonProjectionError> {
    validate(shape, limits)?;
    let mut budget = Budget(limits);
    budget.value()?; // document
    budget.value()?; // path
    budget.value()?; // Root collection
    budget.name("Path")?;
    budget.name("Root")?;
    let root = project(shape, document.root(), &mut budget)?;
    Ok(Instance::Group(
        vec![
            (
                "Path".into(),
                Instance::Scalar(Value::String(document.identity().into())),
            ),
            (
                "Root".into(),
                Instance::Repeated(root.into_iter().collect()),
            ),
        ]
        .into(),
    ))
}

/// Project an already parsed value without inventing a physical file identity.
/// This preserves typed occurrences and numeric lexicals for parameter roots.
pub fn project_json_source_value(
    shape: &JsonSourceShape,
    value: &JsonSourceValue,
    limits: JsonProjectionLimits,
) -> Result<Option<Instance>, JsonProjectionError> {
    validate(shape, limits)?;
    project(shape, value, &mut Budget(limits))
}

fn validate(
    shape: &JsonSourceShape,
    limits: JsonProjectionLimits,
) -> Result<(), JsonProjectionError> {
    shape.validate().map_err(JsonProjectionError::Plan)?;
    if limits.values > MAX_JSON_PROJECTED_VALUES
        || limits.name_bytes > MAX_JSON_PROJECTED_NAME_BYTES
    {
        return Err(JsonProjectionError::InvalidLimits);
    }
    Ok(())
}

struct Budget(JsonProjectionLimits);
impl Budget {
    fn value(&mut self) -> Result<(), JsonProjectionError> {
        self.0.values = self
            .0
            .values
            .checked_sub(1)
            .ok_or(JsonProjectionError::Values)?;
        Ok(())
    }
    fn name(&mut self, name: &str) -> Result<(), JsonProjectionError> {
        self.0.name_bytes = self
            .0
            .name_bytes
            .checked_sub(name.len())
            .ok_or(JsonProjectionError::Names)?;
        Ok(())
    }
}

fn project(
    shape: &JsonSourceShape,
    value: &JsonSourceValue,
    budget: &mut Budget,
) -> Result<Option<Instance>, JsonProjectionError> {
    let matches = matches!(
        (shape, value.kind()),
        (JsonSourceShape::Object { .. }, JsonSourceKind::Object)
            | (JsonSourceShape::Array { .. }, JsonSourceKind::Array)
            | (JsonSourceShape::String, JsonSourceKind::String)
            | (JsonSourceShape::Number, JsonSourceKind::Number)
            | (JsonSourceShape::Boolean, JsonSourceKind::Boolean)
            | (JsonSourceShape::Null, JsonSourceKind::Null)
    );
    if !matches {
        return Ok(None);
    }
    budget.value()?;
    let fields = match shape {
        JsonSourceShape::Object { properties } => {
            let mut fields = Vec::new();
            let positions: std::collections::BTreeMap<_, _> = properties
                .iter()
                .enumerate()
                .map(|(index, property)| (property.name.as_str(), index))
                .collect();
            let mut candidates = vec![Vec::new(); properties.len()];
            for (name, value) in value.members() {
                if let Some(&index) = positions.get(name.as_str()) {
                    candidates[index].push(value);
                }
            }
            for (property, candidates) in properties.iter().zip(candidates) {
                budget.name(&property.name)?;
                budget.value()?;
                let mut items = Vec::new();
                for value in candidates {
                    if let Some(item) = project(&property.shape, value, budget)? {
                        items.push(item);
                    }
                }
                fields.push((property.name.clone(), Instance::Repeated(items)));
            }
            fields
        }
        JsonSourceShape::Array { item } => {
            budget.name("Items")?;
            budget.value()?;
            let mut items = Vec::new();
            for value in value.items() {
                if let Some(value) = project(item, value, budget)? {
                    items.push(value);
                }
            }
            vec![("Items".into(), Instance::Repeated(items))]
        }
        JsonSourceShape::Null => vec![],
        _ => {
            budget.name("Value")?;
            budget.value()?;
            let value = match shape {
                JsonSourceShape::String => Value::String(value.as_str().unwrap().into()),
                JsonSourceShape::Number => Value::String(value.number_text().unwrap().into()),
                JsonSourceShape::Boolean => Value::Bool(value.as_bool().unwrap()),
                _ => unreachable!(),
            };
            vec![("Value".into(), Instance::Scalar(value))]
        }
    };
    Ok(Some(Instance::Group(fields.into())))
}
