use crate::{EmitError, rust_string};
use codegen::Program;

pub(crate) fn render(program: &Program, output: &mut String) -> Result<(), EmitError> {
    if program.execution_contract.is_none() {
        return Ok(());
    }
    // Validation derives the shape from the current Program; no imported proof
    // or host-supplied plan reaches the generated library.
    let Some(plan) = program.validate_execution_contract()? else {
        return Ok(());
    };
    output.push_str("\nfn admit_primary_source(context: &ScopeContext<'_>) -> Result<(), RuntimeError> {\n    static PLAN: std::sync::OnceLock<Result<codegen_runtime::ClosedPrimarySourcePlan, codegen_runtime::SourceContractViolation>> = std::sync::OnceLock::new();\n    let plan = PLAN.get_or_init(|| {\n");
    // Flat bottom-up construction avoids the transport descriptor's independent
    // JSON nesting limit. Only guard-relevant shape metadata is emitted, exactly
    // from mapping's freshly compiled plan, then privately compiled again.
    for (id, node) in plan.nodes().iter().enumerate().rev() {
        let shape = match &node.kind {
            mapping::PrimarySourceShapeKind::Group { children } => {
                let children = children
                    .iter()
                    .map(|id| format!("shape_{id}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "codegen_runtime::SchemaNode::group({}, vec![{children}])",
                    rust_string(&node.name)
                )
            }
            mapping::PrimarySourceShapeKind::Scalar { ty, nillable } => {
                format!(
                    "codegen_runtime::SchemaNode::scalar({}, ScalarType::{ty:?}){}",
                    rust_string(&node.name),
                    if *nillable { ".nillable()" } else { "" }
                )
            }
        };
        output.push_str(&format!(
            "        let shape_{id} = {shape}{};\n",
            if node.repeating { ".repeating()" } else { "" }
        ));
    }
    output.push_str("        codegen_runtime::ClosedPrimarySourcePlan::compile(&shape_0)\n    });\n    let plan = plan.as_ref().map_err(|error| RuntimeError::SourceContractViolation(error.clone()))?;\n    context.validate_closed_primary_source(plan)\n}\n");
    Ok(())
}

pub(crate) fn render_admission(program: &Program, output: &mut String) {
    if program.execution_contract.is_some() {
        output.push_str("    admit_primary_source(context)?;\n");
    }
}
