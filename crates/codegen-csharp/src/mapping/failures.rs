use codegen::{FailureIteration, FailureRule, FailureSelection};

pub(super) fn render(rules: &[FailureRule], output: &mut String) {
    if rules.is_empty() {
        return;
    }

    output.push_str(
        "\n    private static void EvaluateFailureRules(\n        global::Ferrule.Runtime.ScopeContext context)\n    {\n",
    );
    for index in 0..rules.len() {
        output.push_str(&format!("        FailureRule_{index}(context);\n"));
    }
    output.push_str("    }\n");

    for (index, rule) in rules.iter().enumerate() {
        render_rule(index, rule, output);
    }
}

fn render_rule(index: usize, rule: &FailureRule, output: &mut String) {
    output.push_str(&format!(
        "\n    private static void FailureRule_{index}(\n        global::Ferrule.Runtime.ScopeContext context)\n    {{\n"
    ));
    match &rule.iteration {
        FailureIteration::Source(source) => {
            output.push_str(&format!(
                "        var candidates_failure_{index} = context.IterateSource("
            ));
            super::render_path(source.path(), output);
            output.push_str(");\n");
        }
        FailureIteration::Generated(sequence) => {
            let identifier = format!("failure_{index}");
            super::render_generated_values(&identifier, sequence, output);
            output.push_str(&format!(
                "        var candidates_failure_{index} = context.IterateGenerated(sequence_values_{identifier});\n"
            ));
        }
    }
    output.push_str(&format!(
        "        foreach (var item_context_failure_{index} in candidates_failure_{index})\n        {{\n"
    ));
    match rule.selection {
        FailureSelection::All => render_failure(index, rule.message, output, 3),
        FailureSelection::WhenTrue(predicate) | FailureSelection::WhenFalse(predicate) => {
            output.push_str(&format!(
                "            var selection_failure_{index} = Node_{predicate}(item_context_failure_{index});\n            if ("
            ));
            if matches!(rule.selection, FailureSelection::WhenFalse(_)) {
                output.push('!');
            }
            output.push_str(&format!(
                "global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(selection_failure_{index}, {predicate}U))\n            {{\n"
            ));
            render_failure(index, rule.message, output, 4);
            output.push_str("            }\n");
        }
    }
    output.push_str("        }\n    }\n");
}

fn render_failure(index: usize, message: Option<u32>, output: &mut String, indent: usize) {
    let padding = "    ".repeat(indent);
    let rule = index + 1;
    if let Some(message) = message {
        output.push_str(&format!(
            "{padding}var message_failure_{index} = Node_{message}(item_context_failure_{index});\n{padding}throw global::Ferrule.Runtime.FerruleFailures.MappingFailure({rule}, message_failure_{index});\n"
        ));
    } else {
        output.push_str(&format!(
            "{padding}throw global::Ferrule.Runtime.FerruleFailures.MappingFailure({rule}, null);\n"
        ));
    }
}
