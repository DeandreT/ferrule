//! Expression reachability and Rust function rendering.

use super::*;

// Direct Programs may retain expressions that lowering would omit. Keep only
// roots used by rendered scopes, failures and the existing user-function policy,
// then follow their dependencies. Every expression is still rendered above for
// validation before deciding whether to publish its function.
pub(super) fn expression_usage(
    program: &Program,
    scopes: &[(String, &TargetScope, Vec<String>, Vec<String>)],
) -> BTreeSet<NodeId> {
    expression_usage_with_roots(program, scopes, [])
}

/// `expression_usage` plus roots a renderer calls outside scope bodies.
pub(super) fn expression_usage_with_roots(
    program: &Program,
    scopes: &[(String, &TargetScope, Vec<String>, Vec<String>)],
    roots: impl IntoIterator<Item = NodeId>,
) -> BTreeSet<NodeId> {
    let mut pending = roots.into_iter().collect::<BTreeSet<_>>();
    for (_, scope, _, _) in scopes {
        pending.extend(scope.bindings.iter().map(|binding| binding.expression));
        if let Some(iteration) = &scope.iteration {
            pending.extend(iteration.roots());
            if let Some(dynamic) = iteration
                .source_iteration()
                .and_then(|source| source.path().first())
                .and_then(|name| {
                    program
                        .extra_sources
                        .iter()
                        .find(|source| source.name == *name)
                })
                .and_then(|source| source.dynamic.as_ref())
            {
                pending.insert(dynamic.path);
            }
        }
        match &scope.construction {
            TargetConstruction::DynamicGroup {
                bindings, children, ..
            } => {
                for binding in bindings {
                    pending.extend([binding.key, binding.value]);
                }
                pending.extend(children.iter().map(|child| child.key));
            }
            TargetConstruction::Scalar { expression, .. }
            | TargetConstruction::RecursiveFilter {
                predicate: expression,
                ..
            } => {
                pending.insert(*expression);
            }
            TargetConstruction::AdjacencyTree { root, .. }
            | TargetConstruction::OwnedAdjacencyTree { root, .. } => pending.extend(*root),
            TargetConstruction::Group
            | TargetConstruction::CollectDocuments
            | TargetConstruction::CopyCurrentSource
            | TargetConstruction::CopyCurrentRows
            | TargetConstruction::XmlMixedContent { .. }
            | TargetConstruction::PathHierarchy { .. }
            | TargetConstruction::OwnedPathHierarchy { .. } => {}
        }
    }
    for rule in &program.failure_rules {
        pending.extend(rule.selection.predicate());
        pending.extend(rule.message);
        if let codegen::FailureIteration::Generated(sequence) = &rule.iteration {
            pending.extend(sequence.roots());
        }
    }
    // User-function bodies retain their existing emission. Keeping each call
    // root also keeps its arguments, without introducing unused function bodies.
    pending.extend(program.expressions.iter().filter_map(|node| {
        matches!(&node.expression, Expression::UserFunctionCall { .. }).then_some(node.id)
    }));
    let expressions = program
        .expressions
        .iter()
        .map(|node| (node.id, &node.expression))
        .collect::<BTreeMap<_, _>>();
    let mut retained = BTreeSet::new();
    while let Some(node) = pending.pop_first() {
        if retained.insert(node)
            && let Some(expression) = expressions.get(&node)
        {
            pending.extend(expression_inputs(expression));
        }
    }
    retained
}

pub(super) fn expression_inputs(expression: &Expression) -> Vec<NodeId> {
    match expression {
        Expression::DynamicSourceField { key, .. } => vec![*key],
        Expression::OwnedDynamicSourceField(field) => vec![field.key],
        Expression::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Expression::OwnedXmlMixedContent(mixed) => mixed
            .replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Expression::RuntimeParameterDefault { default, .. } => vec![*default],
        Expression::InvocationCounter {
            start,
            step,
            restart,
            ..
        } => [Some(*start), Some(*step), *restart]
            .into_iter()
            .flatten()
            .collect(),
        Expression::Call { args, .. } | Expression::UserFunctionCall { args, .. } => args.clone(),
        Expression::DelimitedTextField { input, .. } | Expression::ValueMap { input, .. } => {
            vec![*input]
        }
        Expression::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Expression::Raise { message } => message.iter().copied().collect(),
        Expression::Lookup { matches, .. } => vec![*matches],
        Expression::OwnedLookup(lookup) => vec![lookup.matches],
        Expression::OwnedCollectionFind(find) => vec![find.predicate, find.value],
        Expression::OwnedAggregate(aggregate) => aggregate.inputs(),
        Expression::OwnedJoinAggregate(aggregate) => aggregate.inputs(),
        Expression::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Expression::Aggregate { value, arg, .. } => {
            value.expression().into_iter().chain(*arg).collect()
        }
        Expression::JoinAggregate {
            join,
            expression,
            arg,
            ..
        } => join
            .plan()
            .computed_source()
            .map(|computed| computed.value())
            .into_iter()
            .chain(*expression)
            .chain(*arg)
            .collect(),
        Expression::SequenceExists {
            sequence,
            predicate,
        } => sequence.roots().chain([*predicate]).collect(),
        Expression::SequenceItemAt { sequence, index } => {
            sequence.roots().chain([*index]).collect()
        }
        Expression::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } => sequence
            .roots()
            .chain(*predicate)
            .chain(*expression)
            .chain(*arg)
            .collect(),
        Expression::OwnedSourceField(_)
        | Expression::OwnedPosition(_)
        | Expression::OwnedRowDocumentPath(_)
        | Expression::OwnedSourceDocumentPath(_)
        | Expression::OwnedXmlSerialize(_)
        | Expression::OwnedJoinField(_)
        | Expression::OwnedJoinSourcePosition(_)
        | Expression::SourceField { .. }
        | Expression::SourceRootXmlTypeEquals { .. }
        | Expression::SourceRootField { .. }
        | Expression::XmlSerialize { .. }
        | Expression::SourceDocumentPath
        | Expression::Position { .. }
        | Expression::GeneratedPosition { .. }
        | Expression::JoinField { .. }
        | Expression::JoinPosition { .. }
        | Expression::Const { .. }
        | Expression::FunctionParameter { .. }
        | Expression::RuntimeValue { .. }
        | Expression::RuntimeParameter { .. } => Vec::new(),
    }
}

pub(super) fn render_user_function(
    function: &UserFunctionProgram,
    functions: &BTreeMap<FunctionId, &UserFunctionProgram>,
    filter_map: bool,
) -> Result<String, EmitError> {
    let prefix = format!("user_function_{}_expression_", function.id.get());
    let parameters = function
        .parameters
        .iter()
        .enumerate()
        .map(|(index, parameter)| (parameter.id, index))
        .collect::<BTreeMap<_, _>>();
    let mut output = String::new();
    for expression in &function.expressions {
        output.push_str(&render_expression(
            expression.id,
            &expression.expression,
            &prefix,
            Some(&parameters),
            functions,
            filter_map,
            Some(function.id.get()),
        )?);
    }
    if filter_map {
        let id = function.id.get();
        output.push_str(&format!("#[allow(dead_code)]\nconst USER_FUNCTION_{id}_OUTPUT: u32 = {};\n\n#[allow(dead_code)]\nfn user_function_{id}(\n    context: &ScopeContext<'_>,\n    raw_parameters: &[Value],\n) -> Result<Value, RuntimeError> {{\n    let _call = context.filter_map_call_entry({id})?;\n    if raw_parameters.len() != {} {{\n        return Err(RuntimeError::UserFunctionArity {{ function: {id}, expected: {}, found: raw_parameters.len() }});\n    }}\n    let parameters = vec![\n", function.output, function.parameters.len(), function.parameters.len()));
        for (index, parameter) in function.parameters.iter().enumerate() {
            output.push_str(&format!("        adapt_user_function_value(raw_parameters[{index}].clone(), ScalarType::{}, {id}, Some({}))?,\n", scalar_type_name(parameter.ty), parameter.id.get()));
        }
        output.push_str(&format!("    ];\n    let output = {prefix}{}(context, &parameters)?;\n    adapt_user_function_value(output, ScalarType::{}, {id}, None)\n}}\n\n", function.output, scalar_type_name(function.output_type)));
    } else {
        output.push_str(&format!(
            "fn user_function_{}(\n    context: &ScopeContext<'_>,\n    parameters: &[Value],\n) -> Result<Value, RuntimeError> {{\n    let output = {}{}(context, parameters)?;\n    adapt_user_function_value(output, ScalarType::{}, {}, None)\n}}\n\n",
            function.id.get(), prefix, function.output,
            scalar_type_name(function.output_type), function.id.get(),
        ));
    }
    Ok(output)
}

#[derive(Clone, Copy)]
pub(super) struct ExpressionEmission {
    pub(super) filter_map: bool,
    pub(super) owner_function: Option<u64>,
    pub(super) basic: bool,
    pub(super) owned_leaf_nodes: bool,
}
impl ExpressionEmission {
    fn is_basic(self) -> bool {
        self.basic
    }
}

pub(super) fn render_expression(
    id: NodeId,
    expression: &Expression,
    prefix: &str,
    parameters: Option<&BTreeMap<FunctionParameterId, usize>>,
    functions: &BTreeMap<FunctionId, &UserFunctionProgram>,
    filter_map: bool,
    owner_function: Option<u64>,
) -> Result<String, EmitError> {
    render_expression_in_context(
        id,
        expression,
        prefix,
        parameters,
        functions,
        ExpressionEmission {
            filter_map,
            owner_function,
            basic: false,
            owned_leaf_nodes: false,
        },
    )
}

pub(super) fn render_expression_in_context(
    id: NodeId,
    expression: &Expression,
    prefix: &str,
    parameters: Option<&BTreeMap<FunctionParameterId, usize>>,
    functions: &BTreeMap<FunctionId, &UserFunctionProgram>,
    emission: ExpressionEmission,
) -> Result<String, EmitError> {
    render_expression_in_context_with_scalar_item_at(
        id, expression, prefix, parameters, functions, emission, false,
    )
}

pub(super) fn render_expression_in_context_with_scalar_item_at(
    id: NodeId,
    expression: &Expression,
    prefix: &str,
    parameters: Option<&BTreeMap<FunctionParameterId, usize>>,
    functions: &BTreeMap<FunctionId, &UserFunctionProgram>,
    emission: ExpressionEmission,
    scalar_item_at: bool,
) -> Result<String, EmitError> {
    let filter_map = emission.filter_map;
    let owner_function = emission.owner_function;
    let basic = emission.is_basic();
    let context_type = if basic {
        "BasicScopeContext"
    } else {
        "ScopeContext"
    };
    let legacy_context = if basic {
        "context.legacy_view()"
    } else {
        "context"
    };
    let parameter_argument = parameters.map(|_| ", parameters").unwrap_or_default();
    let call_expression =
        |node: NodeId, context: &str| format!("{prefix}{node}({context}{parameter_argument})");
    let owned_body = match expression {
        // The key is evaluated in the receiving context before the owned read.
        Expression::OwnedDynamicSourceField(field) if basic => Some(
            owned_basic::render_owned_dynamic_field(field, &call_expression(field.key, "context")),
        ),
        _ if basic => owned_basic::render_owned_access(expression),
        _ => None,
    };
    let body = if let Some(body) = owned_body {
        if emission.owned_leaf_nodes {
            // Keep the leaf's early `?` inside the Result being annotated.
            format!(
                "(|| -> Result<Value, RuntimeError> {{ {body} }})().map_err(|error| error.with_owned_source_node({id}))"
            )
        } else {
            body
        }
    } else {
        match expression {
            Expression::OwnedLookup(lookup) if basic => owned_basic::render_owned_lookup(
                id,
                lookup,
                &call_expression(lookup.matches, "context"),
            ),
            Expression::OwnedCollectionFind(find) if basic => {
                owned_basic::render_owned_collection_find(
                    id,
                    find,
                    &call_expression(find.predicate, "item_context"),
                    &call_expression(find.value, "item_context"),
                )
            }
            Expression::OwnedXmlSerialize(serializer) if basic => {
                owned_basic::render_owned_xml_serialize(id, serializer)?
            }
            Expression::OwnedAggregate(aggregate) if basic => {
                owned_basic::render_owned_aggregate(id, aggregate, call_expression)
            }
            Expression::OwnedJoinAggregate(aggregate) if basic => {
                owned_basic::render_owned_join_aggregate(id, aggregate, call_expression)
            }
            Expression::JoinPosition { join } if basic => format!(
                "Ok(Value::Int(context.owned_join_position({})? as i64))",
                join.get()
            ),
            Expression::OwnedXmlMixedContent(mixed) if basic => {
                owned_basic::render_owned_xml_mixed_content(id, mixed, prefix)
            }
            Expression::OwnedSourceField(_)
            | Expression::OwnedPosition(_)
            | Expression::OwnedRowDocumentPath(_)
            | Expression::OwnedSourceDocumentPath(_)
            | Expression::OwnedDynamicSourceField(_)
            | Expression::OwnedLookup(_)
            | Expression::OwnedCollectionFind(_)
            | Expression::OwnedXmlSerialize(_)
            | Expression::OwnedAggregate(_)
            | Expression::OwnedJoinField(_)
            | Expression::OwnedJoinSourcePosition(_)
            | Expression::OwnedXmlMixedContent(_)
            | Expression::OwnedJoinAggregate(_) => {
                let namespace = owner_function.map_or(codegen::OwnedSourceNamespace::Main, |id| {
                    codegen::OwnedSourceNamespace::Function(FunctionId::new(id))
                });
                return Err(
                    codegen::owned_expression_emission_error(namespace, id, expression).into(),
                );
            }
            Expression::SourceField { frame, path } if basic => {
                let path = render_string_path(path);
                let frame = frame.as_ref().map_or_else(
                    || "None".to_string(),
                    |frame| format!("Some(&[{}][..])", render_string_path(frame)),
                );
                format!("context.legacy_field(&[{path}], {frame})")
            }
            Expression::SourceField { frame, path } => {
                let path = path
                    .iter()
                    .map(|segment| rust_string(segment))
                    .collect::<Vec<_>>()
                    .join(", ");
                match frame {
                    Some(frame) => {
                        let frame = frame
                            .iter()
                            .map(|segment| rust_string(segment))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!(
                            "context.resolve_source_field_in_frame(&[{frame}], &[{path}]).map_err(RuntimeError::from)"
                        )
                    }
                    None => {
                        format!(
                            "context.resolve_source_field(&[{path}]).map_err(RuntimeError::from)"
                        )
                    }
                }
            }
            Expression::SourceRootXmlTypeEquals {
                canonical_expanded_type,
            } => format!(
                "context.source_root_xml_type_equals({id}, {})",
                rust_string(canonical_expanded_type)
            ),
            Expression::SourceRootField { path, required } => {
                let reader = if *required {
                    "source_root_required_field"
                } else {
                    "source_root_field"
                };
                format!("context.{reader}({id}, &[{}])", render_string_path(path))
            }
            Expression::DynamicSourceField { object, frame, key } => {
                let object = render_string_path(object);
                let frame = frame.as_ref().map_or_else(
                    || "None".to_string(),
                    |frame| format!("Some(&[{}][..])", render_string_path(frame)),
                );
                let key = call_expression(*key, "context");
                format!(
                    "{{\n        let key = {key}?;\n        Ok(context.dynamic_scalar({frame}, &[{object}], &key))\n    }}"
                )
            }
            Expression::XmlSerialize {
                frame,
                path,
                schema,
                declaration,
                indent,
                namespace,
            } => {
                let schema =
                    serialize_embedded_schema(schema, codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES)?;
                let path = render_string_path(path);
                let frame = frame.as_ref().map_or_else(
                    || "None".to_string(),
                    |frame| format!("Some(&[{}][..])", render_string_path(frame)),
                );
                let namespace = namespace.as_ref().map_or_else(
                    || "None".to_string(),
                    |namespace| format!("Some({})", rust_string(namespace)),
                );
                format!(
                    "{{\n        let instance = context.resolve_xml_instance({frame}, &[{path}])?;\n        serialize_xml({id}, {}, instance, {declaration}, {indent}, {namespace})\n    }}",
                    rust_string(&schema)
                )
            }
            Expression::XmlMixedContent {
                frame,
                path,
                replacements,
            } => {
                let path = render_string_path(path);
                let frame = frame.as_ref().map_or_else(
                    || "None".to_string(),
                    |frame| format!("Some(&[{}][..])", render_string_path(frame)),
                );
                let replacements = replacements
                .iter()
                .map(|replacement| {
                    format!(
                        "XmlMixedContentReplacement {{ element: {}, collection: &[{}], expression: {prefix}{} }}",
                        rust_string(&replacement.element),
                        render_string_path(&replacement.collection),
                        replacement.expression,
                    )
                })
                .collect::<Vec<_>>()
                .join(",\n            ");
                format!(
                    "xml_mixed_content(\n        context,\n        {frame},\n        &[{path}],\n        &[\n            {replacements}\n        ],\n    )"
                )
            }
            Expression::SourceDocumentPath => {
                "context.source_document_path().map_err(RuntimeError::from)".into()
            }
            Expression::GeneratedPosition { item } => {
                format!("context.generated_position({id}, {item})")
            }
            Expression::Position { collection } if basic => {
                // Only actual control-free Vec-backed walks or legacy fallback1
                // create these positions; no caller-injected phase is admitted.
                format!(
                    "Ok(Value::Int(context.legacy_position(&[{}]) as i64))",
                    render_string_path(collection)
                )
            }
            Expression::Position { collection } => {
                let collection = collection
                    .iter()
                    .map(|segment| rust_string(segment))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Ok(Value::Int(context.position(&[{collection}]) as i64))")
            }
            Expression::JoinField {
                join,
                collection,
                path,
            } => {
                let collection = render_string_path(collection);
                let path = render_string_path(path);
                format!(
                    "context.resolve_join_scalar({}, &[{collection}], &[{path}]).map_err(RuntimeError::from)",
                    join.get()
                )
            }
            Expression::JoinPosition { join } => format!(
                "Ok(Value::Int(context.join_position({})? as i64))",
                join.get()
            ),
            Expression::Const { value } => {
                format!("Ok({})", rust_value(id, value)?)
            }
            Expression::FunctionParameter { parameter } => {
                let index = parameters
                    .and_then(|parameters| parameters.get(parameter))
                    .copied()
                    .ok_or(ProgramValidationError::FunctionParameterInMain {
                        node: id,
                        parameter: *parameter,
                    })?;
                format!("Ok(parameters[{index}].clone())")
            }
            Expression::RuntimeValue { value } => format!(
                "{legacy_context}.runtime_value(RuntimeValue::{})",
                runtime_value_name(*value)
            ),
            Expression::RuntimeParameter { name, ty } => format!(
                "{legacy_context}.runtime_parameter({}, {}, ScalarType::{})",
                id,
                rust_string(name),
                scalar_type_name(*ty)
            ),
            Expression::RuntimeParameterDefault { name, ty, default } => format!(
                "{legacy_context}.runtime_parameter_or_else({}, {}, ScalarType::{}, || {})",
                id,
                rust_string(name),
                scalar_type_name(*ty),
                call_expression(*default, "context")
            ),
            Expression::InvocationCounter {
                key,
                start,
                step,
                restart,
            } => {
                if let Some(function) = owner_function {
                    return Err(ProgramValidationError::UnsupportedUserFunctionExpression {
                        function: FunctionId::new(function),
                        node: id,
                    }
                    .into());
                }
                let restart = restart.map_or_else(
                    || "Value::Null".to_string(),
                    |node| format!("{}?", call_expression(node, "context")),
                );
                format!(
                    "{{\n        {legacy_context}.require_invocation_counters({id})?;\n        let start = codegen_runtime::invocation_counter_integer({id}, \"start\", {}?)?;\n        let step = codegen_runtime::invocation_counter_integer({id}, \"step\", {}?)?;\n        let restart = codegen_runtime::invocation_counter_restart({id}, {restart})?;\n        {legacy_context}.next_invocation_counter({id}, {}, start, step, restart.as_deref())\n    }}",
                    call_expression(*start, "context"),
                    call_expression(*step, "context"),
                    rust_string(key)
                )
            }
            Expression::Call { function, args } => {
                let mut body = String::from("{\n        let args = vec![\n");
                for arg in args {
                    body.push_str(&format!(
                        "            {}?,\n",
                        call_expression(*arg, "context")
                    ));
                }
                let call = if owner_function.is_none()
                    && matches!(
                        function.as_str(),
                        "json_parse_array_length"
                            | "json_parse_array_field"
                            | "json_parse_field"
                            | "json_parse_object_exists"
                    )
                    && let [source, ..] = args.as_slice()
                {
                    format!(
                        "{legacy_context}.call_json_array_projection({source}, {}, &args)",
                        rust_string(function.as_str())
                    )
                } else {
                    format!("call({}, &args)", rust_string(function.as_str()))
                };
                body.push_str(&format!("        ];\n        {call}\n    }}"));
                body
            }
            Expression::DelimitedTextField { input, parser } => format!(
                "{{\n        let args = [\n            {}?,\n            Value::String({}.to_string()),\n            Value::String({}.to_string()),\n        ];\n        call(\"flextext_parse_field\", &args)\n    }}",
                call_expression(*input, "context"),
                rust_string(parser.layout_descriptor()),
                rust_string(parser.path_descriptor()),
            ),
            Expression::UserFunctionCall { function, args } => {
                let definition = functions.get(function).ok_or({
                    ProgramValidationError::MissingUserFunction {
                        owner: None,
                        node: id,
                        function: *function,
                    }
                })?;
                let mut body = String::from("{\n");
                // Evaluate all expressions before adapting any declared parameter.
                for (index, argument) in args.iter().enumerate() {
                    body.push_str(&format!(
                        "        let raw_argument_{index} = {}?;\n",
                        call_expression(*argument, "context"),
                    ));
                }
                if !filter_map {
                    for (index, parameter) in definition.parameters.iter().enumerate() {
                        body.push_str(&format!(
                    "        let argument_{index} = adapt_user_function_value(\n            raw_argument_{index},\n            ScalarType::{},\n            {},\n            Some({}),\n        )?;\n",
                    scalar_type_name(parameter.ty),
                    function.get(),
                    parameter.id.get(),
                ));
                    }
                }
                let arguments = (0..args.len())
                    .map(|index| {
                        if filter_map {
                            format!("raw_argument_{index}")
                        } else {
                            format!("argument_{index}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                body.push_str(&format!(
                    "        user_function_{}(context, &[{arguments}])\n    }}",
                    function.get()
                ));
                body
            }
            Expression::If {
                condition,
                then,
                else_,
            } => {
                let condition_call = call_expression(*condition, "context");
                let then_call = call_expression(*then, "context");
                let else_call = call_expression(*else_, "context");
                format!(
                    "{{\n        let condition = {condition_call}?;\n        if require_bool({condition}, condition)? {{\n            {then_call}\n        }} else {{\n            {else_call}\n        }}\n    }}"
                )
            }
            Expression::Raise { message } => {
                let message = message.map_or_else(
                    || "None".to_string(),
                    |message| format!("Some({}?)", call_expression(message, "context")),
                );
                format!("Err(codegen_runtime::mapping_exception({id}, {message}))")
            }
            Expression::ValueMap {
                input,
                input_type,
                table,
                default,
            } => {
                let normalization = if parameters.is_none() && input_type.is_some() {
                    "        let input = match input { Value::JsonNull(_) => Value::Null, value => value };\n"
                } else {
                    ""
                };
                let input_type = input_type.map_or_else(
                    || "None".to_string(),
                    |value| format!("Some(ScalarType::{})", scalar_type_name(value)),
                );
                let table = table
                    .iter()
                    .map(|(from, to)| {
                        Ok(format!(
                            "({}, {})",
                            rust_value(id, from)?,
                            rust_value(id, to)?
                        ))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ");
                let default = match default {
                    Some(value) => format!("Some({})", rust_value(id, value)?),
                    None => "None".to_string(),
                };
                let input = call_expression(*input, "context");
                let lookup = if parameters.is_some() {
                    "codegen_runtime::value_map_user_function"
                } else {
                    "value_map"
                };
                format!(
                    "{{\n        let input = {input}?;\n{normalization}        Ok({lookup}(input, {input_type}, &[{table}], {default}))\n    }}"
                )
            }
            Expression::Lookup {
                collection,
                key,
                matches,
                value,
            } => {
                let collection = render_string_path(collection);
                let key = render_string_path(key);
                let value = render_string_path(value);
                format!(
                    "{{\n        let needle = expression_{matches}(context)?;\n        context.lookup(&[{collection}], &[{key}], &needle, &[{value}]).map_err(RuntimeError::from)\n    }}"
                )
            }
            Expression::CollectionFind {
                collection,
                predicate,
                value,
            } => {
                let collection = render_string_path(collection);
                format!(
                    "{{\n        let candidates = context.collection_find_items(&[{collection}])?;\n        for item_context in candidates {{\n            let predicate = expression_{predicate}(&item_context)?;\n            if collection_find_selected({predicate}, predicate)? {{\n                return expression_{value}(&item_context);\n            }}\n        }}\n        Ok(Value::Null)\n    }}"
                )
            }
            Expression::Aggregate {
                function,
                collection,
                value,
                arg,
            } => {
                let collection = collection
                    .iter()
                    .map(|segment| rust_string(segment))
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut body = format!(
                    "{{\n        let item_contexts = context.aggregate_items(&[{collection}]);\n        let mut values = Vec::with_capacity(item_contexts.len());\n        for item_context in item_contexts {{\n"
                );
                match value {
                    AggregateValue::Path(path) => {
                        let path = path
                            .iter()
                            .map(|segment| rust_string(segment))
                            .collect::<Vec<_>>()
                            .join(", ");
                        body.push_str(&format!(
                            "            {}\n",
                            aggregate_collect(
                                *function,
                                "values",
                                &format!("item_context.aggregate_current_scalar(&[{path}])")
                            )
                        ));
                    }
                    AggregateValue::Expression(expression) => body.push_str(&format!(
                        "            {}\n",
                        aggregate_collect(
                            *function,
                            "values",
                            &format!("expression_{expression}(&item_context)?")
                        )
                    )),
                }
                body.push_str("        }\n");
                match arg {
                    Some(arg) => body.push_str(&format!(
                        "        let arg = Some(expression_{arg}(context)?);\n"
                    )),
                    None => body.push_str("        let arg = None;\n"),
                }
                body.push_str(&format!(
                    "        aggregate(AggregateFunction::{}, &values, arg)\n    }}",
                    aggregate_function_name(*function)
                ));
                body
            }
            Expression::JoinAggregate {
                function,
                join,
                expression,
                arg,
            } => {
                let tuples = render_inner_join_call(join).unwrap_or_else(|| "Vec::new()".into());
                let mut body = format!("{{\n        let tuple_contexts = {tuples};\n");
                match expression {
                Some(expression) => body.push_str(&format!(
                    "        let mut values = Vec::with_capacity(tuple_contexts.len());\n        for tuple_context in tuple_contexts {{\n            {}\n        }}\n", aggregate_collect(*function, "values", &format!("expression_{expression}(&tuple_context)?"))
                )),
                None if *function == AggregateFunction::DecimalSum => body.push_str(&format!(
                    "        let mut values = Vec::with_capacity(tuple_contexts.len());\n        for _ in tuple_contexts {{\n            {}\n        }}\n",
                    aggregate_collect(*function, "values", "Value::Null")
                )),
                None => body.push_str(
                    "        let values = vec![Value::Null; tuple_contexts.len()];\n",
                ),
            }
                match arg {
                    Some(arg) => body.push_str(&format!(
                        "        let arg = Some(expression_{arg}(context)?);\n"
                    )),
                    None => body.push_str("        let arg = None;\n"),
                }
                body.push_str(&format!(
                    "        aggregate(AggregateFunction::{}, &values, arg)\n    }}",
                    aggregate_function_name(*function)
                ));
                body
            }
            Expression::SequenceExists {
                sequence,
                predicate,
            } if scalar_item_at => {
                // Producer arguments use the receiving context. Each reached
                // item copies that view with one appended lexical item frame;
                // Basic copies are admitted against the caller's policy.
                let mut body = String::from("{\n");
                if !scalar_sequence::render_values(sequence, "        ", &mut body, &|node| {
                    call_expression(node, "context")
                }) {
                    return Err(codegen::owned_expression_emission_error(
                        codegen::OwnedSourceNamespace::Main,
                        id,
                        expression,
                    )
                    .into());
                }
                let reached = if basic {
                    "            let item_context = item_context?;\n"
                } else {
                    ""
                };
                let predicate_call = call_expression(*predicate, "&item_context");
                body.push_str(&format!(
                 "        let generated_items = GeneratedItems::with_owner(sequence_values, {});\n        for item_context in context.generated_item_contexts(&generated_items) {{\n{reached}            let predicate = {predicate_call}?;\n            if require_bool({predicate}, predicate)? {{\n                return Ok(Value::Bool(true));\n            }}\n        }}\n        Ok(Value::Bool(false))\n    }}", sequence.item()
            ));
                body
            }
            Expression::SequenceExists {
                sequence,
                predicate,
            } => {
                if basic {
                    return Err(codegen::owned_expression_emission_error(
                        codegen::OwnedSourceNamespace::Main,
                        id,
                        expression,
                    )
                    .into());
                }
                let mut body = String::from("{\n");
                render_generated_values(sequence, "        ", &mut body)?;
                body.push_str(&format!(
                 "        let generated_items = GeneratedItems::with_owner(sequence_values, {});\n        for item_context in context.generated_item_contexts(&generated_items) {{\n            let predicate = expression_{predicate}(&item_context)?;\n            if require_bool({predicate}, predicate)? {{\n                return Ok(Value::Bool(true));\n            }}\n        }}\n        Ok(Value::Bool(false))\n    }}", sequence.item()
            ));
                body
            }
            Expression::SequenceItemAt { sequence, index } => {
                let mut body = String::from("{\n");
                if scalar_item_at {
                    // Both phases use the receiving source context. The item
                    // declaration is lexical metadata, never a generated frame.
                    let evaluate = |node| call_expression(node, "context");
                    if !scalar_sequence::render_values(sequence, "        ", &mut body, &evaluate)
                        && !(basic
                            && scalar_sequence::render_owned_values(
                                sequence, id, "        ", &mut body, &evaluate,
                            ))
                    {
                        return Err(codegen::owned_expression_emission_error(
                            codegen::OwnedSourceNamespace::Main,
                            id,
                            expression,
                        )
                        .into());
                    }
                } else if basic {
                    return Err(codegen::owned_expression_emission_error(
                        codegen::OwnedSourceNamespace::Main,
                        id,
                        expression,
                    )
                    .into());
                } else {
                    render_generated_values(sequence, "        ", &mut body)?;
                }
                let index = if scalar_item_at {
                    call_expression(*index, "context")
                } else {
                    format!("expression_{index}(context)")
                };
                body.push_str(&format!(
                "        let index = {index}?;\n        aggregate(AggregateFunction::ItemAt, &sequence_values, Some(index))\n    }}"
            ));
                body
            }
            Expression::SequenceAggregate {
                function,
                sequence,
                predicate,
                expression: value,
                arg,
            } if scalar_item_at => {
                // Producer arguments and the reducer argument use the receiving
                // context. Each item copies that view with one appended lexical
                // item frame for the predicate and value; Basic copies are
                // admitted against the caller's policy.
                let mut body = String::from("{\n");
                if !scalar_sequence::render_values(sequence, "        ", &mut body, &|node| {
                    call_expression(node, "context")
                }) {
                    return Err(codegen::owned_expression_emission_error(
                        codegen::OwnedSourceNamespace::Main,
                        id,
                        expression,
                    )
                    .into());
                }
                let reached = if basic {
                    "            let item_context = item_context?;\n"
                } else {
                    ""
                };
                body.push_str(&format!(
                    "        let generated_items = GeneratedItems::with_owner(sequence_values, {});\n        let mut aggregate_values = Vec::new();\n        for item_context in context.generated_item_contexts(&generated_items) {{\n{reached}", sequence.item()
                ));
                if let Some(predicate) = predicate {
                    let predicate_call = call_expression(*predicate, "&item_context");
                    body.push_str(&format!(
                        "            let selected = {predicate_call}?;\n            if !require_bool({predicate}, selected)? {{\n                continue;\n            }}\n"
                    ));
                }
                let value_call = call_expression(value.unwrap_or(sequence.item()), "&item_context");
                body.push_str(&format!(
                    "            aggregate_values.push({value_call}?);\n        }}\n"
                ));
                match arg {
                    Some(arg) => body.push_str(&format!(
                        "        let arg = Some({}?);\n",
                        call_expression(*arg, "context")
                    )),
                    None => body.push_str("        let arg = None;\n"),
                }
                body.push_str(&format!(
                    "        aggregate(AggregateFunction::{}, &aggregate_values, arg)\n    }}",
                    aggregate_function_name(*function)
                ));
                body
            }
            Expression::SequenceAggregate {
                function,
                sequence,
                predicate,
                expression: value,
                arg,
            } => {
                if basic {
                    return Err(codegen::owned_expression_emission_error(
                        codegen::OwnedSourceNamespace::Main,
                        id,
                        expression,
                    )
                    .into());
                }
                let mut body = String::from("{\n");
                render_generated_values(sequence, "        ", &mut body)?;
                body.push_str(&format!(
                "        let generated_items = GeneratedItems::with_owner(sequence_values, {});\n        let mut aggregate_values = Vec::new();\n", sequence.item()
            ));
                body.push_str(
                "        for item_context in context.generated_item_contexts(&generated_items) {\n",
            );
                if let Some(predicate) = predicate {
                    body.push_str(&format!(
                    "            let selected = expression_{predicate}(&item_context)?;\n            if !require_bool({predicate}, selected)? {{\n                continue;\n            }}\n"
                ));
                }
                body.push_str(&format!(
                    "            {}\n        }}\n",
                    aggregate_collect(
                        *function,
                        "aggregate_values",
                        &format!(
                            "expression_{}(&item_context)?",
                            value.unwrap_or(sequence.item())
                        )
                    )
                ));
                match arg {
                    Some(arg) => body.push_str(&format!(
                        "        let arg = Some(expression_{arg}(context)?);\n"
                    )),
                    None => body.push_str("        let arg = None;\n"),
                }
                body.push_str(&format!(
                    "        aggregate(AggregateFunction::{}, &aggregate_values, arg)\n    }}",
                    aggregate_function_name(*function)
                ));
                body
            }
        }
    };
    let parameters_argument = parameters
        .map(|_| ", parameters: &[Value]")
        .unwrap_or_default();
    let parameters_use = parameters
        .map(|_| "    let _ = parameters;\n")
        .unwrap_or_default();
    if filter_map {
        let function = owner_function.map_or_else(|| "None".into(), |id| format!("Some({id})"));
        Ok(format!(
            "#[allow(dead_code)]\nfn {prefix}{id}(context: &ScopeContext<'_>{parameters_argument}) -> Result<Value, RuntimeError> {{\n    let work_context = context.filter_map_node({function}, {id})?;\n    let context = &work_context;\n{parameters_use}    let result = (|| -> Result<Value, RuntimeError> {{ {body} }})();\n    result.map_err(|error| context.filter_map_wrap(error))\n}}\n\n"
        ))
    } else {
        let entry_observation = if basic {
            format!(
                "    #[cfg(test)]\n    eprintln!(\"RAW PRIVATE_BASIC_EXPRESSION module={{}} node={id}\", module_path!());\n"
            )
        } else {
            String::new()
        };
        Ok(format!(
            "fn {prefix}{id}(context: &{context_type}<'_>{parameters_argument}) -> Result<Value, RuntimeError> {{\n{entry_observation}    let _ = context;\n{parameters_use}    {body}\n}}\n\n"
        ))
    }
}
