//! The explicit failure permission does not change ordinary failure rendering.

use super::*;
use codegen::{FailureIteration, FailureRule, FailureSelection};

pub(super) fn needs_owned_rows(program: &Program, rule: &FailureRule) -> bool {
    codegen::is_basic_owned_primary_row_failure(program, rule)
}

pub(super) fn render_rule(
    index: usize,
    rule: &FailureRule,
    output: &mut String,
) -> Result<(), EmitError> {
    let FailureIteration::Source(source) = &rule.iteration else {
        return Err(failure_refusal(index, Reason::FailureRule));
    };
    let number = index + 1;
    output.push_str(&format!("\n    private static void BasicFailureRule_{index}(global::Ferrule.Runtime.FerruleBasicScopeContext context)\n    {{\n        context.VisitPrimaryFailureRows("));
    mapping::basic_path(source.path(), output);
    output.push_str(&format!(", item_context_failure_{index} =>\n        {{\n"));
    let indent = match rule.selection {
        FailureSelection::All => 3,
        FailureSelection::WhenTrue(predicate) | FailureSelection::WhenFalse(predicate) => {
            output.push_str(&format!("            var selection_failure_{index} = Node_{predicate}(item_context_failure_{index});\n            if ("));
            if matches!(rule.selection, FailureSelection::WhenFalse(_)) {
                output.push('!');
            }
            output.push_str(&format!("global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(selection_failure_{index}, {predicate}U))\n            {{\n"));
            4
        }
    };
    let pad = "    ".repeat(indent);
    if let Some(message) = rule.message {
        output.push_str(&format!("{pad}var message_failure_{index} = Node_{message}(item_context_failure_{index});\n{pad}throw global::Ferrule.Runtime.FerruleFailures.MappingFailure({number}, message_failure_{index});\n"));
    } else {
        output.push_str(&format!(
            "{pad}throw global::Ferrule.Runtime.FerruleFailures.MappingFailure({number}, null);\n"
        ));
    }
    if !matches!(rule.selection, FailureSelection::All) {
        output.push_str("            }\n");
    }
    output.push_str("        });\n    }\n");
    Ok(())
}

/// Walk only the selected endpoint; the complete cohort is admitted before the
/// first predicate and stays alive while row contexts are evaluated.
pub(super) fn render_owned_source_rule(
    index: usize,
    rule: &FailureRule,
    source: &::mapping::OwnedSourceRef,
    output: &mut String,
) {
    let number = index + 1;
    output.push_str(&format!("\n    private static void BasicFailureRule_{index}(global::Ferrule.Runtime.FerruleBasicScopeContext context)\n    {{\n        var cohort_failure_{index} = context.WalkOwnedSourceCohort("));
    super::render_owner(&source.owner, output);
    output.push_str(", ");
    mapping::basic_path(&source.path, output);
    output.push_str(&format!(");\n        try\n        {{\n            foreach (var candidate_failure_{index} in cohort_failure_{index})\n            {{\n                var item_context_failure_{index} = candidate_failure_{index}.Context;\n"));
    let indent = match rule.selection {
        FailureSelection::All => 4,
        FailureSelection::WhenTrue(predicate) | FailureSelection::WhenFalse(predicate) => {
            output.push_str(&format!("                var selection_failure_{index} = Node_{predicate}(item_context_failure_{index});\n                if ("));
            if matches!(rule.selection, FailureSelection::WhenFalse(_)) {
                output.push('!');
            }
            output.push_str(&format!("global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(selection_failure_{index}, {predicate}U))\n                {{\n"));
            5
        }
    };
    let pad = "    ".repeat(indent);
    if let Some(message) = rule.message {
        output.push_str(&format!("{pad}var message_failure_{index} = Node_{message}(item_context_failure_{index});\n{pad}throw global::Ferrule.Runtime.FerruleFailures.MappingFailure({number}, message_failure_{index});\n"));
    } else {
        output.push_str(&format!(
            "{pad}throw global::Ferrule.Runtime.FerruleFailures.MappingFailure({number}, null);\n"
        ));
    }
    if !matches!(rule.selection, FailureSelection::All) {
        output.push_str("                }\n");
    }
    output.push_str(&format!("            }}\n        }}\n        finally {{ global::System.GC.KeepAlive(cohort_failure_{index}); }}\n    }}\n"));
}
