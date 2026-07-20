use std::collections::BTreeMap;

use codegen::{
    AggregateFunction, AggregateValue, Binding, Expression, GeneratedSequence, InnerJoin,
    IterationOutput, IterationPlan, IterationSource, JoinSource, JoinSourceCardinality, Program,
    SequenceWindow, SortFilterOrder, TargetConstruction, TargetScope,
};
use ir::ScalarType;

use crate::{EmitError, literal};

mod failures;

struct ScopePlan<'a> {
    repeating: bool,
    iteration: Option<&'a IterationPlan>,
    construction: TargetConstruction,
    evaluations: Vec<u32>,
    bindings: Vec<BindingPlan<'a>>,
    children: Vec<(&'a str, usize)>,
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

    let mut scopes = Vec::new();
    let primary_scope = add_scope(&program.root, &mut scopes);
    let extra_scopes = program
        .extra_targets
        .iter()
        .map(|target| (target.name.as_str(), add_scope(&target.root, &mut scopes)))
        .collect::<Vec<_>>();

    let mut output = String::from(
        "namespace Ferrule.Generated;\n\npublic sealed record NamedInput(\n    string Name,\n    global::Ferrule.Runtime.FerruleInstance Instance);\n\npublic sealed record NamedOutput(\n    string Name,\n    global::Ferrule.Runtime.FerruleInstance Instance);\n\npublic sealed record ExecutionOutputs(\n    global::Ferrule.Runtime.FerruleInstance Primary,\n    global::System.Collections.Generic.IReadOnlyList<NamedOutput> Extras);\n\npublic static class GeneratedMapping\n{\n",
    );
    render_entry_points(program, primary_scope, &extra_scopes, &mut output);
    failures::render(&program.failure_rules, &mut output);

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
            Expression::RuntimeValue { value } => {
                output.push_str(" =>\n        context.ResolveRuntimeValue(");
                output.push_str("global::Ferrule.Runtime.FerruleRuntimeValue.");
                output.push_str(runtime_value_name(*value));
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
                    ");\n        foreach (var find_context_{node} in find_items_{node})\n        {{\n            var find_predicate_{node} = Node_{predicate}(find_context_{node});\n            if (find_predicate_{node}.Kind == global::Ferrule.Runtime.FerruleValueKind.Bool)\n            {{\n                if (find_predicate_{node}.BooleanValue)\n                {{\n                    return Node_{value}(find_context_{node});\n                }}\n                continue;\n            }}\n            if (find_predicate_{node}.Kind is global::Ferrule.Runtime.FerruleValueKind.Null or global::Ferrule.Runtime.FerruleValueKind.XmlNil)\n            {{\n                continue;\n            }}\n            _ = global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(find_predicate_{node}, {predicate}U);\n        }}\n        return global::Ferrule.Runtime.FerruleValue.Null;\n    }}\n"
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
            render_iteration_scope(scope_index, iteration, &mut output);
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
        output.push_str(&format!("        return group_{scope_index};\n"));
        output.push_str("    }\n");
    }
    output.push_str("}\n");
    Ok(output)
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
    let renumber_output = iteration.filter().is_some() || sort.is_some() || has_windows;

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
    if has_windows && !filter_before_sort {
        render_prefilter(scope, iteration.filter(), output);
    }
    if has_windows {
        output.push_str(&format!(
            "        candidates_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(global::Ferrule.Runtime.FerruleSequences.ApplyWindows(candidates_{scope}, windows_{scope}));\n"
        ));
    }

    output.push_str(&format!(
        "        var items_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleInstance>();\n        foreach (var item_context_{scope} in candidates_{scope})\n        {{\n"
    ));
    if !filter_before_sort
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
    }
}

fn render_inner_join(scope: usize, join: &InnerJoin, output: &mut String) {
    let mut sources = join.plan().sources();
    let Some(first) = sources.next() else {
        unreachable!("validated inner joins contain a first source");
    };
    output.push_str(&format!(
        "        var candidates_{scope} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.ScopeContext>(context.InnerJoin({}UL,\n            new global::Ferrule.Runtime.FerruleJoinPlan(\n                ",
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
    output.push_str("                })));\n");
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
            "        var sequence_input_{identifier} = Node_{input}(context);\n        if (sequence_input_{identifier}.Kind != global::Ferrule.Runtime.FerruleValueKind.Null)\n        {{\n            var sequence_parameter_{identifier} = Node_{delimiter}(context);\n            if (sequence_parameter_{identifier}.Kind != global::Ferrule.Runtime.FerruleValueKind.Null)\n            {{\n                sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.Tokenize(sequence_input_{identifier}, sequence_parameter_{identifier});\n            }}\n        }}\n"
        )),
        GeneratedSequence::TokenizeByLength { input, length, .. } => output.push_str(&format!(
            "        var sequence_input_{identifier} = Node_{input}(context);\n        if (sequence_input_{identifier}.Kind != global::Ferrule.Runtime.FerruleValueKind.Null)\n        {{\n            var sequence_parameter_{identifier} = Node_{length}(context);\n            if (sequence_parameter_{identifier}.Kind != global::Ferrule.Runtime.FerruleValueKind.Null)\n            {{\n                sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.TokenizeByLength(sequence_input_{identifier}, sequence_parameter_{identifier});\n            }}\n        }}\n"
        )),
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
            "        var sequence_from_{identifier} = Node_{from}(context);\n        if (sequence_from_{identifier}.Kind != global::Ferrule.Runtime.FerruleValueKind.Null)\n        {{\n            var sequence_to_{identifier} = Node_{to}(context);\n            if (sequence_to_{identifier}.Kind != global::Ferrule.Runtime.FerruleValueKind.Null)\n            {{\n                sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.GenerateRange(sequence_from_{identifier}, sequence_to_{identifier});\n            }}\n        }}\n"
        )),
        GeneratedSequence::Range { from: None, to, .. } => output.push_str(&format!(
            "        var sequence_to_{identifier} = Node_{to}(context);\n        if (sequence_to_{identifier}.Kind != global::Ferrule.Runtime.FerruleValueKind.Null)\n        {{\n            sequence_values_{identifier} = global::Ferrule.Runtime.FerruleSequences.GenerateRange(null, sequence_to_{identifier});\n        }}\n"
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
        construction: TargetConstruction::Group,
        evaluations: Vec::new(),
        bindings: Vec::new(),
        children: Vec::new(),
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
    scopes[scope_index] = ScopePlan {
        repeating: scope.repeating,
        iteration: scope.iteration.as_ref(),
        construction: scope.construction,
        evaluations,
        bindings,
        children,
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
