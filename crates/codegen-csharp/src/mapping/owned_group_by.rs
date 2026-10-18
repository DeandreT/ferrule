//! Stateless controls for the authenticated, bounded paired By operation.
//! Context overloads are chosen before user literals render; no generated source
//! or identifier replacement is used to compose legacy and paired scopes.

use codegen::{GroupingPlan, IterationOutput, IterationPlan, SequenceWindow, SortFilterOrder};

pub(super) fn render_operation(index: usize, iteration: &IterationPlan, output: &mut String) {
    let operation = match iteration.grouping() {
        Some(GroupingPlan::By { .. }) => "MapGroupBySourceCohort",
        Some(GroupingPlan::AdjacentBy { .. }) => "MapGroupAdjacentBySourceCohort",
        _ => unreachable!("only admitted paired keyed groups have controls"),
    };
    output.push_str(&format!(
        ");\n        var items_{index} = context.{operation}(\n            cohort_{index}, GroupByControls_{index}.Instance, candidate => ScopeItem_{index}(candidate));\n        global::System.GC.KeepAlive(cohort_{index});\n"
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

pub(super) fn render_controls(index: usize, iteration: &IterationPlan, output: &mut String) {
    let key = match iteration.grouping() {
        Some(GroupingPlan::By { key } | GroupingPlan::AdjacentBy { key }) => key,
        _ => unreachable!("only admitted paired By/AdjacentBy scopes have keyed controls"),
    };
    output.push_str(&format!(
        "\n    private sealed class GroupByControls_{index} : global::Ferrule.Runtime.IFerruleBasicGroupByControls\n    {{\n        internal static readonly GroupByControls_{index} Instance = new();\n        private GroupByControls_{index}() {{ }}\n        private static readonly bool[] Descending = new bool[] {{ "
    ));
    if let Some(sort) = iteration.sort() {
        for (position, key) in sort.keys().enumerate() {
            if position != 0 {
                output.push_str(", ");
            }
            output.push_str(if key.descending { "true" } else { "false" });
        }
    }
    let filter_before_sort = iteration
        .sort()
        .is_some_and(|sort| sort.filter_order() == SortFilterOrder::FilterThenSort);
    output.push_str(&format!(
        " }};\n        public global::Ferrule.Runtime.FerruleBasicGroupByMetadata Metadata()\n            => new(Descending, {}, {}, {}, {}, {});\n",
        filter_before_sort,
        iteration.windows().len(),
        iteration.output() == IterationOutput::First,
        iteration.filter().is_some(),
        iteration.post_group_filter().is_some(),
    ));
    output.push_str("        public bool Filter(global::Ferrule.Runtime.FerruleBasicScopeContext candidate)\n            => ");
    render_predicate(iteration.filter(), output);
    output.push_str(";\n        public global::Ferrule.Runtime.FerruleValue SortKey(int key, global::Ferrule.Runtime.FerruleBasicScopeContext candidate)\n");
    if let Some(sort) = iteration.sort() {
        output.push_str("            => key switch\n            {\n");
        for (position, sort_key) in sort.keys().enumerate() {
            output.push_str(&format!(
                "                {position} => Node_{}(candidate),\n",
                sort_key.expression
            ));
        }
        output.push_str("                _ => throw new global::System.InvalidOperationException(\"Invalid generated group sort key.\"),\n            };\n");
    } else {
        output.push_str("            => throw new global::System.InvalidOperationException(\"This generated group has no sort keys.\");\n");
    }
    output.push_str("        public global::Ferrule.Runtime.FerruleSequenceWindow Window(int bound, global::Ferrule.Runtime.FerruleBasicScopeContext parent)\n");
    if iteration.windows().is_empty() {
        output.push_str("            => throw new global::System.InvalidOperationException(\"This generated group has no explicit window bounds.\");\n");
    } else {
        output.push_str("            => bound switch\n            {\n");
        for (position, window) in iteration.windows().iter().copied().enumerate() {
            output.push_str(&format!(
                "                {position} => global::Ferrule.Runtime.FerruleSequenceWindow.{},\n",
                render_window(window)
            ));
        }
        output.push_str("                _ => throw new global::System.InvalidOperationException(\"Invalid generated group window bound.\"),\n            };\n");
    }
    output.push_str(&format!(
        "        public global::Ferrule.Runtime.FerruleValue GroupKey(global::Ferrule.Runtime.FerruleBasicScopeContext candidate)\n            => Node_{key}(candidate);\n        public bool RetainMember(global::Ferrule.Runtime.FerruleBasicScopeContext candidate)\n            => "
    ));
    render_predicate(iteration.post_group_filter(), output);
    output.push_str(";\n    }\n");
}

fn render_predicate(predicate: Option<u32>, output: &mut String) {
    if let Some(predicate) = predicate {
        output.push_str(&format!(
            "global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(Node_{predicate}(candidate), {predicate}U)"
        ));
    } else {
        output.push_str("true");
    }
}

fn render_window(window: SequenceWindow) -> String {
    let value = |node| {
        format!("global::Ferrule.Runtime.FerruleSequences.ItemCount({node}U, Node_{node}(parent))")
    };
    match window {
        SequenceWindow::SkipFirst { count } => format!("SkipFirst({})", value(count)),
        SequenceWindow::First { count } => format!("First({})", value(count)),
        SequenceWindow::From { position } => format!("From({})", value(position)),
        SequenceWindow::Last { count } => format!("Last({})", value(count)),
        SequenceWindow::FromTo { first, last } => {
            format!("FromTo({}, {})", value(first), value(last))
        }
    }
}
