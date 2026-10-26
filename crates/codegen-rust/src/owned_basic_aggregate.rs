//! Explicitly owned collection aggregates over one exact endpoint cohort.

use super::*;

pub(super) fn collect_value(function: mapping::AggregateOp, value: String) -> String {
    if function == mapping::AggregateOp::DecimalSum {
        format!(
            "codegen_runtime::collect_aggregate_value(codegen_runtime::AggregateFunction::DecimalSum, &mut values, {value})?;"
        )
    } else {
        format!("values.push({value});")
    }
}

/// The complete owned cohort is admitted before any item value. Count never
/// reads a value path; the argument runs once in the parent after every item
/// value, and the shared reducer keeps the legacy empty/null/type policy.
pub(crate) fn render_owned_aggregate(
    id: NodeId,
    aggregate: &mapping::OwnedAggregate,
    call: impl Fn(NodeId, &str) -> String,
) -> String {
    let owner = owner_literal(&aggregate.collection.owner);
    let collection = render_string_path(&aggregate.collection.path);
    let mut body = format!(
        "{{\n        let cohort = context.walk_owned_source_cohort({owner}, &[{collection}]).map_err(|error| error.with_owned_source_node({id}))?;\n"
    );
    match &aggregate.value {
        mapping::OwnedAggregateValue::Expression(expression) => body.push_str(&format!(
            "        let mut values = Vec::with_capacity(cohort.len());\n        for candidate in cohort.iter() {{\n            let item_context = candidate.context();\n            {}\n        }}\n",
            collect_value(aggregate.function, format!("{}?", call(*expression, "item_context")))
        )),
        mapping::OwnedAggregateValue::Path(_)
            if aggregate.function == mapping::AggregateOp::Count =>
        {
            body.push_str("        let values = vec![Value::Null; cohort.len()];\n");
        }
        mapping::OwnedAggregateValue::Path(relative) => {
            let mut path = aggregate.collection.path.clone();
            path.extend(relative.iter().cloned());
            let path = render_string_path(&path);
            body.push_str(&format!(
                "        let mut values = Vec::with_capacity(cohort.len());\n        for candidate in cohort.iter() {{\n            let item_context = candidate.context();\n            {}\n        }}\n",
                collect_value(aggregate.function, format!("item_context.owned_field({owner}, &[{path}], None).map_err(|error| error.with_owned_source_node({id}))?"))
            ));
        }
    }
    match aggregate.arg {
        Some(arg) => body.push_str(&format!(
            "        let arg = Some({}?);\n",
            call(arg, "context")
        )),
        None => body.push_str("        let arg = None;\n"),
    }
    body.push_str(&format!(
        "        codegen_runtime::aggregate(codegen_runtime::AggregateFunction::{}, &values, arg)\n    }}",
        aggregate_function_name(aggregate.function.into())
    ));
    body
}
