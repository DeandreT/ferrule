//! Checked Basic owned mappings: the prepared mapping plus its complete
//! named-input catalogues and checked input accessor, and the public entry
//! points every explicit capability set renders, including the loader-taking
//! entries of a dynamic-loading set.

use super::*;

// Separate checked prototype; the prepared mapping above remains byte-exact.
#[cfg(test)]
pub(crate) fn render_checked_basic_owned_mapping(program: &Program) -> Result<String, EmitError> {
    render_checked_basic_owned_mapping_profile(
        program,
        false,
        OwnedPreset::ControlFree.capabilities(),
    )
}

fn render_checked_basic_owned_mapping_profile(
    program: &Program,
    public_owned_nodes: bool,
    capabilities: OwnedCapabilities,
) -> Result<String, EmitError> {
    let (mut source, plan) = if public_owned_nodes {
        (
            render_basic_owned_mapping_profile(program, true, capabilities)?,
            codegen::legacy_named_input_admission_plan_with_owned_capabilities(
                program,
                capabilities,
            )?,
        )
    } else {
        (
            render_basic_owned_mapping(program)?,
            codegen::legacy_named_input_admission_plan(program)?,
        )
    };
    let (primary, extras) = mapping::render_basic_scopes(program, &mut String::new())?;
    // Reuse the ordinary NamedInput spelling and exact record declaration.
    source.push_str("\npublic sealed record NamedInput(\n    string Name,\n    global::Ferrule.Runtime.FerruleInstance Instance);\n\npublic static partial class GeneratedMapping\n{\n");
    render_checked_catalogue("CheckedAllInputs", &plan.all_outputs, &mut source);
    render_checked_catalogue("CheckedPrimaryInputs", &plan.primary, &mut source);
    for target in &plan.named_targets {
        render_checked_catalogue(
            &format!("CheckedNamedInputs_{}", target.target_index),
            &target.catalogue,
            &mut source,
        );
    }
    // A loading profile threads the optional host loader into its roots.
    let (loader_parameter, loader_attach) = if capabilities.dynamic_loading {
        (
            ",\n        global::Ferrule.Runtime.IFerruleDynamicSourceLoader? loader = null",
            "        if (loader is not null) roots = roots.WithDynamicSourceLoader(loader);\n",
        )
    } else {
        ("", "")
    };
    source.push_str(CHECKED_INPUT_ACCESSOR);
    source.push_str(&format!(
        r#"
    private static ExecutionOutputs ExecuteCheckedOwnedBasic(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> supplied,
        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,
        global::Ferrule.Runtime.FerruleExecutionContext? execution = null{loader_parameter})
    {{
        global::System.ArgumentNullException.ThrowIfNull(source);
        var accessor = new CheckedNamedInputAccessor(supplied);
        ObserveCheckedInputBoundary(true);
        var roots = global::Ferrule.Runtime.FerruleBasicSourceRoots.CreateFromNamedInputs(
            source, CheckedAllInputs, accessor.Supplied, accessor, limits);
{loader_attach}        var context = roots.RootContext(execution);
        var result = (ExecutionOutputs)ExecuteCheckedBasicContext(context, null);
        global::System.GC.KeepAlive(roots);
        return result;
    }}

    private static SelectedTargetOutput ExecuteSelectedCheckedOwnedBasic(
        global::Ferrule.Runtime.FerruleInstance source,
        global::Ferrule.Runtime.FerruleTargetSelection selection,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> supplied,
        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,
        global::Ferrule.Runtime.FerruleExecutionContext? execution = null{loader_parameter})
    {{
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(selection);
        var selected = ResolveOwnedBasicTarget(selection);
        var catalogue = selected switch
        {{
            0 => CheckedPrimaryInputs,
"#
    ));
    for (index, _) in extras.iter().enumerate() {
        source.push_str(&format!(
            "            {} => CheckedNamedInputs_{index},\n",
            index + 1
        ));
    }
    source.push_str(&format!(r#"            _ => throw new global::System.InvalidOperationException("Resolved checked target is invalid."),
        }};
        var accessor = new CheckedNamedInputAccessor(supplied);
        ObserveCheckedInputBoundary(true);
        var roots = global::Ferrule.Runtime.FerruleBasicSourceRoots.CreateFromNamedInputs(
            source, catalogue, accessor.Supplied, accessor, limits);
{loader_attach}        var context = roots.RootContext(execution);
        var result = (SelectedTargetOutput)ExecuteCheckedBasicContext(context, selected);
        global::System.GC.KeepAlive(roots);
        return result;
    }}

    // One checked registry context is reused for ordinary global failures and
    // selected/all output. Specialized failure domains remain typed-closed.
    // Both return shapes are existing reference types; no wrapper is allocated.
    private static object ExecuteCheckedBasicContext(
        global::Ferrule.Runtime.FerruleBasicScopeContext context, int? selected)
    {{
"#));
    if !program.failure_rules.is_empty() {
        source.push_str("        EvaluateBasicFailureRules(context);\n");
    }
    source.push_str(
        r#"        if (selected.HasValue)
        {
            switch (selected.Value)
            {
"#,
    );
    source.push_str(&format!("                case 0: return new SelectedTargetOutput.Primary(Scope_{primary}(context));\n"));
    for (index, (name, scope)) in extras.iter().enumerate() {
        source.push_str(&format!("                case {}: return new SelectedTargetOutput.Named(new NamedOutput({}, Scope_{scope}(context)));\n",index+1,literal::string(name)));
    }
    source.push_str("                default: throw new global::System.InvalidOperationException(\"Resolved checked target is invalid.\");\n            }\n        }\n");
    source.push_str(&format!(
        "        var primary = Scope_{primary}(context);\n"
    ));
    for (index, (_, scope)) in extras.iter().enumerate() {
        source.push_str(&format!(
            "        var extra_{index} = Scope_{scope}(context);\n"
        ));
    }
    source.push_str("        return new ExecutionOutputs(primary, new NamedOutput[]\n        {\n");
    for (index, (name, _)) in extras.iter().enumerate() {
        source.push_str(&format!(
            "            new({}, extra_{index}),\n",
            literal::string(name)
        ));
    }
    source.push_str("        });\n    }\n}\n");
    Ok(source)
}

fn render_checked_catalogue(
    name: &str,
    catalogue: &codegen::NamedInputCatalogue<'_>,
    source: &mut String,
) {
    source.push_str(&format!("    private static readonly global::Ferrule.Runtime.FerruleBasicNamedSourceDeclaration[] {name} = new global::Ferrule.Runtime.FerruleBasicNamedSourceDeclaration[]\n    {{\n"));
    for entry in &catalogue.entries {
        let mode = match entry.availability {
            codegen::NamedInputAvailability::Optional => "Optional",
            codegen::NamedInputAvailability::RequiredLegacy => "RequiredLegacy",
            codegen::NamedInputAvailability::ReservedUnavailable => "ReservedUnavailable",
        };
        source.push_str(&format!("        new({}, global::Ferrule.Runtime.FerruleBasicNamedSourceMode.{mode}), // declaration {}\n",literal::string(entry.name),entry.declaration_index));
    }
    source.push_str("    };\n\n");
}

const CHECKED_INPUT_ACCESSOR: &str = r#"
    // Calls are erased unless the private host supplies this partial observer.
    static partial void ObserveCheckedInputBoundary(bool factory);
    private readonly struct CheckedNamedInputAccessor : global::Ferrule.Runtime.IFerruleNamedSourceInputAccessor<NamedInput>
    {
        private readonly global::System.Collections.Generic.IReadOnlyList<NamedInput> _supplied;
        internal CheckedNamedInputAccessor(global::System.Collections.Generic.IReadOnlyList<NamedInput> supplied) { _supplied = supplied; }
        internal global::System.Collections.Generic.IReadOnlyList<NamedInput> Supplied => _supplied;
        public global::Ferrule.Runtime.FerruleNamedSourceInput Read(NamedInput item)
        {
            ObserveCheckedInputBoundary(false);
            return item is null ? default : new(item.Name, item.Instance);
        }
    }
"#;

pub(crate) fn render_public_basic_owned_mapping(program: &Program) -> Result<String, EmitError> {
    render_public_basic_owned_capability_mapping(program, OwnedPreset::ControlFree.capabilities())
}

/// Render the public Basic owned mapping for one capability set.
pub(crate) fn render_public_basic_owned_capability_mapping(
    program: &Program,
    capabilities: OwnedCapabilities,
) -> Result<String, EmitError> {
    let mut source = render_checked_basic_owned_mapping_profile(program, true, capabilities)?;
    source.push_str(PUBLIC_BASIC_CHECKED_ENTRIES);
    if capabilities.dynamic_loading {
        source.push_str(PUBLIC_BASIC_LOADING_ENTRIES);
    }
    Ok(source)
}

const PUBLIC_BASIC_CHECKED_ENTRIES: &str = r#"
public static partial class GeneratedMapping
{
    public static ExecutionOutputs ExecuteWithBasicOwnedSources(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> supplied,
        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,
        global::Ferrule.Runtime.FerruleExecutionContext? execution)
        => ExecuteCheckedOwnedBasic(source, supplied, limits, execution);

    public static SelectedTargetOutput ExecuteSelectedWithBasicOwnedSources(
        global::Ferrule.Runtime.FerruleInstance source,
        global::Ferrule.Runtime.FerruleTargetSelection selection,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> supplied,
        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,
        global::Ferrule.Runtime.FerruleExecutionContext? execution)
        => ExecuteSelectedCheckedOwnedBasic(source, selection, supplied, limits, execution);
}
"#;

// Owned loading scopes request documents from this host loader. Without one, a
// reached loading scope fails before its driver walk.
const PUBLIC_BASIC_LOADING_ENTRIES: &str = r#"
public static partial class GeneratedMapping
{
    public static ExecutionOutputs ExecuteWithBasicOwnedSourcesAndDynamicSourceLoader(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> supplied,
        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,
        global::Ferrule.Runtime.FerruleExecutionContext? execution,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)
    {
        global::System.ArgumentNullException.ThrowIfNull(loader);
        return ExecuteCheckedOwnedBasic(source, supplied, limits, execution, loader);
    }

    public static SelectedTargetOutput ExecuteSelectedWithBasicOwnedSourcesAndDynamicSourceLoader(
        global::Ferrule.Runtime.FerruleInstance source,
        global::Ferrule.Runtime.FerruleTargetSelection selection,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> supplied,
        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,
        global::Ferrule.Runtime.FerruleExecutionContext? execution,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)
    {
        global::System.ArgumentNullException.ThrowIfNull(loader);
        return ExecuteSelectedCheckedOwnedBasic(source, selection, supplied, limits, execution, loader);
    }
}
"#;
