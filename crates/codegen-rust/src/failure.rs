use codegen::{FailureIteration, FailureRule, FailureSelection, Program};

use super::{render_generated_values, render_string_path};

pub(super) fn render(program: &Program) -> String {
    let mut output = String::new();
    for (index, rule) in program.failure_rules.iter().enumerate() {
        output.push_str(&render_rule(index, rule));
    }
    output.push_str(
        "fn evaluate_failure_rules(context: &ScopeContext<'_>) -> Result<(), RuntimeError> {\n",
    );
    if program.failure_rules.is_empty() {
        output.push_str("    let _ = context;\n");
    } else {
        for index in 0..program.failure_rules.len() {
            output.push_str(&format!("    evaluate_failure_rule_{index}(context)?;\n"));
        }
    }
    output.push_str("    Ok(())\n}\n\n");
    output
}

fn render_rule(index: usize, rule: &FailureRule) -> String {
    let mut output = format!(
        "fn evaluate_failure_rule_{index}(context: &ScopeContext<'_>) -> Result<(), RuntimeError> {{\n"
    );
    match &rule.iteration {
        FailureIteration::Source(source) => {
            let path = render_string_path(source.path());
            output.push_str(&format!(
                "    let candidates = context.walk_source(&[{path}]);\n"
            ));
        }
        FailureIteration::Generated(sequence) => {
            render_generated_values(sequence, "    ", &mut output);
            output.push_str(
                "    let generated_items = GeneratedItems::new(sequence_values);\n    let candidates = context.generated_items(&generated_items);\n",
            );
        }
    }
    output.push_str("    for item_context in candidates {\n");
    render_selection(rule.selection, &mut output);
    output.push_str("        if !selected {\n            continue;\n        }\n");
    match rule.message {
        Some(message) => output.push_str(&format!(
            "        let message = Some(expression_{message}(&item_context)?);\n"
        )),
        None => output.push_str("        let message = None;\n"),
    }
    output.push_str(&format!(
        "        return Err(codegen_runtime::mapping_failure({}, message));\n",
        index + 1
    ));
    output.push_str("    }\n    Ok(())\n}\n\n");
    output
}

fn render_selection(selection: FailureSelection, output: &mut String) {
    match selection {
        FailureSelection::All => output.push_str("        let selected = true;\n"),
        FailureSelection::WhenTrue(predicate) => output.push_str(&format!(
            "        let predicate = expression_{predicate}(&item_context)?;\n        let selected = require_bool({predicate}, predicate)?;\n"
        )),
        FailureSelection::WhenFalse(predicate) => output.push_str(&format!(
            "        let predicate = expression_{predicate}(&item_context)?;\n        let selected = !require_bool({predicate}, predicate)?;\n"
        )),
    }
}
