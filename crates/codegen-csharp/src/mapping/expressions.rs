//! Expression and user-function rendering.

use super::*;

pub(super) fn render_user_function(
    function: &UserFunctionProgram,
    functions: &BTreeMap<FunctionId, &UserFunctionProgram>,
    has_filter_map: bool,
    output: &mut String,
) -> Result<(), EmitError> {
    let parameters = function
        .parameters
        .iter()
        .enumerate()
        .map(|(index, parameter)| (parameter.id, index))
        .collect::<BTreeMap<_, _>>();
    for expression in &function.expressions {
        render_user_function_expression(
            function.id,
            expression.id,
            &expression.expression,
            &parameters,
            functions,
            has_filter_map,
            output,
        )?;
    }
    output.push_str(&format!(
        "\n    private static global::Ferrule.Runtime.FerruleValue UserFunction_{}(\n        global::Ferrule.Runtime.ScopeContext context,\n        global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.FerruleValue> parameters)\n    {{\n        var value = UserFunction_{}_Node_{}(context, parameters);\n        return global::Ferrule.Runtime.FerruleUserFunctions.Adapt(\n            value,\n            global::Ferrule.Runtime.FerruleScalarType.{},\n            {}UL,\n            null);\n    }}\n",
        function.id.get(),
        function.id.get(),
        function.output,
        scalar_type_name(function.output_type),
        function.id.get(),
    ));
    if has_filter_map {
        filter_map::render_stage_entry(function, output);
    }
    Ok(())
}

pub(super) fn render_user_function_expression(
    function: FunctionId,
    node: NodeId,
    expression: &Expression,
    parameters: &BTreeMap<FunctionParameterId, usize>,
    functions: &BTreeMap<FunctionId, &UserFunctionProgram>,
    has_filter_map: bool,
    output: &mut String,
) -> Result<(), EmitError> {
    let call = |dependency: NodeId| {
        format!(
            "UserFunction_{}_Node_{dependency}(context, parameters)",
            function.get()
        )
    };
    output.push_str(&format!(
        "\n    private static global::Ferrule.Runtime.FerruleValue {}_{}_Node_{node}(\n        global::Ferrule.Runtime.ScopeContext context,\n        global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.FerruleValue> parameters)",
        if has_filter_map {
            "UserFunctionCore"
        } else {
            "UserFunction"
        },
        function.get()
    ));
    match expression {
        Expression::OwnedSourceField(_)
        | Expression::OwnedPosition(_)
        | Expression::OwnedRowDocumentPath(_)
        | Expression::OwnedSourceDocumentPath(_)
        | Expression::OwnedDynamicSourceField(_) => {
            return Err(codegen::owned_expression_emission_error(
                codegen::OwnedSourceNamespace::Function(function),
                node,
                expression,
            )
            .into());
        }
        Expression::Const { value } => {
            output.push_str(" =>\n        ");
            output.push_str(&literal::value(node, value)?);
            output.push_str(";\n");
        }
        Expression::FunctionParameter { parameter } => {
            let Some(index) = parameters.get(parameter) else {
                return Err(ProgramValidationError::UnknownFunctionParameter {
                    function,
                    node,
                    parameter: *parameter,
                }
                .into());
            };
            output.push_str(&format!(" =>\n        parameters[{index}];\n"));
        }
        Expression::RuntimeValue { value } => {
            output.push_str(" =>\n        context.ResolveRuntimeValue(");
            output.push_str("global::Ferrule.Runtime.FerruleRuntimeValue.");
            output.push_str(runtime_value_name(*value));
            output.push_str(");\n");
        }
        Expression::RuntimeParameter { name, ty } => {
            output.push_str(" =>\n        context.ResolveRuntimeParameter(");
            output.push_str(&format!("{node}U, "));
            output.push_str(&literal::string(name));
            output.push_str(", global::Ferrule.Runtime.FerruleScalarType.");
            output.push_str(scalar_type_name(*ty));
            output.push_str(");\n");
        }
        Expression::RuntimeParameterDefault { name, ty, default } => {
            output.push_str(" =>\n        context.ResolveRuntimeParameterOrDefault(");
            output.push_str(&format!("{node}U, "));
            output.push_str(&literal::string(name));
            output.push_str(", global::Ferrule.Runtime.FerruleScalarType.");
            output.push_str(scalar_type_name(*ty));
            output.push_str(&format!(", () => {});\n", call(*default)));
        }
        Expression::InvocationCounter { .. } => {
            return Err(ProgramValidationError::UnsupportedUserFunctionExpression {
                function,
                node,
            }
            .into());
        }
        Expression::Call {
            function: builtin,
            args,
        } => {
            output.push_str(" =>\n        global::Ferrule.Runtime.FerruleFunctions.Call(");
            output.push_str(&literal::string(builtin.as_str()));
            output.push_str(", new global::Ferrule.Runtime.FerruleValue[] { ");
            for (index, argument) in args.iter().enumerate() {
                if index != 0 {
                    output.push_str(", ");
                }
                output.push_str(&call(*argument));
            }
            output.push_str(" });\n");
        }
        Expression::DelimitedTextField { input, parser } => {
            output.push_str(" =>\n        ");
            render_delimited_text_field(parser, &call(*input), output);
            output.push_str(";\n");
        }
        Expression::UserFunctionCall {
            function: called,
            args,
        } => {
            output.push_str("\n    {\n");
            render_user_function_call(
                node,
                *called,
                args,
                functions,
                call,
                has_filter_map,
                output,
            )?;
            output.push_str("    }\n");
        }
        Expression::If {
            condition,
            then,
            else_,
        } => {
            output.push_str("\n    {\n");
            output.push_str(&format!(
                "        var condition_{node} = {};\n",
                call(*condition)
            ));
            output.push_str(&format!(
                "        if (global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(condition_{node}, {condition}U))\n        {{\n            return {};\n        }}\n",
                call(*then)
            ));
            output.push_str(&format!("        return {};\n    }}\n", call(*else_)));
        }
        Expression::Raise { message } => {
            let message = message.map_or_else(|| "null".to_string(), call);
            output.push_str(&format!(" =>\n        throw global::Ferrule.Runtime.FerruleFailures.MappingException({node}U, {message});\n"));
        }
        Expression::ValueMap {
            input,
            input_type,
            table,
            default,
        } => {
            output.push_str("\n    {\n");
            output.push_str(&format!(
                "        var input_{node} = {};\n        return global::Ferrule.Runtime.FerruleValueMaps.ApplyUserFunction(\n            input_{node}, ",
                call(*input)
            ));
            match input_type {
                Some(value) => output.push_str(&format!(
                    "global::Ferrule.Runtime.FerruleScalarType.{}",
                    scalar_type_name(*value)
                )),
                None => output.push_str("null"),
            }
            output.push_str(",\n            new global::Ferrule.Runtime.FerruleValueMapEntry[]\n            {\n");
            for (from, to) in table {
                output.push_str("                new(");
                output.push_str(&literal::value(node, from)?);
                output.push_str(", ");
                output.push_str(&literal::value(node, to)?);
                output.push_str("),\n");
            }
            output.push_str("            },\n            ");
            match default {
                Some(value) => output.push_str(&literal::value(node, value)?),
                None => output.push_str("null"),
            }
            output.push_str(");\n    }\n");
        }
        _ => {
            return Err(ProgramValidationError::UnsupportedUserFunctionExpression {
                function,
                node,
            }
            .into());
        }
    }
    Ok(())
}

pub(super) fn render_user_function_call(
    node: NodeId,
    function: FunctionId,
    args: &[NodeId],
    functions: &BTreeMap<FunctionId, &UserFunctionProgram>,
    call: impl Fn(NodeId) -> String,
    has_filter_map: bool,
    output: &mut String,
) -> Result<(), EmitError> {
    let Some(definition) = functions.get(&function) else {
        return Err(ProgramValidationError::MissingUserFunction {
            owner: None,
            node,
            function,
        }
        .into());
    };
    // Match ordinary and nested interpreter calls: collect, then adapt.
    for (index, argument) in args.iter().enumerate() {
        output.push_str(&format!(
            "        var raw_argument_{node}_{index} = {};\n",
            call(*argument),
        ));
    }
    if has_filter_map {
        output.push_str(&format!(
            "        var call_context_{node} = context.EnterFilterMapCall({}UL);\n",
            function.get(),
        ));
    }
    for (index, parameter) in definition.parameters.iter().enumerate() {
        output.push_str(&format!(
            "        var argument_{node}_{index} = global::Ferrule.Runtime.FerruleUserFunctions.Adapt(\n            raw_argument_{node}_{index},\n            global::Ferrule.Runtime.FerruleScalarType.{},\n            {}UL,\n            {}UL);\n",
            scalar_type_name(parameter.ty),
            function.get(),
            parameter.id.get(),
        ));
    }
    output.push_str(&format!(
        "        return UserFunction_{}({}, new global::Ferrule.Runtime.FerruleValue[] {{ ",
        function.get(),
        if has_filter_map {
            format!("call_context_{node}")
        } else {
            "context".to_string()
        }
    ));
    for index in 0..args.len() {
        if index != 0 {
            output.push_str(", ");
        }
        output.push_str(&format!("argument_{node}_{index}"));
    }
    output.push_str(" });\n");
    Ok(())
}

pub(super) fn render_delimited_text_field(
    parser: &codegen::DelimitedTextField,
    input: &str,
    output: &mut String,
) {
    match parser.profile() {
        codegen::FlexTextFieldProfile::Delimited {
            field_separator,
            record_separator,
            quote,
            escape,
        } => {
            output.push_str("global::Ferrule.Runtime.FerruleDelimitedText.ParseField(");
            output.push_str(input);
            output.push_str(", ");
            output.push_str(&literal::string(field_separator));
            output.push_str(", ");
            output.push_str(&literal::string(record_separator));
            output.push_str(", ");
            output.push_str(&literal::string(quote));
            output.push_str(", ");
            output.push_str(&literal::string(escape));
        }
        codegen::FlexTextFieldProfile::FixedWidth {
            widths,
            fill_char,
            record_delimiters,
            treat_empty_as_absent,
        } => {
            output.push_str("global::Ferrule.Runtime.FerruleDelimitedText.ParseFixedWidthField(");
            output.push_str(input);
            output.push_str(", ");
            output.push_str(&literal::string(&fill_char.to_string()));
            output.push_str(", ");
            output.push_str(if *record_delimiters { "true" } else { "false" });
            output.push_str(", ");
            output.push_str(if *treat_empty_as_absent {
                "true"
            } else {
                "false"
            });
            output.push_str(", new uint[] { ");
            for (index, width) in widths.iter().enumerate() {
                if index != 0 {
                    output.push_str(", ");
                }
                output.push_str(&format!("{width}U"));
            }
            output.push_str(" }");
        }
    }
    output.push_str(", new global::Ferrule.Runtime.FerruleScalarType[] { ");
    for (index, field) in parser.fields().iter().enumerate() {
        if index != 0 {
            output.push_str(", ");
        }
        output.push_str("global::Ferrule.Runtime.FerruleScalarType.");
        output.push_str(scalar_type_name(*field));
    }
    output.push_str(&format!(" }}, {}U)", parser.selected()));
}

#[derive(Clone, Copy)]
pub(super) enum ExpressionContext {
    Legacy,
    Basic,
    PublicBasicOwnedNodes,
}

// The Legacy branch retains the ordinary source bytes and scalar evaluation order.
pub(super) fn render_expression(
    node: NodeId,
    expression: &Expression,
    functions: &BTreeMap<FunctionId, &UserFunctionProgram>,
    has_filter_map: bool,
    mode: ExpressionContext,
    output: &mut String,
) -> Result<(), EmitError> {
    let (context_type, legacy_receiver, position_reader) = match mode {
        ExpressionContext::Legacy => (
            "global::Ferrule.Runtime.ScopeContext",
            "context",
            "Position",
        ),
        ExpressionContext::Basic | ExpressionContext::PublicBasicOwnedNodes => (
            "global::Ferrule.Runtime.FerruleBasicScopeContext",
            "context.LegacyView",
            "LegacyPosition",
        ),
    };
    if matches!(
        mode,
        ExpressionContext::Basic | ExpressionContext::PublicBasicOwnedNodes
    ) {
        if matches!(mode, ExpressionContext::PublicBasicOwnedNodes)
            && matches!(
                expression,
                Expression::OwnedSourceField(_)
                    | Expression::OwnedPosition(_)
                    | Expression::OwnedRowDocumentPath(_)
                    | Expression::OwnedSourceDocumentPath(_)
                    | Expression::OwnedDynamicSourceField(_)
                    | Expression::OwnedXmlSerialize(_)
            )
        {
            output.push_str(&format!("\n    private static global::Ferrule.Runtime.FerruleValue Node_{node}(\n        {context_type} context)\n    {{\n        ObserveOwnedBasicNode({node}U, context);\n        try {{ return NodeBody_{node}(context); }}\n        catch (global::Ferrule.Runtime.FerruleRuntimeException error)\n        {{\n            error.WithOwnedSourceNode({node}U);\n            throw;\n        }}\n    }}\n"));
        } else {
            output.push_str(&format!("\n    private static global::Ferrule.Runtime.FerruleValue Node_{node}(\n        {context_type} context)\n    {{\n        ObserveOwnedBasicNode({node}U, context);\n        return NodeBody_{node}(context);\n    }}\n"));
        }
    }
    let method = match mode {
        ExpressionContext::Legacy if has_filter_map => format!("NodeCore_{node}"),
        ExpressionContext::Legacy => format!("Node_{node}"),
        ExpressionContext::Basic | ExpressionContext::PublicBasicOwnedNodes => {
            format!("NodeBody_{node}")
        }
    };
    output.push('\n');
    output.push_str(&format!(
        "    private static global::Ferrule.Runtime.FerruleValue {method}(\n        {context_type} context)",
    ));
    match expression {
        Expression::OwnedLookup(_) | Expression::OwnedCollectionFind(_)
            if matches!(
                mode,
                ExpressionContext::Basic | ExpressionContext::PublicBasicOwnedNodes
            ) =>
        {
            return crate::owned_basic::render_owned_search(node, expression, output);
        }
        Expression::OwnedAggregate(aggregate)
            if matches!(
                mode,
                ExpressionContext::Basic | ExpressionContext::PublicBasicOwnedNodes
            ) =>
        {
            crate::owned_basic::render_owned_aggregate(node, aggregate, output);
            return Ok(());
        }
        Expression::OwnedXmlMixedContent(mixed)
            if matches!(
                mode,
                ExpressionContext::Basic | ExpressionContext::PublicBasicOwnedNodes
            ) =>
        {
            crate::owned_basic::render_owned_xml_mixed_content(node, mixed, output);
            return Ok(());
        }
        Expression::OwnedJoinAggregate(aggregate)
            if matches!(
                mode,
                ExpressionContext::Basic | ExpressionContext::PublicBasicOwnedNodes
            ) =>
        {
            owned_join::render_aggregate(node, aggregate, output);
            return Ok(());
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
            if matches!(
                mode,
                ExpressionContext::Basic | ExpressionContext::PublicBasicOwnedNodes
            ) {
                return crate::owned_basic::render_owned_leaf(node, expression, output);
            }
            return Err(codegen::owned_expression_emission_error(
                codegen::OwnedSourceNamespace::Main,
                node,
                expression,
            )
            .into());
        }
        Expression::SourceField { frame, path } => {
            if matches!(
                mode,
                ExpressionContext::Basic | ExpressionContext::PublicBasicOwnedNodes
            ) {
                output.push_str(" =>\n        context.ResolveLegacyField(");
                render_path(path, output);
                output.push_str(", ");
                render_optional_path(frame.as_deref(), output);
                output.push_str(");\n");
                return Ok(());
            }
            output.push_str(" =>\n        ");
            if let Some(frame) = frame {
                output.push_str("context.ResolveSourceFieldInFrame(");
                render_path(frame, output);
                output.push_str(", ");
                render_path(path, output);
            } else {
                output.push_str("context.ResolveSourceField(");
                render_path(path, output);
            }
            output.push_str(");\n");
        }
        Expression::SourceRootXmlTypeEquals {
            canonical_expanded_type,
        } => {
            output.push_str(&format!(
                " =>\n        context.ResolveSourceRootXmlTypeEquals({node}U, {});\n",
                literal::string(canonical_expanded_type)
            ));
        }
        Expression::SourceRootField { path, required } => {
            let reader = if *required {
                "ResolveRequiredSourceRootField"
            } else {
                "ResolveSourceRootField"
            };
            output.push_str(&format!(" =>\n        context.{reader}({node}U, "));
            render_path(path, output);
            output.push_str(");\n");
        }
        Expression::DynamicSourceField { object, frame, key } => {
            output.push_str("\n    {\n        var key = Node_");
            output.push_str(&key.to_string());
            output.push_str("(context);\n        return context.ResolveDynamicScalar(");
            match frame {
                Some(frame) => render_path(frame, output),
                None => output.push_str("null"),
            }
            output.push_str(", ");
            render_path(object, output);
            output.push_str(", key);\n    }\n");
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
                codegen::serialize_embedded_schema(schema, codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES)?;
            output.push_str("\n    {\n        var instance = context.ResolveXmlInstance(");
            match frame {
                Some(frame) => render_path(frame, output),
                None => output.push_str("null"),
            }
            output.push_str(", ");
            render_path(path, output);
            output.push_str("\n        );\n        return global::Ferrule.Runtime.FerruleXml.SerializeEmbedded(\n            ");
            output.push_str(&format!("{node}U, "));
            output.push_str(&literal::string(&schema));
            output.push_str(", instance, ");
            output.push_str(if *declaration { "true" } else { "false" });
            output.push_str(", ");
            output.push_str(if *indent { "true" } else { "false" });
            output.push_str(", ");
            match namespace {
                Some(namespace) => output.push_str(&literal::string(namespace)),
                None => output.push_str("null"),
            }
            output.push_str(");\n    }\n");
        }
        Expression::XmlMixedContent {
            frame,
            path,
            replacements,
        } => {
            output.push_str(
                "\n    {\n        return global::Ferrule.Runtime.FerruleXmlMixedContent.Evaluate(\n            context,\n            ",
            );
            match frame {
                Some(frame) => render_path(frame, output),
                None => output.push_str("null"),
            }
            output.push_str(",\n            ");
            render_path(path, output);
            output.push_str(
                ",\n            new global::Ferrule.Runtime.FerruleXmlMixedContentReplacement[]\n            {\n",
            );
            for replacement in replacements {
                output.push_str("                new(");
                output.push_str(&literal::string(&replacement.element));
                output.push_str(", ");
                render_path(&replacement.collection, output);
                output.push_str(&format!(", Node_{}),\n", replacement.expression));
            }
            output.push_str("            });\n    }\n");
        }
        Expression::SourceDocumentPath => {
            output.push_str(" =>\n        context.ResolveSourceDocumentPath();\n");
        }
        Expression::Position { collection } => {
            output.push_str(&format!(
                " =>\n        global::Ferrule.Runtime.FerruleValue.FromInt64(context.{position_reader}(",
            ));
            render_path(collection, output);
            output.push_str("));\n");
        }
        Expression::GeneratedPosition { item } => {
            output.push_str(&format!(
                " =>\n        context.GeneratedPosition({node}U, {item}U);\n"
            ));
        }
        Expression::JoinField {
            join,
            collection,
            path,
        } => {
            output.push_str(&format!(
                " =>\n        context.ResolveJoinScalar({}UL, ",
                join.get()
            ));
            render_path(collection, output);
            output.push_str(", ");
            render_path(path, output);
            output.push_str(");\n");
        }
        Expression::JoinPosition { join }
            if matches!(
                mode,
                ExpressionContext::Basic | ExpressionContext::PublicBasicOwnedNodes
            ) =>
        {
            output.push_str(&format!(
                " =>\n        global::Ferrule.Runtime.FerruleValue.FromInt64(context.OwnedJoinPosition({}UL));\n",
                join.get()
            ));
        }
        Expression::JoinPosition { join } => {
            output.push_str(&format!(
                " =>\n        global::Ferrule.Runtime.FerruleValue.FromInt64(context.JoinPosition({}UL));\n",
                join.get()
            ));
        }
        Expression::Const { value } => {
            output.push_str(" =>\n        ");
            output.push_str(&literal::value(node, value)?);
            output.push_str(";\n");
        }
        Expression::FunctionParameter { parameter } => {
            return Err(ProgramValidationError::FunctionParameterInMain {
                node,
                parameter: *parameter,
            }
            .into());
        }
        Expression::RuntimeValue { value } => {
            output.push_str(&format!(
                " =>\n        {legacy_receiver}.ResolveRuntimeValue("
            ));
            output.push_str("global::Ferrule.Runtime.FerruleRuntimeValue.");
            output.push_str(runtime_value_name(*value));
            output.push_str(");\n");
        }
        Expression::RuntimeParameter { name, ty } => {
            output.push_str(&format!(
                " =>\n        {legacy_receiver}.ResolveRuntimeParameter("
            ));
            output.push_str(&format!("{node}U, "));
            output.push_str(&literal::string(name));
            output.push_str(", global::Ferrule.Runtime.FerruleScalarType.");
            output.push_str(scalar_type_name(*ty));
            output.push_str(");\n");
        }
        Expression::RuntimeParameterDefault { name, ty, default } => {
            output.push_str(&format!(
                " =>\n        {legacy_receiver}.ResolveRuntimeParameterOrDefault("
            ));
            output.push_str(&format!("{node}U, "));
            output.push_str(&literal::string(name));
            output.push_str(", global::Ferrule.Runtime.FerruleScalarType.");
            output.push_str(scalar_type_name(*ty));
            output.push_str(&format!(", () => Node_{default}(context));\n"));
        }
        Expression::InvocationCounter {
            key,
            start,
            step,
            restart,
        } => {
            let restart = restart.map_or_else(
                || "global::Ferrule.Runtime.FerruleValue.Null".to_string(),
                |id| format!("Node_{id}(context)"),
            );
            output.push_str(&format!(
                "\n    {{\n        {legacy_receiver}.RequireInvocationCounters({node}U);\n        var start = global::Ferrule.Runtime.FerruleInvocationCounterOperands.RequireInteger({node}U, \"start\", Node_{start}(context));\n        var step = global::Ferrule.Runtime.FerruleInvocationCounterOperands.RequireInteger({node}U, \"step\", Node_{step}(context));\n        var restart = global::Ferrule.Runtime.FerruleInvocationCounterOperands.RequireRestart({node}U, {restart});\n        return {legacy_receiver}.NextInvocationCounter({node}U, {}, start, step, restart);\n    }}\n",
                literal::string(key)));
        }
        Expression::Call { function, args } => {
            if matches!(
                function.as_str(),
                "json_parse_array_length"
                    | "json_parse_array_field"
                    | "json_parse_field"
                    | "json_parse_object_exists"
            ) && let [source, ..] = args.as_slice()
            {
                output.push_str(&format!(
                    " =>\n        {legacy_receiver}.CallJsonArrayProjection({source}U, "
                ));
            } else {
                output.push_str(" =>\n        global::Ferrule.Runtime.FerruleFunctions.Call(");
            }
            output.push_str(&literal::string(function.as_str()));
            output.push_str(", new global::Ferrule.Runtime.FerruleValue[] { ");
            for (index, argument) in args.iter().enumerate() {
                if index != 0 {
                    output.push_str(", ");
                }
                output.push_str(&format!("Node_{argument}(context)"));
            }
            output.push_str(" });\n");
        }
        Expression::DelimitedTextField { input, parser } => {
            output.push_str(" =>\n        ");
            render_delimited_text_field(parser, &format!("Node_{input}(context)"), output);
            output.push_str(";\n");
        }
        Expression::UserFunctionCall { function, args } => {
            output.push_str("\n    {\n");
            render_user_function_call(
                node,
                *function,
                args,
                functions,
                |argument| format!("Node_{argument}(context)"),
                has_filter_map,
                output,
            )?;
            output.push_str("    }\n");
        }
        Expression::If {
            condition,
            then,
            else_,
        } => {
            output.push_str("\n    {\n");
            output.push_str(&format!(
                "        var condition_{node} = Node_{condition}(context);\n"
            ));
            output.push_str(&format!(
                "        if (global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(condition_{node}, {condition}U))\n        {{\n            return Node_{then}(context);\n        }}\n"
            ));
            output.push_str(&format!("        return Node_{else_}(context);\n    }}\n"));
        }
        Expression::Raise { message } => {
            let message = message.map_or_else(
                || "null".to_string(),
                |message| format!("Node_{message}(context)"),
            );
            output.push_str(&format!(" =>\n        throw global::Ferrule.Runtime.FerruleFailures.MappingException({node}U, {message});\n"));
        }
        Expression::ValueMap {
            input,
            input_type,
            table,
            default,
        } => {
            output.push_str("\n    {\n");
            let normalization = if input_type.is_some() {
                format!(
                    "        if (input_{node}.Kind == global::Ferrule.Runtime.FerruleValueKind.JsonNull)\n        {{\n            input_{node} = global::Ferrule.Runtime.FerruleValue.Null;\n        }}\n"
                )
            } else {
                String::new()
            };
            output.push_str(&format!(
                "        var input_{node} = Node_{input}(context);\n{normalization}        return global::Ferrule.Runtime.FerruleValueMaps.Apply(\n            input_{node}, "
            ));
            match input_type {
                Some(value) => output.push_str(&format!(
                    "global::Ferrule.Runtime.FerruleScalarType.{}",
                    scalar_type_name(*value)
                )),
                None => output.push_str("null"),
            }
            output.push_str(",\n            new global::Ferrule.Runtime.FerruleValueMapEntry[]\n            {\n");
            for (from, to) in table {
                output.push_str("                new(");
                output.push_str(&literal::value(node, from)?);
                output.push_str(", ");
                output.push_str(&literal::value(node, to)?);
                output.push_str("),\n");
            }
            output.push_str("            },\n            ");
            match default {
                Some(value) => output.push_str(&literal::value(node, value)?),
                None => output.push_str("null"),
            }
            output.push_str(");\n    }\n");
        }
        Expression::Lookup {
            collection,
            key,
            matches,
            value,
        } => {
            output.push_str("\n    {\n");
            output.push_str(&format!(
                "        var lookup_match_{node} = Node_{matches}(context);\n        return context.Lookup("
            ));
            render_path(collection, output);
            output.push_str(", ");
            render_path(key, output);
            output.push_str(&format!(", lookup_match_{node}, "));
            render_path(value, output);
            output.push_str(");\n    }\n");
        }
        Expression::CollectionFind {
            collection,
            predicate,
            value,
        } => {
            output.push_str("\n    {\n");
            output.push_str(&format!(
                "        var find_items_{node} = context.CollectionFindItems("
            ));
            render_path(collection, output);
            output.push_str(&format!(
                ");\n        foreach (var find_context_{node} in find_items_{node})\n        {{\n            var find_predicate_{node} = Node_{predicate}(find_context_{node});\n            if (find_predicate_{node}.Kind == global::Ferrule.Runtime.FerruleValueKind.Bool)\n            {{\n                if (find_predicate_{node}.BooleanValue)\n                {{\n                    return Node_{value}(find_context_{node});\n                }}\n                continue;\n            }}\n            if (find_predicate_{node}.Kind is global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull or global::Ferrule.Runtime.FerruleValueKind.XmlNil)\n            {{\n                continue;\n            }}\n            _ = global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(find_predicate_{node}, {predicate}U);\n        }}\n        return global::Ferrule.Runtime.FerruleValue.Null;\n    }}\n"
            ));
        }
        Expression::Aggregate {
            function,
            collection,
            value,
            arg,
        } => {
            let collect = aggregate_collect_prefix(*function, &format!("values_{node}"));
            output.push_str("\n    {\n");
            output.push_str(&format!(
                "        var items_{node} = context.AggregateItems("
            ));
            render_path(collection, output);
            output.push_str(&format!(
                ");\n        var values_{node} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleValue>(items_{node}.Count);\n        foreach (var item_context_{node} in items_{node})\n        {{\n"
            ));
            match value {
                AggregateValue::Path(path) => {
                    output.push_str(&format!(
                        "            {collect}item_context_{node}.AggregateCurrentScalar("
                    ));
                    render_path(path, output);
                    output.push_str("));\n");
                }
                AggregateValue::Expression(expression) => output.push_str(&format!(
                    "            {collect}Node_{expression}(item_context_{node}));\n"
                )),
            }
            output.push_str("        }\n");
            match arg {
                Some(arg) => output.push_str(&format!(
                    "        global::Ferrule.Runtime.FerruleValue? argument_{node} = Node_{arg}(context);\n"
                )),
                None => output.push_str(&format!(
                    "        global::Ferrule.Runtime.FerruleValue? argument_{node} = null;\n"
                )),
            }
            output.push_str(&format!(
                "        return global::Ferrule.Runtime.FerruleAggregates.Apply(\n            global::Ferrule.Runtime.FerruleAggregateOperation.{}, values_{node}, argument_{node});\n    }}\n",
                aggregate_function_name(*function)
            ));
        }
        Expression::JoinAggregate {
            function,
            join,
            expression,
            arg,
        } => {
            let collect = aggregate_collect_prefix(*function, &format!("values_{node}"));
            output.push_str("\n    {\n");
            output.push_str(&format!("        var tuple_contexts_{node} = "));
            render_inner_join_call(join, output);
            output.push_str(&format!(
                ";\n        var values_{node} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleValue>(tuple_contexts_{node}.Count);\n        foreach (var tuple_context_{node} in tuple_contexts_{node})\n        {{\n"
            ));
            match expression {
                Some(expression) => output.push_str(&format!(
                    "            {collect}Node_{expression}(tuple_context_{node}));\n"
                )),
                None => output.push_str(&format!(
                    "            {collect}global::Ferrule.Runtime.FerruleValue.Null);\n"
                )),
            }
            output.push_str("        }\n");
            match arg {
                Some(arg) => output.push_str(&format!(
                    "        global::Ferrule.Runtime.FerruleValue? argument_{node} = Node_{arg}(context);\n"
                )),
                None => output.push_str(&format!(
                    "        global::Ferrule.Runtime.FerruleValue? argument_{node} = null;\n"
                )),
            }
            output.push_str(&format!(
                "        return global::Ferrule.Runtime.FerruleAggregates.Apply(\n            global::Ferrule.Runtime.FerruleAggregateOperation.{}, values_{node}, argument_{node});\n    }}\n",
                aggregate_function_name(*function)
            ));
        }
        Expression::SequenceExists {
            sequence,
            predicate,
        } => {
            output.push_str("\n    {\n");
            let identifier = format!("node_{node}");
            render_generated_values(&identifier, sequence, output)?;
            output.push_str(&format!(
                "        foreach (var sequence_context_{identifier} in context.EnumerateGenerated(sequence_values_{identifier}, {}U))\n        {{\n            var sequence_predicate_{identifier} = Node_{predicate}(sequence_context_{identifier});\n            if (global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(sequence_predicate_{identifier}, {predicate}U))\n            {{\n                return global::Ferrule.Runtime.FerruleValue.FromBoolean(true);\n            }}\n        }}\n        return global::Ferrule.Runtime.FerruleValue.FromBoolean(false);\n    }}\n", sequence.item()
            ));
        }
        Expression::SequenceItemAt { sequence, index } => {
            // Pure producer arguments and the index use this same receiving
            // context overload. The explicit Basic ItemAt inventory admits the
            // four scalar producers and, under its own profile, the owned
            // recursive walk; no generated item context is attached.
            output.push_str("\n    {\n");
            let identifier = format!("node_{node}");
            if matches!(mode, ExpressionContext::Legacy) || sequence.owned_source().is_none() {
                render_generated_values(&identifier, sequence, output)?;
            } else {
                owned_recursive_collect::render_values(node, &identifier, sequence, output);
            }
            output.push_str(&format!(
                "        var sequence_index_{identifier} = Node_{index}(context);\n        return global::Ferrule.Runtime.FerruleAggregates.Apply(\n            global::Ferrule.Runtime.FerruleAggregateOperation.ItemAt, sequence_values_{identifier}, sequence_index_{identifier});\n    }}\n"
            ));
        }
        Expression::SequenceAggregate {
            function,
            sequence,
            predicate,
            expression,
            arg,
        } => {
            output.push_str("\n    {\n");
            let identifier = format!("node_{node}");
            render_generated_values(&identifier, sequence, output)?;
            let collect =
                aggregate_collect_prefix(*function, &format!("aggregate_values_{identifier}"));
            output.push_str(&format!(
                "        var aggregate_values_{identifier} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleValue>();\n        foreach (var sequence_context_{identifier} in context.EnumerateGenerated(sequence_values_{identifier}, {}U))\n        {{\n", sequence.item()
            ));
            if let Some(predicate) = predicate {
                output.push_str(&format!(
                    "            var sequence_predicate_{identifier} = Node_{predicate}(sequence_context_{identifier});\n            if (!global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(sequence_predicate_{identifier}, {predicate}U))\n            {{\n                continue;\n            }}\n"
                ));
            }
            output.push_str(&format!(
                "            {collect}Node_{}(sequence_context_{identifier}));\n        }}\n",
                expression.unwrap_or(sequence.item())
            ));
            match arg {
                Some(arg) => output.push_str(&format!(
                    "        global::Ferrule.Runtime.FerruleValue? aggregate_argument_{identifier} = Node_{arg}(context);\n"
                )),
                None => output.push_str(&format!(
                    "        global::Ferrule.Runtime.FerruleValue? aggregate_argument_{identifier} = null;\n"
                )),
            }
            output.push_str(&format!(
                "        return global::Ferrule.Runtime.FerruleAggregates.Apply(\n            global::Ferrule.Runtime.FerruleAggregateOperation.{}, aggregate_values_{identifier}, aggregate_argument_{identifier});\n    }}\n",
                aggregate_function_name(*function)
            ));
        }
    }
    Ok(())
}

pub(crate) fn render_basic_expression(
    node: NodeId,
    expression: &Expression,
    output: &mut String,
) -> Result<(), EmitError> {
    render_expression(
        node,
        expression,
        &BTreeMap::new(),
        false,
        ExpressionContext::Basic,
        output,
    )
}

pub(crate) fn render_public_basic_expression(
    node: NodeId,
    expression: &Expression,
    output: &mut String,
) -> Result<(), EmitError> {
    render_expression(
        node,
        expression,
        &BTreeMap::new(),
        false,
        ExpressionContext::PublicBasicOwnedNodes,
        output,
    )
}

// Use context overloads, not textual identifier replacement: paths and literal
// values may themselves contain Node_, Scope_, or ScopeItem_.
pub(crate) fn render_legacy_basic_expression(
    node: NodeId,
    expression: &Expression,
    output: &mut String,
) -> Result<(), EmitError> {
    render_expression(
        node,
        expression,
        &BTreeMap::new(),
        false,
        ExpressionContext::Legacy,
        output,
    )
}
