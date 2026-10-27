use ir::{ScalarType, SchemaKind, SchemaNode};
use serde::{Deserialize, Serialize};

use crate::{JsonProjectionPlanError, JsonSourceShape};

/// An explicit fan-out view for native grouping keys. Each distinct selected key keeps
/// the complete original member, including its other key occurrences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonGroupingPreparation {
    /// Collection in one projected physical document, including `Root`.
    pub collection: Vec<String>,
    /// Scalar occurrence collection relative to one original member.
    pub keys: Vec<String>,
}

impl JsonGroupingPreparation {
    pub const KEY: &str = "__group_key";
    pub const MEMBER: &str = "__group_member";

    pub fn validate_paths(&self) -> Result<(), JsonProjectionPlanError> {
        for path in [&self.collection, &self.keys] {
            if path.is_empty()
                || path.len() > 64
                || path.iter().any(|s| s.is_empty() || s.contains('\0'))
            {
                return Err(JsonProjectionPlanError::SourceView);
            }
            if path.iter().map(String::len).sum::<usize>() > 65536 {
                return Err(JsonProjectionPlanError::Names);
            }
        }
        if self.collection.first().map(String::as_str) != Some("Root") {
            return Err(JsonProjectionPlanError::SourceView);
        }
        Ok(())
    }

    pub fn validate(&self, shape: &JsonSourceShape) -> Result<(), JsonProjectionPlanError> {
        self.validate_paths()?;
        let schema = shape.document_schema()?;
        let member = at(&schema, &self.collection).ok_or(JsonProjectionPlanError::SourceView)?;
        if !member.repeating || !matches!(member.kind, SchemaKind::Group { .. }) {
            return Err(JsonProjectionPlanError::SourceView);
        }
        let keys = at(member, &self.keys).ok_or(JsonProjectionPlanError::SourceView)?;
        if !keys.repeating
            || !matches!(&keys.kind, SchemaKind::Group { children, .. }
            if children.len() == 1 && children[0].name == "Value"
                && matches!(children[0].kind, SchemaKind::Scalar { ty: ScalarType::String }))
        {
            return Err(JsonProjectionPlanError::SourceView);
        }
        // Number projections also use a String execution field. Admission must
        // inspect the raw selection kind, not just the lowered field type.
        let raw_member =
            raw_at(shape, &self.collection[1..]).ok_or(JsonProjectionPlanError::SourceView)?;
        if !matches!(
            raw_at(raw_member, &self.keys),
            Some(JsonSourceShape::String)
        ) {
            return Err(JsonProjectionPlanError::SourceView);
        }
        Ok(())
    }

    pub fn document_schema(
        &self,
        shape: &JsonSourceShape,
    ) -> Result<SchemaNode, JsonProjectionPlanError> {
        self.validate(shape)?;
        let mut schema = shape.document_schema()?;
        let mut selected = &mut schema;
        for name in &self.collection {
            let SchemaKind::Group { children, .. } = &mut selected.kind else {
                return Err(JsonProjectionPlanError::SourceView);
            };
            selected = children
                .iter_mut()
                .find(|child| child.name == *name)
                .ok_or(JsonProjectionPlanError::SourceView)?;
        }
        let mut member = selected.clone();
        member.name = Self::MEMBER.into();
        member.repeating = false;
        *selected = SchemaNode::group(
            selected.name.clone(),
            vec![SchemaNode::scalar(Self::KEY, ScalarType::String), member],
        )
        .repeating();
        Ok(schema)
    }

    pub fn document_set_schema(
        &self,
        shape: &JsonSourceShape,
    ) -> Result<SchemaNode, JsonProjectionPlanError> {
        let mut document = self.document_schema(shape)?;
        document.name = "Documents".into();
        Ok(SchemaNode::group(
            "JsonDocuments",
            vec![document.repeating()],
        ))
    }
}

fn at<'a>(mut schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    for name in path {
        schema = schema.child(name)?;
    }
    Some(schema)
}

fn raw_at<'a>(mut shape: &'a JsonSourceShape, path: &[String]) -> Option<&'a JsonSourceShape> {
    for name in path {
        shape = match shape {
            JsonSourceShape::Object { properties } => {
                &properties
                    .iter()
                    .find(|property| property.name == *name)?
                    .shape
            }
            JsonSourceShape::Array { item } if name == "Items" => item,
            _ => return None,
        };
    }
    Some(shape)
}
