//! Target scope traversal and construction.

use super::*;

pub(super) fn collect_scopes<'a>(
    scope: &'a TargetScope,
    name: String,
    output: &mut Vec<(String, &'a TargetScope, Vec<String>, Vec<String>)>,
) {
    let child_names = scope
        .children
        .iter()
        .enumerate()
        .map(|(index, _)| format!("{name}_{index}"))
        .collect::<Vec<_>>();
    let dynamic_child_names = match &scope.construction {
        TargetConstruction::DynamicGroup { children, .. } => children
            .iter()
            .enumerate()
            .map(|(index, _)| format!("{name}_dynamic_{index}"))
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let all_child_names = child_names
        .iter()
        .chain(&dynamic_child_names)
        .cloned()
        .collect::<Vec<_>>();
    let segment_names = scope
        .iteration
        .as_ref()
        .and_then(IterationPlan::concatenated)
        .map(|sequence| {
            sequence
                .iter()
                .enumerate()
                .map(|(index, _)| format!("{name}_segment_{index}"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    output.push((name, scope, all_child_names, segment_names.clone()));
    if let Some(sequence) = scope
        .iteration
        .as_ref()
        .and_then(IterationPlan::concatenated)
    {
        for (segment, segment_name) in sequence.iter().zip(segment_names) {
            collect_scopes(segment, segment_name, output);
        }
    }
    for (child, child_name) in scope.children.iter().zip(child_names) {
        collect_scopes(child, child_name, output);
    }
    if let TargetConstruction::DynamicGroup { children, .. } = &scope.construction {
        for (child, child_name) in children.iter().zip(dynamic_child_names) {
            collect_scopes(&child.scope, child_name, output);
        }
    }
}

pub(super) fn render_scope(
    program: &Program,
    name: &str,
    scope: &TargetScope,
    child_names: &[String],
    segment_names: &[String],
) -> Result<String, EmitError> {
    let mut output =
        format!("fn {name}(context: &ScopeContext<'_>) -> Result<Instance, RuntimeError> {{\n");
    output.push_str("    let _ = context;\n");
    if let Some(iteration) = &scope.iteration {
        match iteration.kind() {
            IterationKind::Concatenate(_) => {
                output.push_str(&render_concatenated_scope(
                    program,
                    iteration,
                    segment_names,
                )?);
            }
            IterationKind::OptionalPresence(presence) => {
                output.push_str(&render_presence_scope(
                    scope,
                    presence,
                    child_names,
                    scope_preservation(program, scope),
                ));
            }
            IterationKind::Candidates(candidates) => {
                output.push_str(&render_iteration_scope(
                    program,
                    scope,
                    iteration,
                    candidates,
                    child_names,
                )?);
            }
            IterationKind::Owned(_) => {
                return Err(codegen::unrendered_iteration(program, iteration).into());
            }
        }
    } else {
        output.push_str(&render_scope_item_with_scalar_output(
            scope,
            child_names,
            "    ",
            "context",
            scope_preservation(program, scope),
        ));
        if scope.repeating {
            output.push_str("    Ok(repeated([output]))\n");
        } else {
            output.push_str("    Ok(output)\n");
        }
    }
    output.push_str("}\n\n");
    Ok(output)
}

pub(super) fn render_concatenated_scope(
    program: &Program,
    iteration: &IterationPlan,
    segment_names: &[String],
) -> Result<String, EmitError> {
    let mut output = String::from("    let mut outputs = Vec::new();\n");
    for segment_name in segment_names {
        output.push_str(&format!(
            "    match {segment_name}(context)? {{\n        item @ Instance::Group(_) => outputs.push(item),\n        Instance::Repeated(items) | Instance::MappedSequence(items) => outputs.extend(items),\n        _ => unreachable!(\"validated scope sequence segment output\"),\n    }}\n"
        ));
    }
    match iteration.output() {
        IterationOutput::Repeated => output.push_str("    Ok(repeated(outputs))\n"),
        IterationOutput::MappedSequence => {
            output.push_str("    Ok(Instance::MappedSequence(outputs))\n");
        }
        IterationOutput::First => {
            return Err(codegen::unrendered_iteration(program, iteration).into());
        }
    }
    Ok(output)
}

/// The condition runs once against the parent context, which the constructed
/// group keeps: optional presence adds no candidate frame or position.
pub(super) fn render_presence_scope(
    scope: &TargetScope,
    presence: codegen::OptionalPresence,
    child_names: &[String],
    preserve: Option<ir::XmlScalarPreservation<'_>>,
) -> String {
    let condition = presence.condition();
    let mut output = format!(
        "    if !require_bool({condition}, expression_{condition}(context)?)? {{\n        return Ok(repeated(Vec::new()));\n    }}\n"
    );
    output.push_str(&render_scope_item_with_scalar_output(
        scope,
        child_names,
        "    ",
        "context",
        preserve,
    ));
    output.push_str("    Ok(repeated([output]))\n");
    output
}

/// The validated policy alone selects preserved bindings: the flat profile
/// owns only the root group, the document profile every primary scope.
pub(super) fn scope_preservation<'a>(
    program: &'a Program,
    scope: &TargetScope,
) -> Option<ir::XmlScalarPreservation<'a>> {
    match program.xml_scalar_preservation()? {
        ir::XmlScalarPreservation::RootFields(_) if !std::ptr::eq(scope, &program.root) => None,
        preservation => Some(preservation),
    }
}

pub(super) fn render_scope_item(
    scope: &TargetScope,
    child_names: &[String],
    indent: &str,
    context: &str,
) -> String {
    render_scope_item_with_scalar_output(scope, child_names, indent, context, None)
}

pub(super) fn render_scope_item_with_scalar_output(
    scope: &TargetScope,
    child_names: &[String],
    indent: &str,
    context: &str,
    preserve: Option<ir::XmlScalarPreservation<'_>>,
) -> String {
    if matches!(scope.construction, TargetConstruction::CollectDocuments) {
        let mut output = format!("{indent}let output = codegen_runtime::collect_documents([\n");
        for child in child_names {
            output.push_str(&format!("{indent}    {child}({context})?,\n"));
        }
        output.push_str(&format!("{indent}])?;\n"));
        return output;
    }
    let mut output = String::new();
    if let TargetConstruction::Scalar {
        expression,
        target_domain,
    } = &scope.construction
    {
        let value = render_target_adaptation(
            &format!("expression_{expression}({context})?"),
            *target_domain,
        );
        output.push_str(&format!("{indent}let output = scalar({value});\n"));
        return output;
    }
    if let TargetConstruction::RecursiveFilter {
        children,
        items,
        predicate,
    } = &scope.construction
        && let Some(source) = scope
            .iteration
            .as_ref()
            .and_then(IterationPlan::owned_source_iteration)
    {
        // Only paired Basic scopes render an owned selector: the stored owner
        // is the filter's only owner and the predicate reads paired items.
        let source = source.source();
        let owner = match &source.owner {
            mapping::SourceOwner::Primary => "codegen_runtime::SourceOwnerRef::Primary".to_string(),
            mapping::SourceOwner::Named { name } => {
                format!(
                    "codegen_runtime::SourceOwnerRef::Named({})",
                    rust_string(name)
                )
            }
        };
        output.push_str(&format!(
            "{indent}let output = codegen_runtime::owned_recursive_filter(\n{indent}    {context},\n{indent}    {owner},\n{indent}    &[{}],\n{indent}    {},\n{indent}    {},\n{indent}    {predicate},\n{indent}    expression_{predicate},\n{indent})?;\n",
            render_string_path(&source.path),
            rust_string(children),
            rust_string(items),
        ));
        return output;
    }
    if let TargetConstruction::RecursiveFilter {
        children,
        items,
        predicate,
    } = &scope.construction
    {
        output.push_str(&format!(
            "{indent}let output = recursive_filter(\n{indent}    {context},\n{indent}    {},\n{indent}    {},\n{indent}    {predicate},\n{indent}    expression_{predicate},\n{indent})?;\n",
            rust_string(children),
            rust_string(items),
        ));
        return output;
    }
    if let TargetConstruction::PathHierarchy {
        collection,
        separator,
        directories,
        files,
        name,
    } = &scope.construction
    {
        let collection = render_string_path(collection);
        output.push_str(&format!(
            "{indent}let output = path_hierarchy(\n{indent}    {context},\n{indent}    &[{collection}],\n{indent}    {},\n{indent}    {},\n{indent}    {},\n{indent}    {},\n{indent})?;\n",
            rust_string(separator),
            rust_string(directories),
            rust_string(files),
            rust_string(name),
        ));
        return output;
    }
    if let TargetConstruction::OwnedPathHierarchy {
        collection,
        separator,
        directories,
        files,
        name,
    } = &scope.construction
    {
        // Only paired Basic scopes render this construction; the owner is the
        // exact stored endpoint and the field names describe output only.
        let owner = match &collection.owner {
            mapping::SourceOwner::Primary => "codegen_runtime::SourceOwnerRef::Primary".to_string(),
            mapping::SourceOwner::Named { name } => {
                format!(
                    "codegen_runtime::SourceOwnerRef::Named({})",
                    rust_string(name)
                )
            }
        };
        output.push_str(&format!(
            "{indent}let output = codegen_runtime::owned_path_hierarchy(\n{indent}    {context},\n{indent}    {owner},\n{indent}    &[{}],\n{indent}    {},\n{indent}    {},\n{indent}    {},\n{indent}    {},\n{indent})?;\n",
            render_string_path(&collection.path),
            rust_string(separator),
            rust_string(directories),
            rust_string(files),
            rust_string(name),
        ));
        return output;
    }
    if let TargetConstruction::AdjacencyTree {
        collection,
        key,
        parent,
        target_key,
        target_children,
        root,
    } = &scope.construction
    {
        let collection = render_string_path(collection);
        let key = render_string_path(key);
        let parent = render_string_path(parent);
        let root = root
            .map(|root| format!("Some(expression_{root} as codegen_runtime::AdjacencyTreeRoot)"))
            .unwrap_or_else(|| "None".to_string());
        output.push_str(&format!(
            "{indent}let output = adjacency_tree(\n{indent}    {context},\n{indent}    AdjacencyTreeFields {{\n{indent}        collection: &[{collection}],\n{indent}        key: &[{key}],\n{indent}        parent: &[{parent}],\n{indent}        target_key: {},\n{indent}        target_children: {},\n{indent}    }},\n{indent}    {root},\n{indent})?;\n",
            rust_string(target_key),
            rust_string(target_children),
        ));
        return output;
    }
    if let TargetConstruction::OwnedAdjacencyTree {
        collection,
        key,
        parent,
        target_key,
        target_children,
        root,
    } = &scope.construction
    {
        // Only paired Basic scopes render this construction: the owner is the
        // exact stored endpoint and the root reads the receiving context.
        let owner = match &collection.owner {
            mapping::SourceOwner::Primary => "codegen_runtime::SourceOwnerRef::Primary".to_string(),
            mapping::SourceOwner::Named { name } => {
                format!(
                    "codegen_runtime::SourceOwnerRef::Named({})",
                    rust_string(name)
                )
            }
        };
        let root = root
            .map(|root| {
                format!("Some(expression_{root} as codegen_runtime::OwnedAdjacencyTreeRoot)")
            })
            .unwrap_or_else(|| "None".to_string());
        output.push_str(&format!(
            "{indent}let output = codegen_runtime::owned_adjacency_tree(\n{indent}    {context},\n{indent}    {owner},\n{indent}    codegen_runtime::AdjacencyTreeFields {{\n{indent}        collection: &[{}],\n{indent}        key: &[{}],\n{indent}        parent: &[{}],\n{indent}        target_key: {},\n{indent}        target_children: {},\n{indent}    }},\n{indent}    {root},\n{indent})?;\n",
            render_string_path(&collection.path),
            render_string_path(key),
            render_string_path(parent),
            rust_string(target_key),
            rust_string(target_children),
        ));
        return output;
    }
    if matches!(&scope.construction, TargetConstruction::CopyCurrentSource) {
        output.push_str(&format!(
            "{indent}let output = ({context}).copy_current_group()?;\n"
        ));
        return output;
    }
    if matches!(scope.construction, TargetConstruction::CopyCurrentRows) {
        output.push_str(&format!(
            "{indent}let output = ({context}).copy_current_rows()?;\n"
        ));
        return output;
    }
    for (index, binding) in scope.bindings.iter().enumerate() {
        let expression = format!("expression_{}({context})?", binding.expression);
        let value = if preserve.is_some_and(|preserve| preserve.preserves(&binding.target_field)) {
            expression
        } else {
            render_target_adaptation(&expression, binding.target_domain)
        };
        output.push_str(&format!("{indent}let value_{index} = {value};\n"));
    }

    let groups = binding_groups(&scope.bindings);
    for (group_index, group) in groups.iter().enumerate() {
        if !group.repeating {
            continue;
        }
        output.push_str(&format!(
            "{indent}let mut repeated_{group_index} = Vec::new();\n"
        ));
        for binding_index in &group.binding_indices {
            output.push_str(&format!(
                "{indent}if value_{binding_index} != Value::Null {{\n{indent}    repeated_{group_index}.push(scalar(value_{binding_index}));\n{indent}}}\n"
            ));
        }
    }

    let mut fields = Vec::new();
    for (group_index, binding_group) in groups.iter().enumerate() {
        let name = rust_string(&binding_group.target_field);
        if binding_group.repeating {
            fields.push(format!("field({name}, repeated(repeated_{group_index}))"));
        } else {
            let index = binding_group.binding_indices[0];
            fields.push(format!("field({name}, scalar(value_{index}))"));
        }
    }
    if matches!(&scope.construction, TargetConstruction::DynamicGroup { .. }) {
        output.push_str(&format!("{indent}let mut fields = vec![\n"));
        for field in fields {
            output.push_str(&format!("{indent}    {field},\n"));
        }
        output.push_str(&format!("{indent}];\n"));
        let TargetConstruction::DynamicGroup {
            fixed_fields,
            bindings,
            children,
            ..
        } = &scope.construction
        else {
            unreachable!("matched dynamic group")
        };
        let fixed_fields = fixed_fields
            .iter()
            .map(|name| rust_string(name))
            .collect::<Vec<_>>()
            .join(", ");
        for (index, binding) in bindings.iter().enumerate() {
            let value = render_target_adaptation(
                &format!("expression_{}({context})?", binding.value),
                binding.target_domain,
            );
            output.push_str(&format!(
                "{indent}let dynamic_key_{index} = dynamic_property_name({}, expression_{}({context})?)?;\n{indent}let dynamic_value_{index} = {value};\n{indent}insert_dynamic_field(\n{indent}    &mut fields,\n{indent}    &[{fixed_fields}],\n{indent}    dynamic_key_{index},\n{indent}    scalar(dynamic_value_{index}),\n{indent})?;\n",
                binding.key,
                binding.key,
            ));
        }
        for (child, child_name) in scope
            .children
            .iter()
            .zip(&child_names[..scope.children.len()])
        {
            output.push_str(&format!(
                "{indent}fields.push(field({}, {child_name}({context})?));\n",
                rust_string(&child.target_field)
            ));
        }
        for (index, (child, child_name)) in children
            .iter()
            .zip(&child_names[scope.children.len()..])
            .enumerate()
        {
            output.push_str(&format!(
                "{indent}let dynamic_child_key_{index} = dynamic_property_name({}, expression_{}({context})?)?;\n{indent}let dynamic_child_value_{index} = {child_name}({context})?;\n{indent}insert_dynamic_field(\n{indent}    &mut fields,\n{indent}    &[{fixed_fields}],\n{indent}    dynamic_child_key_{index},\n{indent}    dynamic_child_value_{index},\n{indent})?;\n",
                child.key, child.key,
            ));
        }
        output.push_str(&format!("{indent}let output = group(fields);\n"));
    } else {
        for (child, child_name) in scope.children.iter().zip(child_names) {
            fields.push(format!(
                "field({}, {child_name}({context})?)",
                rust_string(&child.target_field)
            ));
        }
        if fields.is_empty() {
            output.push_str(&format!("{indent}let output = group(Vec::new());\n"));
        } else {
            output.push_str(&format!("{indent}let output = group([\n"));
            for field in fields {
                output.push_str(&format!("{indent}    {field},\n"));
            }
            output.push_str(&format!("{indent}]);\n"));
        }
    }
    if let TargetConstruction::XmlMixedContent { elements } = &scope.construction {
        // An owned selector (always a paired Basic context) attaches only its
        // own exact current occurrence; the legacy form keeps the current frame.
        let owned = scope
            .iteration
            .as_ref()
            .and_then(|iteration| iteration.owned_source_iteration());
        match owned {
            Some(selector) => output.push_str(&format!(
                "{indent}let output = ({context}).preserve_owned_xml_mixed_content(\n{indent}    {},\n{indent}    &[{}],\n{indent}    output,\n{indent}    &[\n",
                owned_basic::owner_literal(&selector.source().owner),
                render_string_path(&selector.source().path),
            )),
            None => output.push_str(&format!(
                "{indent}let output = preserve_xml_mixed_content(\n{indent}    {context},\n{indent}    output,\n{indent}    &[\n"
            )),
        }
        let element_type = if owned.is_some() {
            "codegen_runtime::XmlMixedContentElement"
        } else {
            "XmlMixedContentElement"
        };
        for element in elements {
            output.push_str(&format!(
                "{indent}        {element_type} {{ source: {}, target: {} }},\n",
                rust_string(&element.source),
                rust_string(&element.target),
            ));
        }
        output.push_str(&format!(
            "{indent}    ],\n{indent}){};\n",
            if owned.is_some() { "?" } else { "" }
        ));
    }
    output
}

pub(super) struct BindingGroup {
    pub(super) target_field: String,
    pub(super) repeating: bool,
    pub(super) binding_indices: Vec<usize>,
}

pub(super) fn binding_groups(bindings: &[Binding]) -> Vec<BindingGroup> {
    let mut groups = Vec::<BindingGroup>::new();
    let mut indices = BTreeMap::<&str, usize>::new();
    for (binding_index, binding) in bindings.iter().enumerate() {
        if let Some(index) = indices.get(binding.target_field.as_str()).copied() {
            let group = &mut groups[index];
            group.binding_indices.push(binding_index);
        } else {
            indices.insert(&binding.target_field, groups.len());
            groups.push(BindingGroup {
                target_field: binding.target_field.clone(),
                repeating: binding.repeating,
                binding_indices: vec![binding_index],
            });
        }
    }
    groups
}
