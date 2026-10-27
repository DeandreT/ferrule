//! Explicit selection for parameter-driven invocations, separate from file input.

use crate::{JsonProjectionPlanError, JsonSourceShape, MAX_RUNTIME_PARAMETER_NAME_BYTES};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsonParameterIteration {
    RootStringOccurrences,
    DistinctRootStringKeys,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonParameterInvocationPlan {
    pub parameter: String,
    pub key_member: String,
    pub iteration: JsonParameterIteration,
    pub key_parameter: String,
    pub projection: JsonSourceShape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonParameterPlanError {
    ParameterName,
    KeyParameterName,
    CollidingParameters,
    Projection(JsonProjectionPlanError),
    StringRootMember,
}

impl std::fmt::Display for JsonParameterPlanError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "invalid JSON parameter invocation plan: {self:?}")
    }
}

impl std::error::Error for JsonParameterPlanError {}

impl JsonParameterInvocationPlan {
    pub fn validate(&self) -> Result<(), JsonParameterPlanError> {
        let name = |value: &str| {
            !value.is_empty()
                && !value.contains('\0')
                && value.len() <= MAX_RUNTIME_PARAMETER_NAME_BYTES
        };
        if !name(&self.parameter) {
            return Err(JsonParameterPlanError::ParameterName);
        }
        if !name(&self.key_parameter) {
            return Err(JsonParameterPlanError::KeyParameterName);
        }
        if self.parameter == self.key_parameter {
            return Err(JsonParameterPlanError::CollidingParameters);
        }
        self.projection
            .validate()
            .map_err(JsonParameterPlanError::Projection)?;
        let JsonSourceShape::Object { properties } = &self.projection else {
            return Err(JsonParameterPlanError::StringRootMember);
        };
        if !properties.iter().any(|property| {
            property.name == self.key_member && property.shape == JsonSourceShape::String
        }) {
            return Err(JsonParameterPlanError::StringRootMember);
        }
        Ok(())
    }
}
