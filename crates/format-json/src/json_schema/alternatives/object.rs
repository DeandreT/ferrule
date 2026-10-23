//! Object `anyOf`/`oneOf` alternatives, their discriminators and merges.

use super::*;

pub(in crate::json_schema) fn parse_object_alternatives(
    name: &str,
    schema: &serde_json::Value,
    alternatives: &serde_json::Value,
    mode: GroupAlternativeMode,
    doc: &super::files::LoadedSchema,
    active_refs: &mut Vec<String>,
) -> Result<SchemaNode, JsonFormatError> {
    let keyword = match mode {
        GroupAlternativeMode::Exclusive => "oneOf",
        GroupAlternativeMode::Inclusive => "anyOf",
    };
    let alternatives = alternatives
        .as_array()
        .filter(|alternatives| alternatives.len() >= 2)
        .ok_or_else(|| {
            unsupported_union(
                name,
                &format!("{keyword} must contain at least two alternatives"),
            )
        })?;
    let mut base_children = parse_alternative_properties(schema, doc, active_refs)?;
    let base_required = required_names(schema);
    let base_property_count = property_counts::selected(name, schema)?;
    let base_property_names = property_names::selected(name, schema, doc, active_refs)?;
    let base_additional = match schema.get("additionalProperties") {
        None | Some(serde_json::Value::Bool(true)) => WrapperAdditional::Open,
        Some(serde_json::Value::Bool(false)) => WrapperAdditional::Closed,
        Some(additional @ serde_json::Value::Object(_)) => {
            WrapperAdditional::Typed(Box::new(parse("*", additional, doc, active_refs)?))
        }
        Some(_) => {
            return Err(unsupported_union(
                name,
                "alternative wrapper additionalProperties must be a boolean or schema",
            ));
        }
    };
    let base_constraints = scalar_constraints(name, schema, &base_children)?;
    clear_constrained_fixed(&mut base_children, &base_constraints);
    let mut merged = base_children.clone();
    let mut metadata = Vec::with_capacity(alternatives.len());
    let mut common_property_count = None;
    let mut saw_property_count = false;
    let mut common_property_dependencies = None;
    let mut saw_property_dependencies = false;
    let mut common_dependent_schemas = None;
    let mut saw_dependent_schemas = false;
    let mut common_property_names = None;
    let mut saw_property_names = false;
    for (index, alternative_schema) in alternatives.iter().enumerate() {
        if alternative_schema
            .get("$ref")
            .and_then(serde_json::Value::as_str)
            .is_some()
            && files::ref_siblings_apply(alternative_schema)
        {
            reject_unsupported_ref_siblings(name, alternative_schema)?;
        }
        if alternative_schema
            .get("$ref")
            .and_then(serde_json::Value::as_str)
            .is_none()
        {
            formats::validate(name, alternative_schema)?;
            patterns::validate_ignored(name, alternative_schema)?;
        }
        let resolved = alternative_schema
            .get("$ref")
            .and_then(serde_json::Value::as_str)
            .and_then(|reference| resolve_ref(doc, reference))
            .unwrap_or(alternative_schema);
        if !core::ptr::eq(resolved, alternative_schema) {
            formats::validate(name, resolved)?;
            patterns::validate_ignored(name, resolved)?;
        }
        let alternative_name = files::ref_siblings_apply(alternative_schema)
            .then(|| {
                alternative_schema
                    .get("title")
                    .and_then(serde_json::Value::as_str)
            })
            .flatten()
            .or_else(|| resolved.get("title").and_then(serde_json::Value::as_str))
            .map(str::to_string)
            .or_else(|| {
                alternative_schema
                    .get("$ref")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|reference| reference.rsplit('/').next())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| format!("{keyword}{index}"));
        let normalized = without_direct_property_constraints(resolved);
        let mut parsed = parse(&alternative_name, &normalized, doc, active_refs)?;
        super::super::pattern_properties::reject_composed_node(name, &parsed, keyword)?;
        if !core::ptr::eq(resolved, alternative_schema)
            && files::ref_siblings_apply(alternative_schema)
        {
            dependent_schemas::apply(
                name,
                alternative_schema,
                &mut parsed,
                doc,
                active_refs,
                false,
            )?;
        }
        let sibling_property_count = if !core::ptr::eq(resolved, alternative_schema)
            && files::ref_siblings_apply(alternative_schema)
        {
            property_counts::selected(name, alternative_schema)?
        } else {
            None
        };
        let branch_property_count =
            property_counts::intersect(name, parsed.property_count_range, sibling_property_count)?;
        let effective_property_count =
            property_counts::intersect(name, base_property_count, branch_property_count)?;
        if saw_property_count && common_property_count != effective_property_count {
            return Err(unsupported_union(
                name,
                &format!(
                    "{keyword} object alternatives have differing property-count constraints that cannot be represented independently"
                ),
            ));
        }
        common_property_count = effective_property_count;
        saw_property_count = true;
        let sibling_property_dependencies = if !core::ptr::eq(resolved, alternative_schema)
            && files::ref_siblings_apply(alternative_schema)
        {
            property_dependencies::selected(name, alternative_schema)?
        } else {
            None
        };
        let possible_triggers = match &parsed.kind {
            SchemaKind::Group { children, .. } => children
                .iter()
                .map(|child| child.name.as_str())
                .collect::<Vec<_>>(),
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => Vec::new(),
        };
        let sibling_property_dependencies = property_dependencies::retain_possible_triggers(
            name,
            sibling_property_dependencies.as_ref(),
            possible_triggers.iter().copied(),
        )?;
        let branch_property_dependencies = property_dependencies::intersect(
            name,
            parsed.json_property_dependencies.as_ref(),
            sibling_property_dependencies.as_ref(),
        )?;
        if saw_property_dependencies && common_property_dependencies != branch_property_dependencies
        {
            return Err(unsupported_union(
                name,
                &format!(
                    "{keyword} object alternatives have differing property dependencies that cannot be represented independently"
                ),
            ));
        }
        common_property_dependencies = branch_property_dependencies;
        saw_property_dependencies = true;
        let branch_dependent_schemas = parsed.json_dependent_schemas.clone();
        if saw_dependent_schemas {
            if !equivalent_dependent_schemas(
                common_dependent_schemas.as_ref(),
                branch_dependent_schemas.as_ref(),
            ) {
                return Err(unsupported_union(
                    name,
                    &format!(
                        "{keyword} object alternatives have differing dependent schemas that cannot be represented independently"
                    ),
                ));
            }
        } else {
            common_dependent_schemas = branch_dependent_schemas;
            saw_dependent_schemas = true;
        }
        let sibling_property_names = if !core::ptr::eq(resolved, alternative_schema)
            && files::ref_siblings_apply(alternative_schema)
        {
            property_names::selected(name, alternative_schema, doc, active_refs)?
        } else {
            None
        };
        let branch_property_names = property_names::intersect(
            name,
            parsed.json_property_names.clone(),
            sibling_property_names,
        )?;
        let effective_property_names =
            property_names::intersect(name, base_property_names.clone(), branch_property_names)?;
        if saw_property_names && common_property_names != effective_property_names {
            return Err(unsupported_union(
                name,
                &format!(
                    "{keyword} object alternatives have differing propertyNames constraints that cannot be represented independently"
                ),
            ));
        }
        common_property_names = effective_property_names;
        saw_property_names = true;
        if parsed.repeating {
            return Err(unsupported_union(
                name,
                "array alternatives are not supported",
            ));
        }
        let nested_mode = parsed.alternative_mode();
        let SchemaKind::Group {
            children: mut variant_children,
            alternatives: nested_alternatives,
            ..
        } = parsed.kind
        else {
            return Err(unsupported_union(
                name,
                "only object alternatives are supported",
            ));
        };
        if !nested_alternatives.is_empty() {
            if nested_mode != mode && !alternatives_are_pairwise_disjoint(&nested_alternatives) {
                return Err(unsupported_union(
                    name,
                    "cross-mode nested object alternatives must be provably mutually exclusive",
                ));
            }
            for mut nested in nested_alternatives {
                nested.name = format!("{alternative_name}/{}", nested.name);
                merge_nested_alternative(
                    name,
                    mode,
                    &base_children,
                    &base_required,
                    &base_constraints,
                    &base_additional,
                    &variant_children,
                    nested,
                    &mut merged,
                    &mut metadata,
                )?;
            }
            continue;
        }
        if resolved.get("additionalProperties") != Some(&serde_json::Value::Bool(false)) {
            return Err(unsupported_union(
                name,
                "object alternatives must declare additionalProperties false",
            ));
        }
        let mut required = base_required.clone();
        for field in required_names(resolved) {
            if !required.contains(&field) {
                required.push(field);
            }
        }
        let constraints = scalar_constraints(name, resolved, &variant_children)?;
        clear_constrained_fixed(&mut variant_children, &constraints);
        let constraints = merge_constraints(name, &base_constraints, constraints)?;
        let mut members = Vec::new();
        for child in variant_children {
            if let Some(base) = base_children.iter().find(|base| base.name == child.name)
                && base != &child
            {
                return Err(unsupported_union(
                    name,
                    &format!(
                        "field `{}` has incompatible wrapper and alternative schemas",
                        child.name
                    ),
                ));
            }
            let allowed = wrapper_allows(name, &base_children, &base_additional, &child)?;
            if allowed {
                if let Some(existing) = merged
                    .iter_mut()
                    .find(|existing| existing.name == child.name)
                {
                    if existing != &child
                        && !merge_exact_constrained_nullability(
                            existing,
                            &child,
                            &constraints,
                            &metadata,
                        )
                    {
                        return Err(unsupported_union(
                            name,
                            &format!(
                                "field `{}` has incompatible schemas across alternatives",
                                child.name
                            ),
                        ));
                    }
                } else {
                    merged.push(child.clone());
                }
                if !members.contains(&child.name) {
                    members.push(child.name);
                }
            }
        }
        push_alternative(
            name,
            mode,
            GroupAlternative {
                name: alternative_name,
                members,
                required,
                constraints,
            },
            &mut metadata,
        )?;
    }
    merged.retain(|child| {
        metadata
            .iter()
            .any(|alternative| alternative.members.contains(&child.name))
    });
    let group = SchemaNode::group(name, merged);
    let mut group = match mode {
        GroupAlternativeMode::Exclusive => group.with_alternatives(metadata),
        GroupAlternativeMode::Inclusive => group.with_inclusive_alternatives(metadata),
    }
    .ok_or_else(|| unsupported_union(name, "alternative metadata is internally inconsistent"))?;
    group.property_count_range = common_property_count;
    group.json_property_dependencies = common_property_dependencies;
    group.json_dependent_schemas = common_dependent_schemas;
    group.json_property_names = common_property_names;
    property_counts::ensure_feasible(name, &group)?;
    property_dependencies::ensure_feasible(name, &group)?;
    if !group.json_property_names_are_valid() || !group.property_count_range_is_valid() {
        return Err(unsupported_union(
            name,
            "object alternative propertyNames constraints reject required properties or conflict with cardinality",
        ));
    }
    Ok(group)
}

fn equivalent_dependent_schemas(
    left: Option<&ir::JsonDependentSchemaConstraints>,
    right: Option<&ir::JsonDependentSchemaConstraints>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.semantically_equals(right),
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    }
}

fn merge_exact_constrained_nullability(
    existing: &mut SchemaNode,
    incoming: &SchemaNode,
    incoming_constraints: &[GroupAlternativeConstraint],
    previous: &[GroupAlternative],
) -> bool {
    let (Ok(existing_null), Ok(incoming_null)) = (
        crate::null_layers::value_null(existing),
        crate::null_layers::value_null(incoming),
    ) else {
        return false;
    };
    if existing_null == incoming_null
        || !incoming_constraints
            .iter()
            .any(|constraint| constraint.member == incoming.name)
        || previous.iter().any(|alternative| {
            alternative.members.contains(&incoming.name)
                && !alternative
                    .constraints
                    .iter()
                    .any(|constraint| constraint.member == incoming.name)
        })
    {
        return false;
    }
    let mut normalized_existing = existing.clone();
    normalized_existing.nullable = false;
    let mut normalized_incoming = incoming.clone();
    normalized_incoming.nullable = false;
    if normalized_existing != normalized_incoming {
        return false;
    }
    existing.nullable = true;
    true
}

fn merge_constraints(
    union_name: &str,
    base: &[GroupAlternativeConstraint],
    nested: Vec<GroupAlternativeConstraint>,
) -> Result<Vec<GroupAlternativeConstraint>, JsonFormatError> {
    let mut merged = base.to_vec();
    for constraint in nested {
        if let Some(previous) = merged
            .iter()
            .find(|previous| previous.member == constraint.member)
        {
            if previous.value != constraint.value {
                return Err(unsupported_union(
                    union_name,
                    &format!(
                        "const discriminator `{}` conflicts with its wrapper constraint",
                        constraint.member
                    ),
                ));
            }
        } else {
            merged.push(constraint);
        }
    }
    Ok(merged)
}

fn wrapper_allows(
    union_name: &str,
    base_children: &[SchemaNode],
    additional: &WrapperAdditional,
    child: &SchemaNode,
) -> Result<bool, JsonFormatError> {
    if base_children.iter().any(|base| base.name == child.name) {
        return Ok(true);
    }
    match additional {
        WrapperAdditional::Open => Ok(true),
        WrapperAdditional::Closed => Ok(false),
        WrapperAdditional::Typed(expected) => {
            let mut expected = expected.as_ref().clone();
            expected.name = child.name.clone();
            if expected == *child {
                Ok(true)
            } else {
                Err(unsupported_union(
                    union_name,
                    &format!(
                        "field `{}` does not match the alternative wrapper's typed additionalProperties schema",
                        child.name
                    ),
                ))
            }
        }
    }
}

fn alternatives_are_pairwise_disjoint(alternatives: &[GroupAlternative]) -> bool {
    alternatives.iter().enumerate().all(|(index, left)| {
        alternatives[index + 1..]
            .iter()
            .all(|right| alternatives_are_disjoint(left, right))
    })
}

fn alternatives_are_disjoint(left: &GroupAlternative, right: &GroupAlternative) -> bool {
    left.required
        .iter()
        .any(|required| !right.members.contains(required))
        || right
            .required
            .iter()
            .any(|required| !left.members.contains(required))
        || left.constraints.iter().any(|left_constraint| {
            right.constraints.iter().any(|right_constraint| {
                left_constraint.member == right_constraint.member
                    && left_constraint.value != right_constraint.value
                    && (left.required.contains(&left_constraint.member)
                        || right.required.contains(&right_constraint.member))
            })
        })
}

#[allow(clippy::too_many_arguments)]
fn merge_nested_alternative(
    union_name: &str,
    mode: GroupAlternativeMode,
    base_children: &[SchemaNode],
    base_required: &[String],
    base_constraints: &[GroupAlternativeConstraint],
    base_additional: &WrapperAdditional,
    variant_children: &[SchemaNode],
    alternative: GroupAlternative,
    merged: &mut Vec<SchemaNode>,
    metadata: &mut Vec<GroupAlternative>,
) -> Result<(), JsonFormatError> {
    let mut members = Vec::new();
    for member in &alternative.members {
        let child = variant_children
            .iter()
            .find(|child| child.name == *member)
            .ok_or_else(|| {
                unsupported_union(
                    union_name,
                    &format!("nested union member `{member}` has no declared field"),
                )
            })?;
        if let Some(base) = base_children.iter().find(|base| base.name == child.name)
            && base != child
        {
            return Err(unsupported_union(
                union_name,
                &format!(
                    "field `{}` has incompatible wrapper and alternative schemas",
                    child.name
                ),
            ));
        }
        let allowed = wrapper_allows(union_name, base_children, base_additional, child)?;
        if !allowed {
            continue;
        }
        if let Some(existing) = merged.iter().find(|existing| existing.name == child.name) {
            if existing != child {
                return Err(unsupported_union(
                    union_name,
                    &format!(
                        "field `{}` has incompatible schemas across alternatives",
                        child.name
                    ),
                ));
            }
        } else {
            merged.push(child.clone());
        }
        if !members.contains(member) {
            members.push(member.clone());
        }
    }
    let mut required = base_required.to_vec();
    for member in alternative.required {
        if !required.contains(&member) {
            required.push(member);
        }
    }
    let constraints = merge_constraints(union_name, base_constraints, alternative.constraints)?;
    push_alternative(
        union_name,
        mode,
        GroupAlternative {
            name: alternative.name,
            members,
            required,
            constraints,
        },
        metadata,
    )
}

fn push_alternative(
    union_name: &str,
    mode: GroupAlternativeMode,
    alternative: GroupAlternative,
    metadata: &mut Vec<GroupAlternative>,
) -> Result<(), JsonFormatError> {
    let keyword = match mode {
        GroupAlternativeMode::Exclusive => "oneOf",
        GroupAlternativeMode::Inclusive => "anyOf",
    };
    if alternative
        .required
        .iter()
        .any(|field| !alternative.members.iter().any(|member| member == field))
    {
        return Err(unsupported_union(
            union_name,
            &format!("{keyword} requires a field not declared by that object alternative"),
        ));
    }
    if mode == GroupAlternativeMode::Exclusive
        && metadata.iter().any(|previous| {
            previous.members == alternative.members
                && previous.required == alternative.required
                && previous.constraints == alternative.constraints
        })
    {
        return Err(unsupported_union(
            union_name,
            "alternatives are not distinguishable by supported object fields and requirements",
        ));
    }
    if metadata
        .iter()
        .any(|previous| previous.name == alternative.name)
    {
        return Err(unsupported_union(
            union_name,
            &format!("{keyword} alternatives must have distinct names"),
        ));
    }
    metadata.push(alternative);
    Ok(())
}

fn scalar_constraints(
    union_name: &str,
    schema: &serde_json::Value,
    children: &[SchemaNode],
) -> Result<Vec<GroupAlternativeConstraint>, JsonFormatError> {
    let Some(properties) = schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
    else {
        return Ok(Vec::new());
    };
    properties
        .iter()
        .map(|(member, property)| {
            discriminator_value(union_name, member, property)
                .map(|value| value.map(|value| (member, value)))
        })
        .filter_map(Result::transpose)
        .map(|result| {
            let (member, value) = result?;
            let child = children
                .iter()
                .find(|child| child.name == *member)
                .ok_or_else(|| {
                    unsupported_union(
                        union_name,
                        &format!("scalar discriminator `{member}` has no declared scalar field"),
                    )
                })?;
            if child.repeating {
                return Err(unsupported_union(
                    union_name,
                    &format!("scalar discriminator `{member}` cannot be an array"),
                ));
            }
            let constraint = match child.kind {
                SchemaKind::Scalar { ty } => {
                    let nullable = crate::null_layers::value_null(child)?;
                    constraint_value(union_name, member, value, ty, nullable)
                }
                SchemaKind::ScalarUnion { types } => {
                    let nullable = crate::null_layers::value_null(child)?;
                    union_constraint_value(union_name, member, value, types, nullable)
                }
                SchemaKind::Group { .. } => Err(unsupported_union(
                    union_name,
                    &format!("scalar discriminator `{member}` must be a scalar field"),
                )),
            };
            let value = constraint?;
            Ok(GroupAlternativeConstraint {
                member: member.clone(),
                value,
            })
        })
        .collect()
}

fn clear_constrained_fixed(
    children: &mut [SchemaNode],
    constraints: &[GroupAlternativeConstraint],
) {
    for constraint in constraints {
        if let Some(child) = children
            .iter_mut()
            .find(|child| child.name == constraint.member)
        {
            child.fixed = None;
        }
    }
}

fn parse_alternative_properties(
    schema: &serde_json::Value,
    doc: &super::files::LoadedSchema,
    active_refs: &mut Vec<String>,
) -> Result<Vec<SchemaNode>, JsonFormatError> {
    schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .map(|properties| {
            properties
                .iter()
                .map(|(name, property)| {
                    parse(name, &without_scalar_constraint(property), doc, active_refs)
                })
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn without_direct_property_constraints(schema: &serde_json::Value) -> serde_json::Value {
    let mut schema = schema.clone();
    let Some(properties) = schema
        .as_object_mut()
        .and_then(|schema| schema.get_mut("properties"))
        .and_then(serde_json::Value::as_object_mut)
    else {
        return schema;
    };
    for property in properties.values_mut() {
        *property = without_scalar_constraint(property);
    }
    schema
}

fn without_scalar_constraint(schema: &serde_json::Value) -> serde_json::Value {
    let mut schema = schema.clone();
    if let Some(schema) = schema.as_object_mut() {
        if !schema.contains_key("type") {
            let value = schema.get("const").or_else(|| {
                schema
                    .get("enum")
                    .and_then(serde_json::Value::as_array)
                    .and_then(|values| values.first().filter(|_| values.len() == 1))
            });
            let inferred = match value {
                Some(serde_json::Value::String(_)) => Some("string"),
                Some(serde_json::Value::Bool(_)) => Some("boolean"),
                Some(serde_json::Value::Number(number)) if number.as_i64().is_some() => {
                    Some("integer")
                }
                Some(serde_json::Value::Number(_)) => Some("number"),
                _ => None,
            };
            if let Some(inferred) = inferred {
                schema.insert("type".into(), inferred.into());
            }
        }
        schema.remove("const");
        schema.remove("enum");
    }
    schema
}

fn discriminator_value<'a>(
    union_name: &str,
    member: &str,
    schema: &'a serde_json::Value,
) -> Result<Option<&'a serde_json::Value>, JsonFormatError> {
    let constant = schema.get("const");
    let values = match schema.get("enum") {
        None => None,
        Some(serde_json::Value::Array(values)) if values.is_empty() => {
            return Err(unsupported_union(
                union_name,
                &format!("enum discriminator `{member}` has no possible values"),
            ));
        }
        Some(serde_json::Value::Array(values)) => Some(values),
        Some(_) => {
            return Err(unsupported_union(
                union_name,
                &format!("enum discriminator `{member}` must be an array"),
            ));
        }
    };
    if let Some(constant) = constant {
        if values.is_some_and(|values| !values.contains(constant)) {
            return Err(unsupported_union(
                union_name,
                &format!(
                    "scalar discriminator `{member}` has incompatible const and enum constraints"
                ),
            ));
        }
        return Ok(Some(constant));
    }
    Ok(values.and_then(|values| (values.len() == 1).then(|| &values[0])))
}

fn union_constraint_value(
    union_name: &str,
    member: &str,
    value: &serde_json::Value,
    types: ScalarTypeSet,
    nullable: bool,
) -> Result<GroupAlternativeConstraintValue, JsonFormatError> {
    let selected = match value {
        serde_json::Value::String(_) if types.contains(ScalarType::String) => ScalarType::String,
        serde_json::Value::Bool(_) if types.contains(ScalarType::Bool) => ScalarType::Bool,
        serde_json::Value::Number(value)
            if value.as_i64().is_some() && types.contains(ScalarType::Int) =>
        {
            ScalarType::Int
        }
        serde_json::Value::Number(_) if types.contains(ScalarType::Float) => ScalarType::Float,
        serde_json::Value::Null if nullable => {
            return Ok(GroupAlternativeConstraintValue::JsonNull);
        }
        _ => {
            return Err(unsupported_union(
                union_name,
                &format!(
                    "scalar discriminator `{member}` does not match any declared scalar union type"
                ),
            ));
        }
    };
    constraint_value(union_name, member, value, selected, nullable)
}

fn constraint_value(
    union_name: &str,
    member: &str,
    value: &serde_json::Value,
    ty: ScalarType,
    nullable: bool,
) -> Result<GroupAlternativeConstraintValue, JsonFormatError> {
    let unsupported = |reason: &str| {
        unsupported_union(
            union_name,
            &format!("scalar discriminator `{member}` {reason}"),
        )
    };
    match (ty, value) {
        (ScalarType::String, serde_json::Value::String(value)) => {
            Ok(GroupAlternativeConstraintValue::String(value.clone()))
        }
        (ScalarType::Int, serde_json::Value::Number(value)) => value
            .as_i64()
            .map(GroupAlternativeConstraintValue::Int)
            .ok_or_else(|| unsupported("must be a signed 64-bit integer")),
        (ScalarType::Float, serde_json::Value::Number(value)) => finite_f64(value)
            .and_then(FiniteF64::new)
            .map(GroupAlternativeConstraintValue::Float)
            .ok_or_else(|| unsupported("must be a finite exactly supported number")),
        (ScalarType::Bool, serde_json::Value::Bool(value)) => {
            Ok(GroupAlternativeConstraintValue::Bool(*value))
        }
        (_, serde_json::Value::Null) if nullable => Ok(GroupAlternativeConstraintValue::JsonNull),
        (_, serde_json::Value::Null) => Err(unsupported(
            "can be null only when its scalar type explicitly includes null",
        )),
        _ => Err(unsupported("does not match its declared scalar type")),
    }
}

fn finite_f64(number: &serde_json::Number) -> Option<f64> {
    crate::exact_f64_from_json_number(number)
}

fn required_names(schema: &serde_json::Value) -> Vec<String> {
    schema
        .get("required")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_string)
        .collect()
}
