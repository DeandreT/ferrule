//! Explicitly owned collection aggregates over one exact endpoint cohort.

use super::search::{owned_call, walk_call};
use super::*;

pub(super) fn collect_value(node: NodeId, function: ::mapping::AggregateOp, value: &str) -> String {
    if function == ::mapping::AggregateOp::DecimalSum {
        format!(
            "global::Ferrule.Runtime.FerruleAggregates.CollectValue(global::Ferrule.Runtime.FerruleAggregateOperation.DecimalSum, values_{node}, {value});"
        )
    } else {
        format!("values_{node}.Add({value});")
    }
}

/// The complete owned cohort is admitted before any item value. Count never
/// reads a value path; the argument runs once in the parent after every item
/// value, and the shared reducer keeps the legacy empty/null/type policy.
/// Only this node's walk and item reads carry its node.
pub(crate) fn render_owned_aggregate(
    node: NodeId,
    aggregate: &::mapping::OwnedAggregate,
    output: &mut String,
) {
    output.push_str(&format!(
        "\n    {{\n        global::Ferrule.Runtime.FerruleRetainedSourceCohort cohort_{node};\n        {}\n        var values_{node} = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleValue>(cohort_{node}.Count);\n        try\n        {{\n",
        walk_call(node, &aggregate.collection)
    ));
    match &aggregate.value {
        ::mapping::OwnedAggregateValue::Path(_)
            if aggregate.function == ::mapping::AggregateOp::Count =>
        {
            output.push_str(&format!(
                "            for (var index_{node} = 0; index_{node} < cohort_{node}.Count; index_{node}++)\n            {{\n                values_{node}.Add(global::Ferrule.Runtime.FerruleValue.Null);\n            }}\n"
            ));
        }
        ::mapping::OwnedAggregateValue::Path(relative) => {
            let mut path = aggregate.collection.path.clone();
            path.extend(relative.iter().cloned());
            let mut call = format!("item_context_{node}.ResolveOwnedField(");
            render_owner(&aggregate.collection.owner, &mut call);
            call.push_str(", ");
            mapping::basic_path(&path, &mut call);
            call.push_str(", null)");
            output.push_str(&format!(
                "            foreach (var candidate_{node} in cohort_{node})\n            {{\n                var item_context_{node} = candidate_{node}.Context;\n                global::Ferrule.Runtime.FerruleValue value_{node};\n                {}\n                {}\n            }}\n",
                owned_call(node, &format!("value_{node} = "), &call),
                collect_value(node, aggregate.function, &format!("value_{node}"))
            ));
        }
        ::mapping::OwnedAggregateValue::Expression(expression) => {
            output.push_str(&format!(
                "            foreach (var candidate_{node} in cohort_{node})\n            {{\n                {}\n            }}\n",
                collect_value(node, aggregate.function, &format!("Node_{expression}(candidate_{node}.Context)"))
            ));
        }
    }
    output.push_str(&format!(
        "        }}\n        finally {{ global::System.GC.KeepAlive(cohort_{node}); }}\n"
    ));
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
        mapping::aggregate_function_name(aggregate.function.into())
    ));
}
