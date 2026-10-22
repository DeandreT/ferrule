//! Owned dynamic driver loading. A loading scope's `Scope_{index}` requests
//! every document of its exact driver collection in the receiving view, then
//! runs the ordinary scope body `ScopeLoaded_{index}` in the view rooting each
//! document.

use super::*;
use codegen::OwnedDynamicLoading;

/// Loading scopes by scope index, numbered exactly as `add_scope`.
pub(super) fn loading_scopes(program: &Program) -> BTreeMap<usize, OwnedDynamicLoading<'_>> {
    let mut scopes = BTreeMap::new();
    let mut next = 0;
    visit(
        program,
        &program.root,
        &mut next,
        &mut Vec::new(),
        &mut scopes,
    );
    for target in &program.extra_targets {
        visit(
            program,
            &target.root,
            &mut next,
            &mut Vec::new(),
            &mut scopes,
        );
    }
    scopes
}

fn visit<'p>(
    program: &'p Program,
    scope: &'p TargetScope,
    next: &mut usize,
    enclosing: &mut Vec<&'p ::mapping::OwnedSourceRef>,
    scopes: &mut BTreeMap<usize, OwnedDynamicLoading<'p>>,
) {
    let index = *next;
    *next += 1;
    let owned = scope
        .iteration
        .as_ref()
        .and_then(IterationPlan::owned_source_iteration)
        .map(|iteration| iteration.source());
    if let Some(source) = owned
        && let Some(loading) =
            codegen::owned_dynamic_loading(program, enclosing.iter().copied(), source)
    {
        scopes.insert(index, loading);
    }
    enclosing.extend(owned);
    for child in &scope.children {
        visit(program, child, next, enclosing, scopes);
    }
    if let TargetConstruction::DynamicGroup { children, .. } = &scope.construction {
        for child in children {
            visit(program, &child.scope, next, enclosing, scopes);
        }
    }
    if let Some(sequence) = scope
        .iteration
        .as_ref()
        .and_then(IterationPlan::concatenated)
    {
        for segment in sequence.iter() {
            visit(program, segment, next, enclosing, scopes);
        }
    }
    if owned.is_some() {
        enclosing.pop();
    }
}

/// Every document is requested before `ScopeLoaded_{index}` walks any of them.
pub(super) fn render_wrapper(index: usize, loading: &OwnedDynamicLoading<'_>, output: &mut String) {
    let path = loading.path;
    output.push_str(&format!("\n    private static global::Ferrule.Runtime.FerruleInstance Scope_{index}(\n        global::Ferrule.Runtime.FerruleBasicScopeContext context)\n    {{\n        var documents_{index} = context.LoadOwnedDynamicDocuments(\n            {},\n            ", literal::string(loading.source)));
    crate::owned_basic::render_owner(&loading.driver.owner, output);
    output.push_str(",\n            ");
    render_path(&loading.driver.path, output);
    output.push_str(&format!(",\n            {path}U,\n            driver => Node_{path}(driver));\n        return ScopeLoaded_{index}(documents_{index}.LoadingContext(context));\n    }}\n"));
}
