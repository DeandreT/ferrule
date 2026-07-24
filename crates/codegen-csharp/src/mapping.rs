use std::collections::BTreeMap;

use ::mapping::{FunctionId, FunctionParameterId, NodeId};
use codegen::{
    AggregateFunction, AggregateValue, Binding, Expression, GeneratedSequence, GroupingPlan,
    InnerJoin, IterationOutput, IterationPlan, IterationSource, JoinSource, JoinSourceCardinality,
    Program, ProgramValidationError, SequenceWindow, SortFilterOrder, TargetConstruction,
    TargetScope, UserFunctionProgram,
};
use ir::ScalarType;

use crate::{EmitError, literal};

mod failures;

struct ScopePlan<'a> {
    repeating: bool,
    iteration: Option<&'a IterationPlan>,
    construction: &'a TargetConstruction,
    evaluations: Vec<u32>,
    bindings: Vec<BindingPlan<'a>>,
    children: Vec<(&'a str, usize)>,
    segments: Vec<usize>,
}

struct BindingPlan<'a> {
    target_field: &'a str,
    target_type: ScalarType,
    repeating: bool,
    values: Vec<usize>,
}

pub(crate) fn render(program: &Program) -> Result<String, EmitError> {
    let mut expressions = BTreeMap::new();
    for node in &program.expressions {
        expressions.insert(node.id, &node.expression);
    }
    let functions = program
        .user_functions
        .iter()
        .map(|function| (function.id, function))
        .collect::<BTreeMap<_, _>>();

    let mut scopes = Vec::new();
    let primary_scope = add_scope(&program.root, &mut scopes);
    let extra_scopes = program
        .extra_targets
        .iter()
        .map(|target| (target.name.as_str(), add_scope(&target.root, &mut scopes)))
        .collect::<Vec<_>>();

    let mut output = String::from(
        "namespace Ferrule.Generated;\n\npublic sealed record NamedInput(\n    string Name,\n    global::Ferrule.Runtime.FerruleInstance Instance);\n\npublic sealed record NamedOutput(\n    string Name,\n    global::Ferrule.Runtime.FerruleInstance Instance);\n\npublic sealed record ExecutionOutputs(\n    global::Ferrule.Runtime.FerruleInstance Primary,\n    global::System.Collections.Generic.IReadOnlyList<NamedOutput> Extras);\n\npublic sealed record NamedJsonInput(\n    string Name,\n    string Document);\n\npublic sealed record NamedJsonOutput(\n    string Name,\n    string Document);\n\npublic sealed record JsonExecutionOutputs(\n    string Primary,\n    global::System.Collections.Generic.IReadOnlyList<NamedJsonOutput> Extras);\n\npublic static class GeneratedMapping\n{\n",
    );
    render_entry_points(program, primary_scope, &extra_scopes, &mut output);
    render_json_entry_points(program, &mut output)?;
    failures::render(&program.failure_rules, &mut output);
    for function in &program.user_functions {
        render_user_function(function, &functions, &mut output)?;
    }

    for (node, expression) in expressions {
        output.push('\n');
        output.push_str(&format!(
            "    private static global::Ferrule.Runtime.FerruleValue Node_{node}(\n        global::Ferrule.Runtime.ScopeContext context)",
        ));
        match expression {
            Expression::SourceField { frame, path } => {
                output.push_str(" =>\n        ");
                if let Some(frame) = frame {
                    output.push_str("context.ResolveScalarInFrame(");
                    render_path(frame, &mut output);
                    output.push_str(", ");
                    render_path(path, &mut output);
                } else {
                    output.push_str("context.ResolveScalar(");
                    render_path(path, &mut output);
                }
                output.push_str(");\n");
            }
            Expression::XmlSerialize {
                frame,
                path,
                schema,
                declaration,
                indent,
                namespace,
            } => {
                let schema = serde_json::to_string(schema)
                    .map_err(|error| EmitError::SchemaSerialization(error.to_string()))?;
                output.push_str("\n    {\n        var instance = context.ResolveXmlInstance(");
                match frame {
                    Some(frame) => render_path(frame, &mut output),
                    None => output.push_str("null"),
                }
                output.push_str(", ");
                render_path(path, &mut output);
                output.push_str("\n        );\n        return global::Ferrule.Runtime.FerruleXml.Serialize(\n            ");
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
                    Some(frame) => render_path(frame, &mut output),
                    None => output.push_str("null"),
                }
                output.push_str(",\n            ");
                render_path(path, &mut output);
                output.push_str(
                    ",\n            new global::Ferrule.Runtime.FerruleXmlMixedContentReplacement[]\n            {\n",
                );
                for replacement in replacements {
                    output.push_str("                new(");
                    output.push_str(&literal::string(&replacement.element));
                    output.push_str(", ");
                    render_path(&replacement.collection, &mut output);
                    output.push_str(&format!(", Node_{}),\n", replacement.expression));
                }
                output.push_str("            });\n    }\n");
            }
            Expression::SourceDocumentPath => {
                output.push_str(" =>\n        context.ResolveSourceDocumentPath();\n");
            }
            Expression::Position { collection } => {
                output.push_str(
                    " =>\n        global::Ferrule.Runtime.FerruleValue.FromInt64(context.Position(",
                );
                render_path(collection, &mut output);
                output.push_str("));\n");
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
                render_path(collection, &mut output);
                output.push_str(", ");
                render_path(path, &mut output);
                output.push_str(");\n");
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
            Expression::Call { function, args } => {
                output.push_str(" =>\n        global::Ferrule.Runtime.FerruleFunctions.Call(");
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
            Expression::UserFunctionCall { function, args } => {
                output.push_str("\n    {\n");
                render_user_function_call(
                    node,
                    *function,
                    args,
                    &functions,
                    |argument| format!("Node_{argument}(context)"),
                    &mut output,
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
            Expression::ValueMap {
                input,
                input_type,
                table,
                default,
            } => {
                output.push_str("\n    {\n");
                output.push_str(&format!(
                    "        var input_{node} = Node_{input}(context);\n        return global::Ferrule.Runtime.FerruleValueMaps.Apply(\n            input_{node}, "
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
                render_path(collection, &mut output);
                output.push_str(", ");
                render_path(key, &mut output);
                output.push_str(&format!(", lookup_match_{node}, "));
                render_path(value, &mut output);
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
                render_path(collection, &mut output);
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
                output.push_str("\n    {\n");
                output.push_str(&format!(
                    "        var items_{node} = context.AggregateItems("
                ));
                render_path(collection, &mut output);
                output.push_str(&format!(
                    ");\n        var values_{node} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleValue>(items_{node}.Count);\n        foreach (var item_context_{node} in items_{node})\n        {{\n"
                ));
                match value {
                    AggregateValue::Path(path) => {
                        output.push_str(&format!(
                            "            values_{node}.Add(item_context_{node}.AggregateCurrentScalar("
                        ));
                        render_path(path, &mut output);
                        output.push_str("));\n");
                    }
                    AggregateValue::Expression(expression) => output.push_str(&format!(
                        "            values_{node}.Add(Node_{expression}(item_context_{node}));\n"
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
                output.push_str("\n    {\n");
                output.push_str(&format!("        var tuple_contexts_{node} = "));
                render_inner_join_call(join, &mut output);
                output.push_str(&format!(
                    ";\n        var values_{node} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleValue>(tuple_contexts_{node}.Count);\n        foreach (var tuple_context_{node} in tuple_contexts_{node})\n        {{\n"
                ));
                match expression {
                    Some(expression) => output.push_str(&format!(
                        "            values_{node}.Add(Node_{expression}(tuple_context_{node}));\n"
                    )),
                    None => output.push_str(&format!(
                        "            values_{node}.Add(global::Ferrule.Runtime.FerruleValue.Null);\n"
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
                render_generated_values(&identifier, sequence, &mut output);
                output.push_str(&format!(
                    "        foreach (var sequence_context_{identifier} in context.EnumerateGenerated(sequence_values_{identifier}))\n        {{\n            var sequence_predicate_{identifier} = Node_{predicate}(sequence_context_{identifier});\n            if (global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(sequence_predicate_{identifier}, {predicate}U))\n            {{\n                return global::Ferrule.Runtime.FerruleValue.FromBoolean(true);\n            }}\n        }}\n        return global::Ferrule.Runtime.FerruleValue.FromBoolean(false);\n    }}\n"
                ));
            }
            Expression::SequenceItemAt { sequence, index } => {
                output.push_str("\n    {\n");
                let identifier = format!("node_{node}");
                render_generated_values(&identifier, sequence, &mut output);
                output.push_str(&format!(
                    "        var sequence_index_{identifier} = Node_{index}(context);\n        return global::Ferrule.Runtime.FerruleAggregates.Apply(\n            global::Ferrule.Runtime.FerruleAggregateOperation.ItemAt, sequence_values_{identifier}, sequence_index_{identifier});\n    }}\n"
                ));
            }
        }
    }

    for (scope_index, scope) in scopes.iter().enumerate() {
        output.push('\n');
        output.push_str(&format!(
            "    private static global::Ferrule.Runtime.FerruleInstance Scope_{scope_index}(\n        global::Ferrule.Runtime.ScopeContext context)\n    {{\n"
        ));
        if let Some(iteration) = scope.iteration {
            if iteration.concatenated().is_some() {
                render_concatenated_scope(iteration, &scope.segments, &mut output);
            } else {
                render_iteration_scope(scope_index, iteration, &mut output);
            }
        } else {
            output.push_str(&format!(
                "        var item_{scope_index} = ScopeItem_{scope_index}(context);\n"
            ));
            if scope.repeating {
                output.push_str(&format!(
                    "        return new global::Ferrule.Runtime.FerruleRepeated(new global::Ferrule.Runtime.FerruleInstance[] {{ item_{scope_index} }});\n"
                ));
            } else {
                output.push_str(&format!("        return item_{scope_index};\n"));
            }
        }
        output.push_str("    }\n\n");
        output.push_str(&format!(
            "    private static global::Ferrule.Runtime.FerruleInstance ScopeItem_{scope_index}(\n        global::Ferrule.Runtime.ScopeContext context)\n    {{\n"
        ));
        if let TargetConstruction::Scalar { expression } = scope.construction {
            output.push_str(&format!(
                "        return new global::Ferrule.Runtime.FerruleScalar(Node_{expression}(context));\n    }}\n"
            ));
            continue;
        }
        if let TargetConstruction::RecursiveFilter {
            children,
            items,
            predicate,
        } = scope.construction
        {
            output.push_str(
                "        return global::Ferrule.Runtime.FerruleRecursiveFilter.Apply(\n            context,\n            ",
            );
            output.push_str(&literal::string(children));
            output.push_str(",\n            ");
            output.push_str(&literal::string(items));
            output.push_str(&format!(
                ",\n            {predicate}U,\n            Node_{predicate});\n    }}\n"
            ));
            continue;
        }
        if let TargetConstruction::PathHierarchy {
            collection,
            separator,
            directories,
            files,
            name,
        } = scope.construction
        {
            output.push_str(
                "        return global::Ferrule.Runtime.FerrulePathHierarchy.Build(\n            context,\n            ",
            );
            render_path(collection, &mut output);
            output.push_str(",\n            ");
            output.push_str(&literal::string(separator));
            output.push_str(",\n            ");
            output.push_str(&literal::string(directories));
            output.push_str(",\n            ");
            output.push_str(&literal::string(files));
            output.push_str(",\n            ");
            output.push_str(&literal::string(name));
            output.push_str(");\n    }\n");
            continue;
        }
        if let TargetConstruction::AdjacencyTree {
            collection,
            key,
            parent,
            target_key,
            target_children,
            root,
        } = scope.construction
        {
            output.push_str(
                "        return global::Ferrule.Runtime.FerruleAdjacencyTree.Build(\n            context,\n            ",
            );
            render_path(collection, &mut output);
            output.push_str(",\n            ");
            render_path(key, &mut output);
            output.push_str(",\n            ");
            render_path(parent, &mut output);
            output.push_str(",\n            ");
            output.push_str(&literal::string(target_key));
            output.push_str(",\n            ");
            output.push_str(&literal::string(target_children));
            output.push_str(",\n            ");
            match root {
                Some(root) => output.push_str(&format!("Node_{root}")),
                None => output.push_str("null"),
            }
            output.push_str(");\n    }\n");
            continue;
        }
        if matches!(scope.construction, TargetConstruction::CopyCurrentSource) {
            output.push_str("        return context.CopyCurrentGroup();\n    }\n");
            continue;
        }
        for (binding_index, expression) in scope.evaluations.iter().enumerate() {
            output.push_str(&format!(
                "        var value_{scope_index}_{binding_index} = Node_{expression}(context);\n"
            ));
        }
        output.push_str(
            &format!(
                "        var group_{scope_index} = new global::Ferrule.Runtime.FerruleGroup(new global::Ferrule.Runtime.FerruleField[]\n        {{\n"
            ),
        );
        for binding in &scope.bindings {
            output.push_str("            new global::Ferrule.Runtime.FerruleField(");
            output.push_str(&literal::string(binding.target_field));
            output.push_str(", ");
            if binding.repeating {
                output.push_str(
                    "TargetBuilder.RepeatedScalar(new global::Ferrule.Runtime.FerruleValue[] { ",
                );
                for (index, binding_index) in binding.values.iter().enumerate() {
                    if index != 0 {
                        output.push_str(", ");
                    }
                    output.push_str(&format!("value_{scope_index}_{binding_index}"));
                }
                output.push_str(" }, ");
            } else {
                output.push_str(&format!(
                    "TargetBuilder.Scalar(value_{scope_index}_{}, ",
                    binding.values[0]
                ));
            }
            output.push_str(target_type(binding.target_type));
            output.push_str(")),\n");
        }
        for (target_field, child_index) in &scope.children {
            output.push_str("            new global::Ferrule.Runtime.FerruleField(");
            output.push_str(&literal::string(target_field));
            output.push_str(&format!(", Scope_{child_index}(context)),\n"));
        }
        output.push_str("        });\n");
        if let TargetConstruction::XmlMixedContent { elements } = scope.construction {
            output.push_str(&format!(
                "        return global::Ferrule.Runtime.FerruleXmlMixedContent.Preserve(\n            context,\n            group_{scope_index},\n            new global::Ferrule.Runtime.FerruleXmlMixedContentElement[]\n            {{\n"
            ));
            for element in elements {
                output.push_str("                new(");
                output.push_str(&literal::string(&element.source));
                output.push_str(", ");
                output.push_str(&literal::string(&element.target));
                output.push_str("),\n");
            }
            output.push_str("            });\n");
        } else {
            output.push_str(&format!("        return group_{scope_index};\n"));
        }
        output.push_str("    }\n");
    }
    output.push_str("}\n");
    Ok(output)
}

fn render_user_function(
    function: &UserFunctionProgram,
    functions: &BTreeMap<FunctionId, &UserFunctionProgram>,
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
    Ok(())
}

fn render_user_function_expression(
    function: FunctionId,
    node: NodeId,
    expression: &Expression,
    parameters: &BTreeMap<FunctionParameterId, usize>,
    functions: &BTreeMap<FunctionId, &UserFunctionProgram>,
    output: &mut String,
) -> Result<(), EmitError> {
    let call = |dependency: NodeId| {
        format!(
            "UserFunction_{}_Node_{dependency}(context, parameters)",
            function.get()
        )
    };
    output.push_str(&format!(
        "\n    private static global::Ferrule.Runtime.FerruleValue UserFunction_{}_Node_{node}(\n        global::Ferrule.Runtime.ScopeContext context,\n        global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.FerruleValue> parameters)",
        function.get()
    ));
    match expression {
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
        Expression::UserFunctionCall {
            function: called,
            args,
        } => {
            output.push_str("\n    {\n");
            render_user_function_call(node, *called, args, functions, call, output)?;
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
        Expression::ValueMap {
            input,
            input_type,
            table,
            default,
        } => {
            output.push_str("\n    {\n");
            output.push_str(&format!(
                "        var input_{node} = {};\n        return global::Ferrule.Runtime.FerruleValueMaps.Apply(\n            input_{node}, ",
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

fn render_user_function_call(
    node: NodeId,
    function: FunctionId,
    args: &[NodeId],
    functions: &BTreeMap<FunctionId, &UserFunctionProgram>,
    call: impl Fn(NodeId) -> String,
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
    for (index, (argument, parameter)) in args.iter().zip(&definition.parameters).enumerate() {
        output.push_str(&format!(
            "        var argument_{node}_{index} = global::Ferrule.Runtime.FerruleUserFunctions.Adapt(\n            {},\n            global::Ferrule.Runtime.FerruleScalarType.{},\n            {}UL,\n            {}UL);\n",
            call(*argument),
            scalar_type_name(parameter.ty),
            function.get(),
            parameter.id.get(),
        ));
    }
    output.push_str(&format!(
        "        return UserFunction_{}(context, new global::Ferrule.Runtime.FerruleValue[] {{ ",
        function.get()
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

fn render_entry_points(
    program: &Program,
    primary_scope: usize,
    extra_scopes: &[(&str, usize)],
    output: &mut String,
) {
    output.push_str(
        "    public static global::Ferrule.Runtime.FerruleInstance Execute(\n        global::Ferrule.Runtime.FerruleInstance source)\n    {\n        return ExecuteWithSources(source, global::System.Array.Empty<NamedInput>());\n    }\n",
    );
    output.push_str(
        "\n    public static global::Ferrule.Runtime.FerruleInstance Execute(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        return ExecuteWithSources(source, global::System.Array.Empty<NamedInput>(), executionContext);\n    }\n",
    );
    output.push_str(
        "\n    public static global::Ferrule.Runtime.FerruleInstance ExecuteWithSources(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources)\n    {\n        return ExecuteOutputsWithSources(source, extraSources).Primary;\n    }\n",
    );
    output.push_str(
        "\n    public static global::Ferrule.Runtime.FerruleInstance ExecuteWithSources(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        return ExecuteOutputsWithSources(source, extraSources, executionContext).Primary;\n    }\n",
    );
    output.push_str(
        "\n    public static ExecutionOutputs ExecuteOutputs(\n        global::Ferrule.Runtime.FerruleInstance source)\n    {\n        return ExecuteOutputsWithSources(source, global::System.Array.Empty<NamedInput>());\n    }\n",
    );
    output.push_str(
        "\n    public static ExecutionOutputs ExecuteOutputs(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        return ExecuteOutputsWithSources(source, global::System.Array.Empty<NamedInput>(), executionContext);\n    }\n",
    );
    output.push_str(
        "\n    public static ExecutionOutputs ExecuteOutputsWithSources(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources)\n    {\n        return ExecuteOutputs(CreateContext(source, extraSources, null));\n    }\n",
    );
    output.push_str(
        "\n    public static ExecutionOutputs ExecuteOutputsWithSources(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(executionContext);\n        return ExecuteOutputs(CreateContext(source, extraSources, executionContext));\n    }\n",
    );
    render_source_context(program, output);
    output.push_str(
        "\n    private static ExecutionOutputs ExecuteOutputs(\n        global::Ferrule.Runtime.ScopeContext context)\n    {\n",
    );
    if !program.failure_rules.is_empty() {
        output.push_str("        EvaluateFailureRules(context);\n");
    }
    output.push_str(&format!(
        "        var primary = Scope_{primary_scope}(context);\n"
    ));
    for (index, (_, scope)) in extra_scopes.iter().enumerate() {
        output.push_str(&format!(
            "        var extra_{index} = Scope_{scope}(context);\n"
        ));
    }
    output.push_str(
        "        return new ExecutionOutputs(\n            primary,\n            new NamedOutput[]\n            {\n",
    );
    for (index, (name, _)) in extra_scopes.iter().enumerate() {
        output.push_str("                new(");
        output.push_str(&literal::string(name));
        output.push_str(&format!(", extra_{index}),\n"));
    }
    output.push_str("            });\n    }\n");
}

fn render_json_entry_points(program: &Program, output: &mut String) -> Result<(), EmitError> {
    let source_schema = serde_json::to_string(&program.source)
        .map_err(|error| EmitError::SchemaSerialization(error.to_string()))?;
    let target_schema = serde_json::to_string(&program.target)
        .map_err(|error| EmitError::SchemaSerialization(error.to_string()))?;
    let extra_source_schemas = program
        .extra_sources
        .iter()
        .map(|source| {
            serde_json::to_string(&source.source)
                .map_err(|error| EmitError::SchemaSerialization(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let extra_target_schemas = program
        .extra_targets
        .iter()
        .map(|target| {
            serde_json::to_string(&target.target)
                .map_err(|error| EmitError::SchemaSerialization(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    output.push_str("\n    private const string SourceJsonSchema = ");
    output.push_str(&literal::string(&source_schema));
    output.push_str(";\n    private const string TargetJsonSchema = ");
    output.push_str(&literal::string(&target_schema));
    output.push_str(";\n");
    if !program.extra_sources.is_empty() {
        output.push_str(
            "    private static readonly string[] ExtraSourceJsonSchemas = new string[]\n    {\n",
        );
        for schema in &extra_source_schemas {
            output.push_str("        ");
            output.push_str(&literal::string(schema));
            output.push_str(",\n");
        }
        output.push_str("    };\n");
    }
    if program.extra_targets.is_empty() {
        output.push_str(
            "    private static readonly string[] ExtraTargetJsonSchemas = global::System.Array.Empty<string>();\n",
        );
    } else {
        output.push_str(
            "    private static readonly string[] ExtraTargetJsonSchemas = new string[]\n    {\n",
        );
        for schema in &extra_target_schemas {
            output.push_str("        ");
            output.push_str(&literal::string(schema));
            output.push_str(",\n");
        }
        output.push_str("    };\n");
    }

    output.push_str(
        "\n    public static string ExecuteJson(string source)\n    {\n        return ExecuteJsonOutputs(source).Primary;\n    }\n\
         \n    public static string ExecuteJson(\n        string source,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        return ExecuteJsonOutputs(source, executionContext).Primary;\n    }\n\
         \n    public static string ExecuteJsonWithSources(\n        string source,\n        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources)\n    {\n        return ExecuteJsonOutputsWithSources(source, extraSources).Primary;\n    }\n\
         \n    public static string ExecuteJsonWithSources(\n        string source,\n        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        return ExecuteJsonOutputsWithSources(source, extraSources, executionContext).Primary;\n    }\n\
         \n    public static JsonExecutionOutputs ExecuteJsonOutputs(string source)\n    {\n        return ExecuteJsonOutputsWithSources(source, global::System.Array.Empty<NamedJsonInput>());\n    }\n\
         \n    public static JsonExecutionOutputs ExecuteJsonOutputs(\n        string source,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        return ExecuteJsonOutputsWithSources(\n            source,\n            global::System.Array.Empty<NamedJsonInput>(),\n            executionContext);\n    }\n\
         \n    public static JsonExecutionOutputs ExecuteJsonOutputsWithSources(\n        string source,\n        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(source);\n        global::System.ArgumentNullException.ThrowIfNull(extraSources);\n        ValidateNamedJsonInputNames(extraSources);\n        var parsedSource = global::Ferrule.Runtime.FerruleJson.Parse(SourceJsonSchema, source);\n        var parsedInputs = ParseNamedJsonInputs(extraSources);\n        return SerializeJsonOutputs(ExecuteOutputsWithSources(parsedSource, parsedInputs));\n    }\n\
         \n    public static JsonExecutionOutputs ExecuteJsonOutputsWithSources(\n        string source,\n        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(source);\n        global::System.ArgumentNullException.ThrowIfNull(extraSources);\n        global::System.ArgumentNullException.ThrowIfNull(executionContext);\n        ValidateNamedJsonInputNames(extraSources);\n        var parsedSource = global::Ferrule.Runtime.FerruleJson.Parse(SourceJsonSchema, source);\n        var parsedInputs = ParseNamedJsonInputs(extraSources);\n        return SerializeJsonOutputs(ExecuteOutputsWithSources(\n            parsedSource,\n            parsedInputs,\n            executionContext));\n    }\n",
    );

    output.push_str(
        "\n    private static void ValidateNamedJsonInputNames(\n        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources)\n    {\n",
    );
    if program.extra_sources.is_empty() {
        output.push_str(
            "        foreach (var extraSource in extraSources)\n        {\n            global::System.ArgumentNullException.ThrowIfNull(extraSource);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);\n            throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,\n                $\"named source '{extraSource.Name}' is not declared by this mapping\",\n                detail: extraSource.Name);\n        }\n",
        );
    } else {
        output.push_str(
            "        var matched = new global::System.Collections.Generic.HashSet<string>(global::System.StringComparer.Ordinal);\n        foreach (var extraSource in extraSources)\n        {\n            global::System.ArgumentNullException.ThrowIfNull(extraSource);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Document);\n",
        );
        output.push_str("            if (extraSource.Name is not (");
        for (index, source) in program.extra_sources.iter().enumerate() {
            if index != 0 {
                output.push_str(" or ");
            }
            output.push_str(&literal::string(&source.name));
        }
        output.push_str(
            "))\n            {\n                throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                    global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,\n                    $\"named source '{extraSource.Name}' is not declared by this mapping\",\n                    detail: extraSource.Name);\n            }\n            if (!matched.Add(extraSource.Name))\n            {\n                throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                    global::Ferrule.Runtime.FerruleRuntimeError.DuplicateNamedSource,\n                    $\"named source '{extraSource.Name}' was supplied more than once\",\n                    detail: extraSource.Name);\n            }\n        }\n",
        );
        for source in &program.extra_sources {
            let name = literal::string(&source.name);
            output.push_str(&format!(
                "        if (!matched.Contains({name}))\n        {{\n            throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                global::Ferrule.Runtime.FerruleRuntimeError.MissingNamedSource,\n                \"named source \" + {name} + \" is required by this mapping\",\n                detail: {name});\n        }}\n"
            ));
        }
    }
    output.push_str("    }\n");

    output.push_str(
        "\n    private static global::System.Collections.Generic.IReadOnlyList<NamedInput> ParseNamedJsonInputs(\n        global::System.Collections.Generic.IReadOnlyList<NamedJsonInput> extraSources)\n    {\n",
    );
    if program.extra_sources.is_empty() {
        output.push_str(
            "        foreach (var extraSource in extraSources)\n        {\n            global::System.ArgumentNullException.ThrowIfNull(extraSource);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);\n            throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,\n                $\"named source '{extraSource.Name}' is not declared by this mapping\",\n                detail: extraSource.Name);\n        }\n        return global::System.Array.Empty<NamedInput>();\n",
        );
    } else {
        output.push_str(
            "        var parsed = new global::System.Collections.Generic.List<NamedInput>(extraSources.Count);\n        foreach (var extraSource in extraSources)\n        {\n            global::System.ArgumentNullException.ThrowIfNull(extraSource);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Document);\n            var schema = extraSource.Name switch\n            {\n",
        );
        for (index, source) in program.extra_sources.iter().enumerate() {
            output.push_str("                ");
            output.push_str(&literal::string(&source.name));
            output.push_str(&format!(" => ExtraSourceJsonSchemas[{index}],\n"));
        }
        output.push_str(
            "                _ => throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                    global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,\n                    $\"named source '{extraSource.Name}' is not declared by this mapping\",\n                    detail: extraSource.Name),\n            };\n            parsed.Add(new NamedInput(\n                extraSource.Name,\n                global::Ferrule.Runtime.FerruleJson.Parse(schema, extraSource.Document)));\n        }\n        return parsed;\n",
        );
    }
    output.push_str("    }\n");

    output.push_str(
        "\n    private static JsonExecutionOutputs SerializeJsonOutputs(ExecutionOutputs outputs)\n    {\n        if (outputs.Extras.Count != ExtraTargetJsonSchemas.Length)\n        {\n            throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                global::Ferrule.Runtime.FerruleRuntimeError.JsonBoundary,\n                \"generated mapping returned an unexpected number of named targets\",\n                detail: \"named target count\");\n        }\n        var extras = new global::System.Collections.Generic.List<NamedJsonOutput>(outputs.Extras.Count);\n        for (var index = 0; index < outputs.Extras.Count; index++)\n        {\n            var extra = outputs.Extras[index];\n            extras.Add(new NamedJsonOutput(\n                extra.Name,\n                global::Ferrule.Runtime.FerruleJson.Serialize(\n                    ExtraTargetJsonSchemas[index],\n                    extra.Instance)));\n        }\n        return new JsonExecutionOutputs(\n            global::Ferrule.Runtime.FerruleJson.Serialize(TargetJsonSchema, outputs.Primary),\n            extras);\n    }\n",
    );
    Ok(())
}

fn render_source_context(program: &Program, output: &mut String) {
    output.push_str(
        "\n    private static global::Ferrule.Runtime.ScopeContext CreateContext(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,\n        global::Ferrule.Runtime.FerruleExecutionContext? executionContext)\n    {\n        global::System.ArgumentNullException.ThrowIfNull(source);\n        global::System.ArgumentNullException.ThrowIfNull(extraSources);\n",
    );
    if program.extra_sources.is_empty() {
        output.push_str(
            "        foreach (var extraSource in extraSources)\n        {\n            global::System.ArgumentNullException.ThrowIfNull(extraSource);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Instance);\n            throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,\n                $\"named source '{extraSource.Name}' is not declared by this mapping\",\n                detail: extraSource.Name);\n        }\n",
        );
    } else {
        output.push_str(
            "        var namedSources = new global::System.Collections.Generic.Dictionary<string, global::Ferrule.Runtime.FerruleInstance>(global::System.StringComparer.Ordinal);\n        foreach (var extraSource in extraSources)\n        {\n            global::System.ArgumentNullException.ThrowIfNull(extraSource);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Instance);\n",
        );
        output.push_str("            if (extraSource.Name is not (");
        for (index, source) in program.extra_sources.iter().enumerate() {
            if index != 0 {
                output.push_str(" or ");
            }
            output.push_str(&literal::string(&source.name));
        }
        output.push_str(
            "))\n            {\n                throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                    global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,\n                    $\"named source '{extraSource.Name}' is not declared by this mapping\",\n                    detail: extraSource.Name);\n            }\n            if (!namedSources.TryAdd(extraSource.Name, extraSource.Instance))\n            {\n                throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                    global::Ferrule.Runtime.FerruleRuntimeError.DuplicateNamedSource,\n                    $\"named source '{extraSource.Name}' was supplied more than once\",\n                    detail: extraSource.Name);\n            }\n        }\n",
        );
    }
    for (index, source) in program.extra_sources.iter().enumerate() {
        let name = literal::string(&source.name);
        output.push_str(&format!(
            "        if (!namedSources.TryGetValue({name}, out var namedSource_{index}))\n        {{\n            throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                global::Ferrule.Runtime.FerruleRuntimeError.MissingNamedSource,\n                \"named source \" + {name} + \" is required by this mapping\",\n                detail: {name});\n        }}\n"
        ));
    }
    if program.extra_sources.is_empty() {
        output.push_str(
            "        return global::Ferrule.Runtime.ScopeContext.FromSource(source, executionContext);\n    }\n",
        );
        return;
    }
    output.push_str(
        "        return global::Ferrule.Runtime.ScopeContext.FromSources(\n            source,\n            new global::Ferrule.Runtime.FerruleField[]\n            {\n",
    );
    for (index, source) in program.extra_sources.iter().enumerate() {
        output.push_str("                new(");
        output.push_str(&literal::string(&source.name));
        output.push_str(&format!(", namedSource_{index}),\n"));
    }
    output.push_str("            },\n            executionContext);\n    }\n");
}

fn render_iteration_scope(scope: usize, iteration: &IterationPlan, output: &mut String) {
    render_iteration_candidates(scope, iteration.input(), output);

    let sort = iteration.sort();
    let filter_before_sort = iteration.filter().is_some()
        && sort.is_some_and(|sort| sort.filter_order() == SortFilterOrder::FilterThenSort);
    let has_windows =
        !iteration.windows().is_empty() || iteration.output() == IterationOutput::First;
    let grouping = iteration.grouping();
    let renumber_output =
        grouping.is_some() || iteration.filter().is_some() || sort.is_some() || has_windows;

    if filter_before_sort {
        render_prefilter(scope, iteration.filter(), output);
    }
    if let Some(sort) = sort {
        output.push_str(&format!(
            "        var sort_keys_{scope} = new global::Ferrule.Runtime.FerruleSortKey<global::Ferrule.Runtime.ScopeContext>[]\n        {{\n"
        ));
        for key in sort.keys() {
            output.push_str(&format!(
                "            new(candidate => Node_{}(candidate), {}),\n",
                key.expression,
                if key.descending { "true" } else { "false" }
            ));
        }
        output.push_str(&format!(
            "        }};\n        candidates_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(global::Ferrule.Runtime.FerruleSequences.StableSort(candidates_{scope}, sort_keys_{scope}));\n        for (var sorted_index_{scope} = 0; sorted_index_{scope} < candidates_{scope}.Count; sorted_index_{scope}++)\n        {{\n            candidates_{scope}[sorted_index_{scope}] = candidates_{scope}[sorted_index_{scope}].WithCompactedPosition(sorted_index_{scope} + 1);\n        }}\n"
        ));
    }

    render_windows(scope, iteration, output);
    render_grouping_setup(scope, grouping, output);
    if !filter_before_sort && (has_windows || grouping.is_some()) {
        render_prefilter(scope, iteration.filter(), output);
    }
    if let Some(grouping) = grouping {
        render_grouping(
            scope,
            iteration.input(),
            grouping,
            iteration.post_group_filter(),
            output,
        );
    }
    if has_windows {
        output.push_str(&format!(
            "        candidates_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(global::Ferrule.Runtime.FerruleSequences.ApplyWindows(candidates_{scope}, windows_{scope}));\n"
        ));
    }

    output.push_str(&format!(
        "        var items_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleInstance>();\n        foreach (var item_context_{scope} in candidates_{scope})\n        {{\n"
    ));
    if grouping.is_none()
        && !filter_before_sort
        && !has_windows
        && let Some(filter) = iteration.filter()
    {
        output.push_str(&format!(
            "            var filter_{scope} = Node_{filter}(item_context_{scope});\n            if (!global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(filter_{scope}, {filter}U))\n            {{\n                continue;\n            }}\n"
        ));
    }
    if renumber_output {
        output.push_str(&format!(
            "            var output_context_{scope} = item_context_{scope}.WithCompactedPosition(items_{scope}.Count + 1);\n            items_{scope}.Add(ScopeItem_{scope}(output_context_{scope}));\n"
        ));
    } else {
        output.push_str(&format!(
            "            items_{scope}.Add(ScopeItem_{scope}(item_context_{scope}));\n"
        ));
    }
    output.push_str("        }\n");
    match iteration.output() {
        IterationOutput::Repeated => output.push_str(&format!(
            "        return new global::Ferrule.Runtime.FerruleRepeated(items_{scope});\n"
        )),
        IterationOutput::MappedSequence => output.push_str(&format!(
            "        return new global::Ferrule.Runtime.FerruleMappedSequence(items_{scope});\n"
        )),
        IterationOutput::First => output.push_str(&format!(
            "        return items_{scope}.Count == 0\n            ? new global::Ferrule.Runtime.FerruleGroup(global::System.Array.Empty<global::Ferrule.Runtime.FerruleField>())\n            : items_{scope}[0];\n"
        )),
    }
}

fn render_grouping_setup(scope: usize, grouping: Option<GroupingPlan>, output: &mut String) {
    let Some(GroupingPlan::IntoBlocks { size }) = grouping else {
        return;
    };
    output.push_str(&format!(
        "        var grouping_size_{scope} = global::Ferrule.Runtime.FerruleSequences.PositiveBlockSize({size}U, Node_{size}(context));\n"
    ));
}

fn render_grouping(
    scope: usize,
    input: &IterationSource,
    grouping: GroupingPlan,
    post_group_filter: Option<NodeId>,
    output: &mut String,
) {
    output.push_str(&format!(
        "        candidates_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(context."
    ));
    match grouping {
        GroupingPlan::By { key } => {
            output.push_str(&format!("GroupBy(candidates_{scope}, "));
            render_grouping_path(input, output);
            output.push_str(&format!(
                ", candidate_{scope} => Node_{key}(candidate_{scope})"
            ));
            render_post_group_filter(scope, post_group_filter, output);
            output.push(')');
        }
        GroupingPlan::AdjacentBy { key } => {
            output.push_str(&format!("GroupAdjacentBy(candidates_{scope}, "));
            render_grouping_path(input, output);
            output.push_str(&format!(
                ", candidate_{scope} => Node_{key}(candidate_{scope})"
            ));
            render_post_group_filter(scope, post_group_filter, output);
            output.push(')');
        }
        GroupingPlan::StartingWith { predicate } => {
            output.push_str(&format!("GroupStartingWith(candidates_{scope}, "));
            render_grouping_path(input, output);
            output.push_str(&format!(
                ", candidate_{scope} => global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(Node_{predicate}(candidate_{scope}), {predicate}U)"
            ));
            render_post_group_filter(scope, post_group_filter, output);
            output.push(')');
        }
        GroupingPlan::EndingWith { predicate } => {
            output.push_str(&format!("GroupEndingWith(candidates_{scope}, "));
            render_grouping_path(input, output);
            output.push_str(&format!(
                ", candidate_{scope} => global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(Node_{predicate}(candidate_{scope}), {predicate}U)"
            ));
            render_post_group_filter(scope, post_group_filter, output);
            output.push(')');
        }
        GroupingPlan::IntoBlocks { .. } => {
            output.push_str(&format!("GroupIntoBlocks(candidates_{scope}, "));
            render_grouping_path(input, output);
            output.push_str(&format!(", grouping_size_{scope}"));
            render_post_group_filter(scope, post_group_filter, output);
            output.push(')');
        }
    }
    output.push_str(");\n");
}

fn render_post_group_filter(scope: usize, post_group_filter: Option<NodeId>, output: &mut String) {
    if let Some(predicate) = post_group_filter {
        output.push_str(&format!(
            ", candidate_{scope} => global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(Node_{predicate}(candidate_{scope}), {predicate}U)"
        ));
    }
}

fn render_grouping_path(input: &IterationSource, output: &mut String) {
    match input {
        IterationSource::Source(source) => render_path(source.path(), output),
        IterationSource::Generated(_) => render_path(&[], output),
        IterationSource::InnerJoin(_) | IterationSource::Concatenate(_) => {
            unreachable!("validated portable grouping cannot own an inner join")
        }
    }
}

fn render_iteration_candidates(scope: usize, input: &IterationSource, output: &mut String) {
    match input {
        IterationSource::Source(source) => {
            output.push_str(&format!(
                "        var candidates_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(context.IterateSource("
            ));
            render_path(source.path(), output);
            output.push_str("));\n");
        }
        IterationSource::Generated(sequence) => {
            let identifier = format!("scope_{scope}");
            render_generated_values(&identifier, sequence, output);
            output.push_str(&format!(
                "        var candidates_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(context.IterateGenerated(sequence_values_{identifier}));\n"
            ));
        }
        IterationSource::InnerJoin(join) => render_inner_join(scope, join, output),
        IterationSource::Concatenate(_) => {
            unreachable!("concatenated scopes render before candidate iteration")
        }
    }
}

fn render_concatenated_scope(iteration: &IterationPlan, segments: &[usize], output: &mut String) {
    output.push_str(
        "        var outputs = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleInstance>();\n",
    );
    let variant = match iteration.output() {
        IterationOutput::Repeated => "FerruleRepeated",
        IterationOutput::MappedSequence => "FerruleMappedSequence",
        IterationOutput::First => unreachable!("validated scope sequences cannot use First"),
    };
    for segment in segments {
        output.push_str(&format!(
            "        outputs.AddRange(((global::Ferrule.Runtime.{variant})Scope_{segment}(context)).Items);\n"
        ));
    }
    output.push_str(&format!(
        "        return new global::Ferrule.Runtime.{variant}(outputs);\n"
    ));
}

fn render_inner_join(scope: usize, join: &InnerJoin, output: &mut String) {
    output.push_str(&format!(
        "        var candidates_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>("
    ));
    render_inner_join_call(join, output);
    output.push_str(");\n");
}

fn render_inner_join_call(join: &InnerJoin, output: &mut String) {
    let mut sources = join.plan().sources();
    let Some(first) = sources.next() else {
        unreachable!("validated inner joins contain a first source");
    };
    output.push_str(&format!(
        "context.InnerJoin({}UL,\n            new global::Ferrule.Runtime.FerruleJoinPlan(\n                ",
        join.id().get()
    ));
    render_join_source(first, output);
    output.push_str(
        ",\n                new global::Ferrule.Runtime.FerruleJoinStage[]\n                {\n",
    );
    for (source, conditions) in join.plan().stages() {
        output.push_str("                    new(\n                        ");
        render_join_source(source, output);
        output.push_str(",\n                        new global::Ferrule.Runtime.FerruleJoinKey[]\n                        {\n");
        for condition in conditions.iter() {
            output.push_str("                            new(");
            render_path(condition.left_collection(), output);
            output.push_str(", ");
            render_path(condition.left_path(), output);
            output.push_str(", ");
            render_path(condition.right_path(), output);
            output.push_str("),\n");
        }
        output.push_str("                        }),\n");
    }
    output.push_str("                }))");
}

fn render_join_source(source: &JoinSource, output: &mut String) {
    output.push_str("new global::Ferrule.Runtime.FerruleJoinSource(");
    render_path(source.collection(), output);
    output.push_str(", global::Ferrule.Runtime.FerruleJoinSourceCardinality.");
    output.push_str(match source.cardinality() {
        JoinSourceCardinality::Repeating => "Repeating",
        JoinSourceCardinality::Singleton => "Singleton",
    });
    output.push(')');
}

fn render_generated_values(identifier: &str, sequence: &GeneratedSequence, output: &mut String) {
    output.push_str(&format!(
        "        global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.FerruleValue> sequence_values_{identifier} = global::System.Array.Empty<global::Ferrule.Runtime.FerruleValue>();\n"
    ));
    match sequence {
        GeneratedSequence::Tokenize {
            input, delimiter, ..
        } => output.push_str(&format!(
            "        var sequence_input_{identifier} = Node_{input}(context);\n        if (sequence_input_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n        {{\n            var sequence_parameter_{identifier} = Node_{delimiter}(context);\n            if (sequence_parameter_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n            {{\n                sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.Tokenize(sequence_input_{identifier}, sequence_parameter_{identifier});\n            }}\n        }}\n"
        )),
        GeneratedSequence::TokenizeByLength { input, length, .. } => output.push_str(&format!(
            "        var sequence_input_{identifier} = Node_{input}(context);\n        if (sequence_input_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n        {{\n            var sequence_parameter_{identifier} = Node_{length}(context);\n            if (sequence_parameter_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n            {{\n                sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.TokenizeByLength(sequence_input_{identifier}, sequence_parameter_{identifier});\n            }}\n        }}\n"
        )),
        GeneratedSequence::TokenizeRegex {
            input,
            pattern,
            flags,
            ..
        } => {
            output.push_str(&format!(
                "        var sequence_input_{identifier} = Node_{input}(context);\n        if (sequence_input_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n        {{\n            var sequence_pattern_{identifier} = Node_{pattern}(context);\n            if (sequence_pattern_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n            {{\n"
            ));
            match flags {
                Some(flags) => output.push_str(&format!(
                    "                var sequence_flags_{identifier} = Node_{flags}(context);\n                if (sequence_flags_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n                {{\n                    sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.TokenizeRegex(sequence_input_{identifier}, sequence_pattern_{identifier}, sequence_flags_{identifier});\n                }}\n"
                )),
                None => output.push_str(&format!(
                    "                sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.TokenizeRegex(sequence_input_{identifier}, sequence_pattern_{identifier}, null);\n"
                )),
            }
            output.push_str("            }\n        }\n");
        }
        GeneratedSequence::RecursiveCollect {
            collection,
            children,
            descent_value,
            values,
            value,
            prefix,
            separator,
            ..
        } => {
            output.push_str(&format!(
                "        var sequence_prefix_{identifier} = global::Ferrule.Runtime.FerruleSequences.RecursiveCollectArgumentText(Node_{prefix}(context));\n        var sequence_separator_{identifier} = global::Ferrule.Runtime.FerruleSequences.RecursiveCollectArgumentText(Node_{separator}(context));\n        sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.RecursiveCollect(\n            context,\n            "
            ));
            render_path(collection, output);
            output.push_str(",\n            ");
            render_path(children, output);
            output.push_str(",\n            ");
            render_path(descent_value, output);
            output.push_str(",\n            ");
            render_path(values, output);
            output.push_str(",\n            ");
            render_path(value, output);
            output.push_str(&format!(
                ",\n            sequence_prefix_{identifier},\n            sequence_separator_{identifier});\n"
            ));
        }
        GeneratedSequence::Range {
            from: Some(from),
            to,
            ..
        } => output.push_str(&format!(
            "        var sequence_from_{identifier} = Node_{from}(context);\n        if (sequence_from_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n        {{\n            var sequence_to_{identifier} = Node_{to}(context);\n            if (sequence_to_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n            {{\n                sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.GenerateRange(sequence_from_{identifier}, sequence_to_{identifier});\n            }}\n        }}\n"
        )),
        GeneratedSequence::Range { from: None, to, .. } => output.push_str(&format!(
            "        var sequence_to_{identifier} = Node_{to}(context);\n        if (sequence_to_{identifier}.Kind is not (global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.JsonNull))\n        {{\n            sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.GenerateRange(null, sequence_to_{identifier});\n        }}\n"
        )),
    }
}

fn render_prefilter(scope: usize, filter: Option<u32>, output: &mut String) {
    let Some(filter) = filter else {
        return;
    };
    output.push_str(&format!(
        "        var filtered_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(candidates_{scope}.Count);\n        foreach (var candidate_{scope} in candidates_{scope})\n        {{\n            var filter_{scope} = Node_{filter}(candidate_{scope});\n            if (global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(filter_{scope}, {filter}U))\n            {{\n                filtered_{scope}.Add(candidate_{scope});\n            }}\n        }}\n        candidates_{scope} = filtered_{scope};\n"
    ));
}

fn render_windows(scope: usize, iteration: &IterationPlan, output: &mut String) {
    let has_windows =
        !iteration.windows().is_empty() || iteration.output() == IterationOutput::First;
    if !has_windows {
        return;
    }
    output.push_str(&format!(
        "        var windows_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleSequenceWindow>({});\n",
        iteration.windows().len() + usize::from(iteration.output() == IterationOutput::First)
    ));
    for (index, window) in iteration.windows().iter().enumerate() {
        match *window {
            SequenceWindow::SkipFirst { count } => render_single_window(
                scope,
                index,
                "count",
                count,
                "SkipFirst",
                output,
            ),
            SequenceWindow::First { count } => {
                render_single_window(scope, index, "count", count, "First", output)
            }
            SequenceWindow::From { position } => {
                render_single_window(scope, index, "position", position, "From", output)
            }
            SequenceWindow::Last { count } => {
                render_single_window(scope, index, "count", count, "Last", output)
            }
            SequenceWindow::FromTo { first, last } => output.push_str(&format!(
                "        var window_{scope}_{index}_first = global::Ferrule.Runtime.FerruleSequences.ItemCount({first}U, Node_{first}(context));\n        var window_{scope}_{index}_last = global::Ferrule.Runtime.FerruleSequences.ItemCount({last}U, Node_{last}(context));\n        windows_{scope}.Add(global::Ferrule.Runtime.FerruleSequenceWindow.FromTo(window_{scope}_{index}_first, window_{scope}_{index}_last));\n"
            )),
        }
    }
    if iteration.output() == IterationOutput::First {
        output.push_str(&format!(
            "        windows_{scope}.Add(global::Ferrule.Runtime.FerruleSequenceWindow.First(1));\n"
        ));
    }
}

fn render_single_window(
    scope: usize,
    index: usize,
    label: &str,
    expression: u32,
    kind: &str,
    output: &mut String,
) {
    output.push_str(&format!(
        "        var window_{scope}_{index}_{label} = global::Ferrule.Runtime.FerruleSequences.ItemCount({expression}U, Node_{expression}(context));\n        windows_{scope}.Add(global::Ferrule.Runtime.FerruleSequenceWindow.{kind}(window_{scope}_{index}_{label}));\n"
    ));
}

const fn aggregate_function_name(function: AggregateFunction) -> &'static str {
    match function {
        AggregateFunction::Count => "Count",
        AggregateFunction::Sum => "Sum",
        AggregateFunction::Avg => "Avg",
        AggregateFunction::Min => "Min",
        AggregateFunction::Max => "Max",
        AggregateFunction::Join => "Join",
        AggregateFunction::ItemAt => "ItemAt",
    }
}

const fn runtime_value_name(value: codegen::RuntimeValue) -> &'static str {
    match value {
        codegen::RuntimeValue::MappingFilePath => "MappingFilePath",
        codegen::RuntimeValue::MainMappingFilePath => "MainMappingFilePath",
        codegen::RuntimeValue::CurrentDateTime => "CurrentDateTime",
    }
}

fn add_scope<'a>(scope: &'a TargetScope, scopes: &mut Vec<ScopePlan<'a>>) -> usize {
    let scope_index = scopes.len();
    scopes.push(ScopePlan {
        repeating: false,
        iteration: None,
        construction: &scope.construction,
        evaluations: Vec::new(),
        bindings: Vec::new(),
        children: Vec::new(),
        segments: Vec::new(),
    });

    let mut bindings = Vec::<BindingPlan<'a>>::new();
    let evaluations = scope
        .bindings
        .iter()
        .map(|binding| binding.expression)
        .collect();
    let mut first_binding = BTreeMap::<&str, usize>::new();
    for (binding_index, binding) in scope.bindings.iter().enumerate() {
        if let Some(&existing) = first_binding.get(binding.target_field.as_str()) {
            bindings[existing].values.push(binding_index);
        } else {
            first_binding.insert(binding.target_field.as_str(), bindings.len());
            bindings.push(binding_plan(binding, binding_index));
        }
    }

    let mut children = Vec::with_capacity(scope.children.len());
    for child in &scope.children {
        let child_index = add_scope(child, scopes);
        children.push((child.target_field.as_str(), child_index));
    }
    let segments = scope
        .iteration
        .as_ref()
        .and_then(IterationPlan::concatenated)
        .map(|sequence| {
            sequence
                .iter()
                .map(|segment| add_scope(segment, scopes))
                .collect()
        })
        .unwrap_or_default();
    scopes[scope_index] = ScopePlan {
        repeating: scope.repeating,
        iteration: scope.iteration.as_ref(),
        construction: &scope.construction,
        evaluations,
        bindings,
        children,
        segments,
    };
    scope_index
}

fn binding_plan(binding: &Binding, binding_index: usize) -> BindingPlan<'_> {
    BindingPlan {
        target_field: &binding.target_field,
        target_type: binding.target_type,
        repeating: binding.repeating,
        values: vec![binding_index],
    }
}

fn render_path(path: &[String], output: &mut String) {
    if path.is_empty() {
        output.push_str("global::System.Array.Empty<string>()");
        return;
    }
    output.push_str("new string[] { ");
    for (index, segment) in path.iter().enumerate() {
        if index != 0 {
            output.push_str(", ");
        }
        output.push_str(&literal::string(segment));
    }
    output.push_str(" }");
}

fn target_type(target_type: ScalarType) -> &'static str {
    match target_type {
        ScalarType::String => "TargetScalarType.String",
        ScalarType::Int => "TargetScalarType.Int64",
        ScalarType::Float => "TargetScalarType.Double",
        ScalarType::Bool => "TargetScalarType.Bool",
    }
}

const fn scalar_type_name(value: ScalarType) -> &'static str {
    match value {
        ScalarType::String => "String",
        ScalarType::Int => "Int64",
        ScalarType::Float => "Double",
        ScalarType::Bool => "Bool",
    }
}
