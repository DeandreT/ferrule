//! Owned inner-join scopes and joined aggregates. The scope pipeline keeps the
//! legacy join phases: a filter before sorting and every sort key see original
//! tuple positions, sorting renumbers them, a later filter sees sorted
//! positions, window bounds use the receiving parent, and output sees
//! compacted positions. Raw member positions never change.

use super::*;

const CONTEXT: &str = "global::Ferrule.Runtime.FerruleBasicScopeContext";

fn render_source(source: &::mapping::OwnedJoinSource, output: &mut String) {
    output.push_str("new global::Ferrule.Runtime.FerruleOwnedJoinSource(");
    crate::owned_basic::render_owner(&source.collection().owner, output);
    output.push_str(", ");
    render_path(&source.collection().path, output);
    output.push_str(
        if source.cardinality() == ::mapping::JoinSourceCardinality::Singleton {
            ", singleton: true)"
        } else {
            ")"
        },
    );
}

fn render_stages(plan: &::mapping::OwnedJoinPlan, output: &mut String) {
    let sources = plan
        .sources()
        .map(::mapping::OwnedJoinSource::collection)
        .collect::<Vec<_>>();
    output.push_str("new global::Ferrule.Runtime.FerruleOwnedJoinStage[]\n            {\n");
    for (source, conditions) in plan.stages() {
        output.push_str("                new(");
        render_source(source, output);
        output.push_str(", new global::Ferrule.Runtime.FerruleOwnedJoinKey[] { ");
        for key in conditions.iter() {
            let left = sources
                .iter()
                .position(|collection| *collection == key.left_collection())
                .expect("owned plans join each left source before its key");
            output.push_str(&format!("new({left}, "));
            render_path(key.left_path(), output);
            output.push_str(", ");
            render_path(key.right_path(), output);
            output.push_str("), ");
        }
        output.push_str("}),\n");
    }
    output.push_str("            }");
}

/// Every tuple is joined in the receiving context before any tuple value. A
/// missing expression contributes Null per tuple; the argument runs once in
/// the parent after every tuple value, and the shared reducer keeps the legacy
/// empty/null/type policy. Only this node's join carries its node.
pub(super) fn render_aggregate(
    node: NodeId,
    aggregate: &::mapping::OwnedJoinAggregate,
    output: &mut String,
) {
    let plan = &aggregate.plan;
    let first = plan
        .sources()
        .next()
        .expect("owned plans have a first source");
    output.push_str(&format!(
        "\n    {{\n        global::System.Collections.Generic.IReadOnlyList<{CONTEXT}> tuples_{node};\n        try\n        {{\n            tuples_{node} = context.OwnedInnerJoin({}UL,\n                ",
        aggregate.join.get()
    ));
    render_source(first, output);
    output.push_str(",\n                ");
    render_stages(plan, output);
    output.push_str(&format!(
        ");\n        }}\n        catch (global::Ferrule.Runtime.FerruleRuntimeException error_{node}) {{ error_{node}.WithOwnedSourceNode({node}U); throw; }}\n        var values_{node} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleValue>(tuples_{node}.Count);\n        foreach (var tuple_{node} in tuples_{node})\n        {{\n"
    ));
    match aggregate.expression {
        Some(expression) if aggregate.function == ::mapping::AggregateOp::DecimalSum => output.push_str(&format!(
            "            global::Ferrule.Runtime.FerruleAggregates.CollectValue(global::Ferrule.Runtime.FerruleAggregateOperation.DecimalSum, values_{node}, Node_{expression}(tuple_{node}));\n"
        )),
        Some(expression) => output.push_str(&format!(
            "            values_{node}.Add(Node_{expression}(tuple_{node}));\n"
        )),
        None => output.push_str(&format!(
            "            values_{node}.Add(global::Ferrule.Runtime.FerruleValue.Null);\n"
        )),
    }
    output.push_str("        }\n");
    match aggregate.arg {
        Some(arg) => output.push_str(&format!(
            "        global::Ferrule.Runtime.FerruleValue? argument_{node} = Node_{arg}(context);\n"
        )),
        None => output.push_str(&format!(
            "        global::Ferrule.Runtime.FerruleValue? argument_{node} = null;\n"
        )),
    }
    output.push_str(&format!(
        "        return global::Ferrule.Runtime.FerruleAggregates.Apply(\n            global::Ferrule.Runtime.FerruleAggregateOperation.{}, values_{node}, argument_{node});\n    }}\n",
        aggregate_function_name(aggregate.function.into())
    ));
}

fn render_retain(index: usize, predicate: NodeId, output: &mut String) {
    output.push_str(&format!(
        "        var retained_{index} = new global::System.Collections.Generic.List<{CONTEXT}>(candidates_{index}.Count);\n        foreach (var candidate_{index} in candidates_{index})\n        {{\n            if (global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(Node_{predicate}(candidate_{index}), {predicate}U))\n            {{\n                retained_{index}.Add(candidate_{index});\n            }}\n        }}\n        candidates_{index} = retained_{index};\n"
    ));
}

pub(super) fn render_scope(
    index: usize,
    iteration: &IterationPlan,
    join: &codegen::OwnedInnerJoin,
    output: &mut String,
) {
    let id = join.id().get();
    let plan = join.plan();
    let first = plan
        .sources()
        .next()
        .expect("owned plans have a first source");
    output.push_str(&format!(
        "        var candidates_{index} = new global::System.Collections.Generic.List<{CONTEXT}>(context.OwnedInnerJoin({id}UL,\n            "
    ));
    render_source(first, output);
    output.push_str(",\n            ");
    render_stages(plan, output);
    output.push_str("));\n");
    let sort = iteration.sort();
    let filter_before_sort = iteration.filter().is_some()
        && sort.is_some_and(|sort| sort.filter_order() == SortFilterOrder::FilterThenSort);
    if filter_before_sort && let Some(predicate) = iteration.filter() {
        render_retain(index, predicate, output);
    }
    if let Some(sort) = sort {
        output.push_str(&format!(
            "        var sort_keys_{index} = new global::Ferrule.Runtime.FerruleSortKey<{CONTEXT}>[]\n        {{\n"
        ));
        for key in sort.keys() {
            output.push_str(&format!(
                "            new(candidate => Node_{}(candidate), {}),\n",
                key.expression,
                if key.descending { "true" } else { "false" }
            ));
        }
        output.push_str(&format!(
            "        }};\n        candidates_{index} = new global::System.Collections.Generic.List<{CONTEXT}>(global::Ferrule.Runtime.FerruleSequences.StableSort(candidates_{index}, sort_keys_{index}));\n        for (var sorted_{index} = 0; sorted_{index} < candidates_{index}.Count; sorted_{index}++)\n        {{\n            candidates_{index}[sorted_{index}] = candidates_{index}[sorted_{index}].WithOwnedJoinPosition({id}UL, sorted_{index} + 1);\n        }}\n"
        ));
    }
    let has_windows =
        !iteration.windows().is_empty() || iteration.output() == IterationOutput::First;
    if has_windows {
        render_windows(index, iteration, output);
        if !filter_before_sort && let Some(predicate) = iteration.filter() {
            render_retain(index, predicate, output);
        }
        output.push_str(&format!(
            "        candidates_{index} = new global::System.Collections.Generic.List<{CONTEXT}>(global::Ferrule.Runtime.FerruleSequences.ApplyWindows(candidates_{index}, windows_{index}));\n"
        ));
    }
    output.push_str(&format!(
        "        var items_{index} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleInstance>(candidates_{index}.Count);\n        foreach (var candidate_{index} in candidates_{index})\n        {{\n"
    ));
    if !filter_before_sort
        && !has_windows
        && let Some(predicate) = iteration.filter()
    {
        output.push_str(&format!(
            "            if (!global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(Node_{predicate}(candidate_{index}), {predicate}U))\n            {{\n                continue;\n            }}\n"
        ));
    }
    output.push_str(&format!(
        "            items_{index}.Add(ScopeItem_{index}(candidate_{index}.WithOwnedJoinPosition({id}UL, items_{index}.Count + 1)));\n        }}\n"
    ));
    if iteration.output() == IterationOutput::First {
        output.push_str(&format!(
            "        return items_{index}.Count == 0 ? new global::Ferrule.Runtime.FerruleGroup(global::System.Array.Empty<global::Ferrule.Runtime.FerruleField>()) : items_{index}[0];\n"
        ));
    } else {
        output.push_str(&format!(
            "        return new global::Ferrule.Runtime.FerruleRepeated(items_{index});\n"
        ));
    }
}
