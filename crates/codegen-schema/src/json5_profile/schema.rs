use std::collections::BTreeSet;

use ir::{
    GroupAlternativeMode, ScalarType, SchemaKind, SchemaNode, XmlAlternativeKind,
    XmlWildcardProcessContents,
};

use super::{
    Json5ProfileError, Json5ProfileResource, Json5ProfileSummary, Json5RequiredKind,
    MAX_LOGICAL_LEVELS, MAX_NAME_BYTES, MAX_SCHEMA_NODES, MAX_TOTAL_NAME_BYTES, add, limit,
};

pub(super) fn validate(root: &SchemaNode) -> Result<Json5ProfileSummary, Json5ProfileError> {
    if !matches!(
        &root.kind,
        SchemaKind::Group {
            children: _,
            alternatives: _,
            required: _,
            xml_restricted_alternatives: _,
            dynamic: _
        }
    ) {
        return Err(Json5ProfileError::RootObjectRequired);
    }
    let mut summary = Json5ProfileSummary {
        nodes: 0,
        logical_levels: 0,
        name_bytes: 0,
    };
    let mut pending = vec![(root, 1usize)];
    while let Some((node, depth)) = pending.pop() {
        add(
            &mut summary.nodes,
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
        summary.logical_levels = summary.logical_levels.max(depth);
        name(&node.name, &mut summary.name_bytes)?;
        metadata(node)?;
        match &node.kind {
            SchemaKind::Scalar { ty } => match ty {
                ScalarType::String | ScalarType::Int | ScalarType::Float | ScalarType::Bool => {}
            },
            SchemaKind::ScalarUnion { types: _ } => return Err(unsupported("kind.scalar_union")),
            SchemaKind::Group {
                children,
                alternatives,
                required,
                xml_restricted_alternatives,
                dynamic,
            } => {
                if node.nullable {
                    return Err(unsupported("nullable_group"));
                }
                if !alternatives.is_empty() {
                    return Err(unsupported("kind.alternatives"));
                }
                if !xml_restricted_alternatives.is_empty() {
                    return Err(unsupported("kind.xml_restricted_alternatives"));
                }
                if dynamic.is_some() {
                    return Err(unsupported("kind.dynamic"));
                }
                if required.len() > children.len() {
                    return Err(Json5ProfileError::InvalidRequiredName {
                        kind: Json5RequiredKind::TooManyNames,
                        name: None,
                    });
                }
                let requested = summary
                    .nodes
                    .saturating_add(pending.len())
                    .saturating_add(children.len());
                if requested > MAX_SCHEMA_NODES {
                    return Err(limit(
                        Json5ProfileResource::SchemaNodes,
                        requested,
                        MAX_SCHEMA_NODES,
                    ));
                }
                for item in required {
                    name(item, &mut summary.name_bytes)?;
                }
                for child in children.iter().rev() {
                    pending.push((child, depth + 1));
                }
            }
        }
    }
    // All node/name/required sizes are established before bounded set allocation.
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        match &node.kind {
            SchemaKind::Group {
                children,
                alternatives: _,
                required,
                xml_restricted_alternatives: _,
                dynamic: _,
            } => {
                let mut names = BTreeSet::new();
                for child in children {
                    if !names.insert(child.name.as_str()) {
                        return Err(Json5ProfileError::DuplicateChildName {
                            name: child.name.clone(),
                        });
                    }
                }
                let mut required_names = BTreeSet::new();
                for item in required {
                    let kind = if item.is_empty() {
                        Some(Json5RequiredKind::EmptyName)
                    } else if !required_names.insert(item.as_str()) {
                        Some(Json5RequiredKind::DuplicateName)
                    } else if !names.contains(item.as_str()) {
                        Some(Json5RequiredKind::UndeclaredName)
                    } else {
                        None
                    };
                    if let Some(kind) = kind {
                        return Err(Json5ProfileError::InvalidRequiredName {
                            kind,
                            name: Some(item.clone()),
                        });
                    }
                }
                pending.extend(children);
            }
            SchemaKind::Scalar { ty: _ } | SchemaKind::ScalarUnion { types: _ } => {}
        }
    }
    Ok(summary)
}

fn name(value: &str, total: &mut usize) -> Result<(), Json5ProfileError> {
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

fn unsupported(field: &'static str) -> Json5ProfileError {
    Json5ProfileError::UnsupportedMetadata { field }
}

fn metadata(node: &SchemaNode) -> Result<(), Json5ProfileError> {
    // No wildcard: a future compiled schema field requires an explicit policy decision.
    let SchemaNode {
        name: _,
        xml_namespace,
        xml_name_alternatives,
        xml_wildcard_namespace,
        xml_wildcard_process_contents,
        repeating,
        recursive_ref,
        attribute,
        text,
        nillable,
        xml_optional,
        xml_attribute_required,
        nullable: _,
        container_nullable,
        json_any,
        fixed,
        json_allowed_values,
        numeric_range,
        json_multiple_of,
        item_count_range,
        json_contains,
        json_dependent_schemas,
        property_count_range,
        json_property_dependencies,
        json_pattern_property_names,
        json_property_names,
        json_unique_items,
        string_length_range,
        json_patterns,
        json_formats,
        default,
        value_generation,
        alternative_mode,
        xml_alternative_kind,
        xml_type_alternatives,
        xml_default_type,
        xml_repeating_sequences,
        xml_repeating_choices,
        database_relation,
        kind: _,
    } = node;
    macro_rules! none { ($($field:ident),+ $(,)?) => { $(if $field.is_some() { return Err(unsupported(stringify!($field))); })+ }; }
    macro_rules! empty { ($($field:ident),+ $(,)?) => { $(if !$field.is_empty() { return Err(unsupported(stringify!($field))); })+ }; }
    macro_rules! off { ($($field:ident),+ $(,)?) => { $(if *$field { return Err(unsupported(stringify!($field))); })+ }; }
    none!(
        xml_namespace,
        xml_wildcard_namespace,
        recursive_ref,
        fixed,
        json_allowed_values,
        numeric_range,
        json_multiple_of,
        item_count_range,
        json_contains,
        json_dependent_schemas,
        property_count_range,
        json_property_dependencies,
        json_pattern_property_names,
        json_property_names,
        string_length_range,
        json_patterns,
        default,
        value_generation,
        xml_default_type,
        database_relation
    );
    empty!(
        xml_name_alternatives,
        xml_repeating_sequences,
        xml_repeating_choices,
        json_formats
    );
    off!(
        repeating,
        attribute,
        text,
        nillable,
        xml_optional,
        xml_attribute_required,
        container_nullable,
        json_any,
        json_unique_items,
        xml_type_alternatives
    );
    if *xml_wildcard_process_contents != XmlWildcardProcessContents::Skip {
        return Err(unsupported("xml_wildcard_process_contents"));
    }
    if *alternative_mode != GroupAlternativeMode::Exclusive {
        return Err(unsupported("alternative_mode"));
    }
    if *xml_alternative_kind != XmlAlternativeKind::XsiType {
        return Err(unsupported("xml_alternative_kind"));
    }
    Ok(())
}
