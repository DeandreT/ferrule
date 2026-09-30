//! Literal defaults and exact typed host inputs for lowered database queries.

use ir::{ScalarType, Value};

use super::super::function::{FnComponent, is_input, parse_constant};
use super::GraphBuilder;

impl GraphBuilder<'_> {
    /// A required named input with no connected value or enabled preview must
    /// come from the host. Keep this separate from optional inputs, whose
    /// absent-value behavior is not established by a defaultless declaration.
    pub(super) fn required_host_query_parameter_feed(
        &self,
        input_key: u32,
        column_type: ScalarType,
        declared_type: ScalarType,
    ) -> Result<Option<u32>, String> {
        let Some(feed) = self.edge_from.get(&input_key).copied() else {
            return Ok(None);
        };
        let Some(index) = self.fn_by_output.get(&feed).copied() else {
            return Ok(None);
        };
        let component = &self.fn_components[index];
        if !is_input(component)
            || component
                .input_parameter_name
                .as_ref()
                .is_none_or(|parameter| parameter.optional)
            || component.input_preview.is_some()
            || component.outputs.as_slice() != [feed]
            || component.inputs.len() > 1
            || component
                .inputs
                .iter()
                .flatten()
                .any(|input| self.edge_from.contains_key(input))
        {
            return Ok(None);
        }
        if declared_type != column_type || component.input_type != Some(column_type) {
            return Err(
                "required host query parameter, SQL declaration, and compared column must have the same scalar type"
                    .to_string(),
            );
        }
        Ok(Some(feed))
    }

    /// Keep exact typed optional inputs in the graph instead of freezing their
    /// literal default into the query. The legacy constant path still handles
    /// required preview inputs; cross-type dynamic coercion is not inferred.
    pub(super) fn optional_query_parameter_feed(
        &self,
        input_key: u32,
        column_type: ScalarType,
        depth: usize,
    ) -> Result<Option<u32>, String> {
        if depth >= 12 {
            return Err("query parameter feed contains a cycle".to_string());
        }
        let Some(feed) = self.edge_from.get(&input_key).copied() else {
            return Ok(None);
        };
        let Some(index) = self.fn_by_output.get(&feed).copied() else {
            return Ok(None);
        };
        let component = &self.fn_components[index];
        if !is_input(component) {
            return Ok(None);
        }
        let optional = component
            .input_parameter_name
            .as_ref()
            .is_some_and(|parameter| parameter.optional);
        let nested = if optional {
            true
        } else if let Some(input) = component.inputs.first().copied().flatten() {
            self.optional_query_parameter_feed(input, column_type, depth + 1)?
                .is_some()
        } else {
            false
        };
        if !nested {
            return Ok(None);
        }
        if component.input_type != Some(column_type) {
            return Err(
                "optional query input scalar type must match its compared column".to_string(),
            );
        }
        Ok(Some(feed))
    }

    pub(super) fn static_query_parameter(
        &self,
        input_key: u32,
        depth: usize,
    ) -> Result<Value, String> {
        if depth >= 12 {
            return Err("query parameter feed contains a cycle".to_string());
        }
        let feed = self
            .edge_from
            .get(&input_key)
            .copied()
            .ok_or_else(|| "query parameter input is not connected".to_string())?;
        let index = self
            .fn_by_output
            .get(&feed)
            .copied()
            .ok_or_else(|| "query parameter is not a compile-time constant".to_string())?;
        let component: &FnComponent = &self.fn_components[index];
        if component.name == "constant" {
            let (value, datatype) = component
                .constant
                .as_ref()
                .ok_or_else(|| "constant query parameter has no value".to_string())?;
            return Ok(parse_constant(value, datatype));
        }
        if is_input(component) {
            let transparent_input = component.inputs.first().copied().flatten();
            return match transparent_input {
                Some(input) if self.edge_from.contains_key(&input) => {
                    self.static_query_parameter(input, depth + 1)
                }
                _ => component.input_preview.clone().ok_or_else(|| {
                    "query parameter input has neither an upstream value nor an enabled preview value"
                        .to_string()
                }),
            };
        }
        Err(format!(
            "query parameter uses dynamic function `{}`; only literal constants are supported",
            component.name
        ))
    }
}

pub(super) fn coerce_value(value: Value, ty: ScalarType) -> Result<Value, String> {
    match (value, ty) {
        (Value::String(value), ScalarType::String) => Ok(Value::String(value)),
        (Value::Bool(value), ScalarType::Bool) => Ok(Value::Bool(value)),
        (Value::Int(value), ScalarType::Int) => Ok(Value::Int(value)),
        (Value::Float(value), ScalarType::Int)
            if value.is_finite()
                && value.fract() == 0.0
                && value >= i64::MIN as f64
                && value < -(i64::MIN as f64) =>
        {
            Ok(Value::Int(value as i64))
        }
        (Value::Int(value), ScalarType::Float)
            if (-9_007_199_254_740_992..=9_007_199_254_740_992).contains(&value) =>
        {
            Ok(Value::Float(value as f64))
        }
        (Value::Float(value), ScalarType::Float) if value.is_finite() => Ok(Value::Float(value)),
        (Value::String(value), ScalarType::Int) => value
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|_| "query operand is not an integer".to_string()),
        (Value::String(value), ScalarType::Float) => value
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(Value::Float)
            .ok_or_else(|| "query operand is not a finite number".to_string()),
        (Value::Null | Value::JsonNull(_), _) => Err("query parameters cannot be null".to_string()),
        (value, expected) => Err(format!(
            "query operand has type {}, expected {expected:?}",
            value.type_name()
        )),
    }
}
