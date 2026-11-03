use crate::{EmitError, literal};
use codegen::Program;
use mapping::PrimarySourceShapeKind;

pub(super) fn render(program: &Program, output: &mut String) -> Result<(), EmitError> {
    let Some(plan) = program.validate_execution_contract()? else {
        return Ok(());
    };
    output.push_str("\n    private static readonly global::Ferrule.Runtime.FerrulePrimarySourceContract PrimarySourceContract = new(\n        new global::Ferrule.Runtime.FerrulePrimarySourceShape[]\n        {\n");
    for node in plan.nodes() {
        output.push_str("            global::Ferrule.Runtime.FerrulePrimarySourceShape.");
        match &node.kind {
            PrimarySourceShapeKind::Group { children } => {
                let children = children
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                output.push_str(&format!(
                    "Group({}, {}, new int[] {{ {children} }}),\n",
                    literal::string(&node.name),
                    node.repeating
                ));
            }
            PrimarySourceShapeKind::Scalar { ty, nillable } => {
                let ty = match ty {
                    ir::ScalarType::String => "String",
                    ir::ScalarType::Int => "Int64",
                    ir::ScalarType::Float => "Double",
                    ir::ScalarType::Bool => "Bool",
                };
                output.push_str(&format!(
                    "Scalar({}, global::Ferrule.Runtime.FerruleScalarType.{ty}, {nillable}),\n",
                    literal::string(&node.name)
                ));
            }
        }
    }
    output.push_str("        });\n");
    output.push_str(r#"
    private static void ValidateClosedSourceBindings<T>(
        global::System.Collections.Generic.IEnumerable<T> supplied, int count,
        global::Ferrule.Runtime.FerruleExecutionContext? execution)
    {
        global::Ferrule.Runtime.FerrulePrimarySourceContract.ValidateBindings(count, execution?.RuntimeParameters.Count ?? 0);
        // Probe once without reading Current. Never validate one enumeration
        // and use another: all execution contexts use canonical no-extras data.
        using var items = supplied.GetEnumerator();
        if (items.MoveNext())
            global::Ferrule.Runtime.FerrulePrimarySourceContract.ValidateBindings(1, 0);
    }
"#);
    Ok(())
}

pub(super) fn render_context(program: &Program, output: &mut String) -> bool {
    if program.execution_contract.is_none() {
        return false;
    }
    output.push_str(r#"
    private static global::Ferrule.Runtime.ScopeContext CreateContext(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext? executionContext,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader? dynamicSourceLoader = null)
    {
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(extraSources);
        ValidateClosedSourceBindings(extraSources, extraSources.Count, executionContext);
        PrimarySourceContract.Validate(source, 0, executionContext?.RuntimeParameters.Count ?? 0);
        var context = global::Ferrule.Runtime.ScopeContext.FromSource(source, executionContext);
        return dynamicSourceLoader is null ? context : context.WithDynamicSourceLoader(dynamicSourceLoader);
    }
"#);
    true
}

pub(super) fn bindings(program: &Program, output: &mut String, extras: &str, execution: &str) {
    if program.execution_contract.is_some() {
        output.push_str(&format!(
            "        ValidateClosedSourceBindings(extraSources, {extras}, {execution});\n"
        ));
    }
}

pub(super) fn input(
    program: &Program,
    output: &mut String,
    source: &str,
    extras: &str,
    execution: &str,
) {
    if program.execution_contract.is_some() {
        output.push_str(&format!("        PrimarySourceContract.Validate({source}, {extras}, {execution}?.RuntimeParameters.Count ?? 0);\n"));
    }
}
