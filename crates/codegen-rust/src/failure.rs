use codegen::{FailureIteration, FailureRule, FailureSelection, Program};

use super::{EmitError, render_generated_values, render_string_path};

pub(super) fn render(program: &Program) -> Result<String, EmitError> {
    let mut output = String::new();
    for (index, rule) in program.failure_rules.iter().enumerate() {
        output.push_str(&render_rule(index, rule)?);
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
    Ok(output)
}

fn render_rule(index: usize, rule: &FailureRule) -> Result<String, EmitError> {
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
            render_generated_values(sequence, "    ", &mut output)?;
            output.push_str(
                "    let generated_items = GeneratedItems::new(sequence_values);\n    let candidates = context.generated_items(&generated_items);\n",
            );
        }
    }
    if rule.selection.predicate().is_none() && rule.message.is_none() {
        output.push_str("    for _ in candidates {\n");
    } else {
        output.push_str("    for item_context in candidates {\n");
    }
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
    Ok(output)
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

// The ordinary renderer above stays byte-exact. This private test-only bridge
// retains complete paired legacy cohorts until every reached rule returns.
pub(super) fn render_basic(program: &Program) -> Result<String, EmitError> {
    let mut output = String::new();
    for (index, rule) in program.failure_rules.iter().enumerate() {
        output.push_str(&render_basic_rule(index, rule)?);
    }
    output.push_str("fn evaluate_basic_failure_rules(context: &BasicScopeContext<'_>) -> Result<(), RuntimeError> {\n");
    if program.failure_rules.is_empty() {
        output.push_str("    let _ = context;\n");
    } else {
        for index in 0..program.failure_rules.len() {
            output.push_str(&format!(
                "    evaluate_basic_failure_rule_{index}(context)?;\n"
            ));
        }
    }
    output.push_str("    Ok(())\n}\n\n");
    Ok(output)
}

fn render_basic_rule(index: usize, rule: &FailureRule) -> Result<String, EmitError> {
    let FailureIteration::Source(source) = &rule.iteration else {
        return Err(super::owned_basic::failure_refusal(
            index,
            codegen::OwnedSourceCapabilityReason::FailureRule,
        ));
    };
    let path = render_string_path(source.path());
    let number = index + 1;
    let mut output = format!(
        "fn evaluate_basic_failure_rule_{index}(context: &BasicScopeContext<'_>) -> Result<(), RuntimeError> {{\n"
    );
    output.push_str(&format!(
        "    let reached = context.walk_source_cohort(&[{path}]);\n    #[cfg(test)]\n    match &reached {{\n        Ok(cohort) => {{\n            eprintln!(\"RAW PRIVATE_BASIC_FAILURE_COHORT module={{}} rule={number} count={{}}\", module_path!(), cohort.len());\n            for candidate in cohort.iter() {{\n                eprintln!(\"RAW PRIVATE_BASIC_FAILURE_CANDIDATE module={{}} rule={number} id={{}} raw_ordinal={{:?}} raw_position={{}}\", module_path!(), candidate.source_candidate_id(), candidate.raw_source_ordinal(), candidate.context().legacy_position(&[{path}]));\n            }}\n        }}\n        Err(error) => eprintln!(\"RAW PRIVATE_BASIC_FAILURE_COHORT module={{}} rule={number} error={{error:#?}} cause={{:?}}\", module_path!(), std::error::Error::source(error)),\n    }}\n    let cohort = reached?;\n"
    ));
    if rule.selection.predicate().is_none() && rule.message.is_none() {
        output.push_str("    for _ in cohort.iter() {\n");
    } else {
        output.push_str("    for candidate in cohort.iter() {\n        let item_context = candidate.context();\n");
    }
    match rule.selection {
        FailureSelection::All => output.push_str("        let selected = true;\n"),
        FailureSelection::WhenTrue(predicate) | FailureSelection::WhenFalse(predicate) => {
            output.push_str(&format!("        let reached_predicate = expression_{predicate}(item_context);\n        #[cfg(test)]\n        eprintln!(\"RAW PRIVATE_BASIC_FAILURE_PREDICATE module={{}} rule={number} result={{reached_predicate:#?}} cause={{:?}}\", module_path!(), reached_predicate.as_ref().err().and_then(|error| std::error::Error::source(error)));\n        let predicate = reached_predicate?;\n"));
            let invert = if matches!(rule.selection, FailureSelection::WhenFalse(_)) {
                "!"
            } else {
                ""
            };
            output.push_str(&format!(
                "        let selected = {invert}require_bool({predicate}, predicate)?;\n"
            ));
        }
    }
    output.push_str("        if !selected {\n            continue;\n        }\n");
    match rule.message {
        Some(message) => output.push_str(&format!("        let reached_message = expression_{message}(item_context);\n        #[cfg(test)]\n        eprintln!(\"RAW PRIVATE_BASIC_FAILURE_MESSAGE module={{}} rule={number} result={{reached_message:#?}} cause={{:?}}\", module_path!(), reached_message.as_ref().err().and_then(|error| std::error::Error::source(error)));\n        let message = Some(reached_message?);\n")),
        None => output.push_str("        let message = None;\n"),
    }
    output.push_str(&format!("        let failure = codegen_runtime::mapping_failure({number}, message);\n        #[cfg(test)]\n        eprintln!(\"RAW PRIVATE_BASIC_FAILURE_RESULT module={{}} rule={number} error={{failure:#?}} cause={{:?}}\", module_path!(), std::error::Error::source(&failure));\n        return Err(failure);\n"));
    output.push_str("    }\n    Ok(())\n}\n\n");
    Ok(output)
}
