use serde_json::Value;

#[cfg(test)]
mod duplicates;
mod keys;

use super::{
    Json5ProfileError, Json5ProfileResource, Json5RequiredKind, MAX_DESCRIPTOR_BYTES,
    MAX_LOGICAL_LEVELS, MAX_NAME_BYTES, MAX_SCHEMA_NODES, MAX_TOTAL_NAME_BYTES, add, limit,
};

pub(super) fn validate(descriptor: &str) -> Result<(), Json5ProfileError> {
    if descriptor.len() > MAX_DESCRIPTOR_BYTES {
        return Err(limit(
            Json5ProfileResource::DescriptorBytes,
            descriptor.len(),
            MAX_DESCRIPTOR_BYTES,
        ));
    }
    let payload = if let Some(payload) = descriptor.strip_prefix(crate::V2_PREFIX) {
        payload
    } else if descriptor.starts_with("FERRULE-EMBEDDED-SCHEMA/") {
        return Err(Json5ProfileError::Codec(
            crate::CodecError::UnsupportedVersion,
        ));
    } else {
        descriptor
    };
    // The unchanged strict reader's recursion limit remains in force here.
    let raw: Value = serde_json::from_str(payload).map_err(Json5ProfileError::DescriptorSyntax)?;
    // Preserve complete strict parse/depth/error precedence, then refuse
    // duplicate decoded identities in the original text before accepting raw.
    keys::validate(payload)?;
    let mut pending = vec![(&raw, 1usize)];
    let mut nodes = 0usize;
    let mut names = 0usize;
    while let Some((node, depth)) = pending.pop() {
        add(
            &mut nodes,
            1,
            Json5ProfileResource::SchemaNodes,
            MAX_SCHEMA_NODES,
        )?;
        if depth > MAX_LOGICAL_LEVELS {
            return Err(limit(
                Json5ProfileResource::LogicalLevels,
                depth,
                MAX_LOGICAL_LEVELS,
            ));
        }
        let object = node
            .as_object()
            .ok_or(Json5ProfileError::InvalidDescriptorShape { field: "node" })?;
        for (field, value) in object {
            // Refusal payloads cannot grow beyond the admitted name limit.
            if field.len() > MAX_NAME_BYTES {
                return Err(limit(
                    Json5ProfileResource::NameLength,
                    field.len(),
                    MAX_NAME_BYTES,
                ));
            }
            match field.as_str() {
                "name" | "kind" | "nullable" => {}
                "xml_namespace"
                | "xml_wildcard_namespace"
                | "recursive_ref"
                | "fixed"
                | "json_allowed_values"
                | "numeric_range"
                | "json_multiple_of"
                | "item_count_range"
                | "json_contains"
                | "json_dependent_schemas"
                | "property_count_range"
                | "json_property_dependencies"
                | "json_pattern_property_names"
                | "json_property_names"
                | "string_length_range"
                | "json_patterns"
                | "default"
                | "value_generation"
                | "xml_default_type"
                | "database_relation" => default(value.is_null(), field)?,
                "repeating"
                | "attribute"
                | "text"
                | "nillable"
                | "xml_optional"
                | "xml_attribute_required"
                | "container_nullable"
                | "json_any"
                | "json_unique_items"
                | "xml_type_alternatives" => default(value.as_bool() == Some(false), field)?,
                "xml_name_alternatives"
                | "xml_repeating_sequences"
                | "xml_repeating_choices"
                | "json_formats" => default(value.as_array().is_some_and(Vec::is_empty), field)?,
                "xml_wildcard_process_contents" => default(value.as_str() == Some("skip"), field)?,
                "alternative_mode" => default(value.as_str() == Some("exclusive"), field)?,
                "xml_alternative_kind" => default(value.as_str() == Some("xsi_type"), field)?,
                _ => {
                    return Err(Json5ProfileError::UnknownDescriptorField {
                        field: field.clone(),
                    });
                }
            }
        }
        count_name(
            object
                .get("name")
                .and_then(Value::as_str)
                .ok_or(Json5ProfileError::InvalidDescriptorShape { field: "name" })?,
            &mut names,
        )?;
        let kind = object
            .get("kind")
            .and_then(Value::as_object)
            .ok_or(Json5ProfileError::InvalidDescriptorShape { field: "kind" })?;
        let tag = kind
            .get("kind")
            .and_then(Value::as_str)
            .ok_or(Json5ProfileError::InvalidDescriptorShape { field: "kind.kind" })?;
        let allowed: &[&str] = match tag {
            "group" => &[
                "kind",
                "children",
                "alternatives",
                "required",
                "xml_restricted_alternatives",
                "dynamic",
            ],
            "scalar" => &["kind", "ty"],
            _ => return Err(Json5ProfileError::UnsupportedMetadata { field: "kind" }),
        };
        if depth == 1 && tag != "group" {
            return Err(Json5ProfileError::RootObjectRequired);
        }
        for field in kind.keys() {
            if field.len() > MAX_NAME_BYTES {
                return Err(limit(
                    Json5ProfileResource::NameLength,
                    field.len(),
                    MAX_NAME_BYTES,
                ));
            }
            if !allowed.contains(&field.as_str()) {
                return Err(Json5ProfileError::UnknownDescriptorField {
                    field: field.clone(),
                });
            }
        }
        if tag == "scalar" {
            continue;
        }
        let children = kind.get("children").and_then(Value::as_array).ok_or(
            Json5ProfileError::InvalidDescriptorShape {
                field: "kind.children",
            },
        )?;
        for field in ["alternatives", "xml_restricted_alternatives"] {
            if let Some(value) = kind.get(field) {
                default(value.as_array().is_some_and(Vec::is_empty), field)?;
            }
        }
        if let Some(value) = kind.get("dynamic") {
            default(value.is_null(), "dynamic")?;
        }
        if let Some(value) = kind.get("required") {
            let required = value
                .as_array()
                .ok_or(Json5ProfileError::InvalidDescriptorShape {
                    field: "kind.required",
                })?;
            if required.len() > children.len() {
                return Err(Json5ProfileError::InvalidRequiredName {
                    kind: Json5RequiredKind::TooManyNames,
                    name: None,
                });
            }
            for item in required {
                count_name(
                    item.as_str()
                        .ok_or(Json5ProfileError::InvalidDescriptorShape {
                            field: "kind.required.name",
                        })?,
                    &mut names,
                )?;
            }
        }
        let requested = nodes
            .saturating_add(pending.len())
            .saturating_add(children.len());
        if requested > MAX_SCHEMA_NODES {
            return Err(limit(
                Json5ProfileResource::SchemaNodes,
                requested,
                MAX_SCHEMA_NODES,
            ));
        }
        for child in children.iter().rev() {
            pending.push((child, depth + 1));
        }
    }
    Ok(())
}

fn count_name(value: &str, total: &mut usize) -> Result<(), Json5ProfileError> {
    if value.len() > MAX_NAME_BYTES {
        return Err(limit(
            Json5ProfileResource::NameLength,
            value.len(),
            MAX_NAME_BYTES,
        ));
    }
    add(
        total,
        value.len(),
        Json5ProfileResource::NameBytes,
        MAX_TOTAL_NAME_BYTES,
    )
}

fn default(accepted: bool, field: &str) -> Result<(), Json5ProfileError> {
    if accepted {
        Ok(())
    } else {
        // Only a known metadata key is retained, never the input value/document.
        Err(Json5ProfileError::UnsupportedDescriptorMetadata {
            field: field.to_owned(),
        })
    }
}
