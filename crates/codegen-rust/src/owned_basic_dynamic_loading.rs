//! Owned dynamic driver loading. A loading scope renders as a wrapper that
//! requests every document of its exact driver collection in the receiving
//! view, then runs the ordinary scope body in the view rooting each document.

use super::*;
use codegen::OwnedDynamicLoading;

/// Loading scopes by generated function name, named exactly as `collect_scopes`.
pub(super) fn loading_scopes(program: &Program) -> BTreeMap<String, OwnedDynamicLoading<'_>> {
    let mut scopes = BTreeMap::new();
    visit(
        program,
        &program.root,
        "scope_root".into(),
        &mut Vec::new(),
        &mut scopes,
    );
    for (index, target) in program.extra_targets.iter().enumerate() {
        visit(
            program,
            &target.root,
            format!("scope_extra_{index}"),
            &mut Vec::new(),
            &mut scopes,
        );
    }
    scopes
}

fn visit<'p>(
    program: &'p Program,
    scope: &'p TargetScope,
    name: String,
    enclosing: &mut Vec<&'p mapping::OwnedSourceRef>,
    scopes: &mut BTreeMap<String, OwnedDynamicLoading<'p>>,
) {
    let owned = scope
        .iteration
        .as_ref()
        .and_then(IterationPlan::owned_source_iteration)
        .map(|iteration| iteration.source());
    if let Some(source) = owned
        && let Some(loading) =
            codegen::owned_dynamic_loading(program, enclosing.iter().copied(), source)
    {
        scopes.insert(name.clone(), loading);
    }
    enclosing.extend(owned);
    if let Some(sequence) = scope
        .iteration
        .as_ref()
        .and_then(IterationPlan::concatenated)
    {
        for (index, segment) in sequence.iter().enumerate() {
            visit(
                program,
                segment,
                format!("{name}_segment_{index}"),
                enclosing,
                scopes,
            );
        }
    }
    for (index, child) in scope.children.iter().enumerate() {
        visit(program, child, format!("{name}_{index}"), enclosing, scopes);
    }
    if let TargetConstruction::DynamicGroup { children, .. } = &scope.construction {
        for (index, child) in children.iter().enumerate() {
            visit(
                program,
                &child.scope,
                format!("{name}_dynamic_{index}"),
                enclosing,
                scopes,
            );
        }
    }
    if owned.is_some() {
        enclosing.pop();
    }
}

/// The scope function `name`: every document is requested before the body
/// `{name}_loaded` walks any of them.
pub(super) fn render_loading_wrapper(name: &str, loading: &OwnedDynamicLoading<'_>) -> String {
    let path = loading.path;
    format!(
        "fn {name}(context: &BasicScopeContext<'_>) -> Result<Instance, RuntimeError> {{\n    let documents = context.load_owned_dynamic_documents(\n        {},\n        {},\n        &[{}],\n        {path},\n        |driver_context| expression_{path}(driver_context),\n    )?;\n    let loading = documents.loading_context(context)?;\n    {name}_loaded(&loading)\n}}\n\n",
        rust_string(loading.source),
        owner_literal(&loading.driver.owner),
        render_string_path(&loading.driver.path),
    )
}

/// Checked entries taking an optional host loader, with the ordinary public
/// pair unchanged in shape and a loader-taking pair beside it.
pub(super) fn render_loader_entries(program: &Program) -> String {
    let mut source = String::from("pub use codegen_runtime::DynamicSourceLoader;\n");
    source.push_str(&render_checked_entries_with_loader(program, true));
    source.push_str(
        r#"
/// Execute all outputs using borrowed named inputs and the caller's five limits.
/// A reached loading scope fails without a dynamic source loader.
pub fn execute_with_basic_owned_sources<'name, 'src>(
    source: &'src Instance, supplied: &[BasicNamedSourceInput<'name, 'src>],
    execution: Option<&ExecutionContext<'src>>, limits: OwnedSourceLimits,
) -> Result<ExecutionOutputs, RuntimeError> {
    execute_checked_private_owned_basic(source, supplied, execution, limits, None)
}

/// Validate the selected target before inspecting supplied inputs or policy.
pub fn execute_selected_with_basic_owned_sources<'name, 'src>(
    source: &'src Instance, supplied: &[BasicNamedSourceInput<'name, 'src>],
    execution: Option<&ExecutionContext<'src>>, limits: OwnedSourceLimits,
    selection: TargetSelection<'_>,
) -> Result<SelectedTargetOutput, RuntimeError> {
    execute_selected_checked_private_owned_basic(source, supplied, execution, limits, selection, None)
}

/// Execute all outputs; owned loading scopes request documents from `loader`.
pub fn execute_with_basic_owned_sources_and_dynamic_source_loader<'name, 'src>(
    source: &'src Instance, supplied: &[BasicNamedSourceInput<'name, 'src>],
    execution: Option<&ExecutionContext<'src>>, limits: OwnedSourceLimits,
    loader: &'src dyn DynamicSourceLoader,
) -> Result<ExecutionOutputs, RuntimeError> {
    execute_checked_private_owned_basic(source, supplied, execution, limits, Some(loader))
}

/// Execute the selected target; owned loading scopes request documents from `loader`.
pub fn execute_selected_with_basic_owned_sources_and_dynamic_source_loader<'name, 'src>(
    source: &'src Instance, supplied: &[BasicNamedSourceInput<'name, 'src>],
    execution: Option<&ExecutionContext<'src>>, limits: OwnedSourceLimits,
    selection: TargetSelection<'_>, loader: &'src dyn DynamicSourceLoader,
) -> Result<SelectedTargetOutput, RuntimeError> {
    execute_selected_checked_private_owned_basic(
        source, supplied, execution, limits, selection, Some(loader),
    )
}
"#,
    );
    source
}
