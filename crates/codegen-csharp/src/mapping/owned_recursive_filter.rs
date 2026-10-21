//! Paired Basic recursive filtering of an explicitly owned scope selector.

use codegen::{IterationPlan, TargetConstruction};

/// Renders the owned filter of a paired scope item, or nothing for any other
/// construction. The stored selector is the filter's only owner.
pub(super) fn render_item(
    construction: &TargetConstruction,
    iteration: Option<&IterationPlan>,
    output: &mut String,
) -> bool {
    let (
        TargetConstruction::RecursiveFilter {
            children,
            items,
            predicate,
        },
        Some(source),
    ) = (
        construction,
        iteration.and_then(IterationPlan::owned_source_iteration),
    )
    else {
        return false;
    };
    let source = source.source();
    output.push_str(
        "        return global::Ferrule.Runtime.FerruleRecursiveFilter.ApplyOwned(\n            context,\n            ",
    );
    crate::owned_basic::render_owner(&source.owner, output);
    output.push_str(",\n            ");
    super::render_path(&source.path, output);
    for value in [children, items] {
        output.push_str(",\n            ");
        output.push_str(&crate::literal::string(value));
    }
    output.push_str(&format!(
        ",\n            {predicate}U,\n            Node_{predicate});\n    }}\n"
    ));
    true
}
