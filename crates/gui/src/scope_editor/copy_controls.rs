use egui::Ui;
use mapping::{Scope, ScopeConstruction};

pub(super) fn show_reason(ui: &mut Ui) {
    ui.strong("Whole source group copy");
    ui.weak("Copies every field and nested group. Individual bindings, child scopes and grouping are unavailable.");
}

/// Check every existing ancestor before a target route can create new scopes.
/// A missing descendant cannot hide an enclosing whole-group copy.
fn copied_ancestor<'a>(root: &'a Scope, chain: &[String]) -> Option<&'a Scope> {
    let mut scope = root;
    for next in chain.iter().map(Some).chain(std::iter::once(None)) {
        if matches!(scope.construction, ScopeConstruction::CopyCurrentSource) {
            return Some(scope);
        }
        let next = next?;
        scope = scope
            .children
            .iter()
            .find(|child| child.target_field == *next)?;
    }
    None
}

/// Static child helpers select scope indices, including invalid saved trees
/// whose sibling target names collide. Do not substitute a name-based route.
pub(crate) fn copied_ancestor_at_path<'a>(root: &'a Scope, path: &[usize]) -> Option<&'a Scope> {
    let mut scope = root;
    for next in path.iter().map(Some).chain(std::iter::once(None)) {
        if matches!(scope.construction, ScopeConstruction::CopyCurrentSource) {
            return Some(scope);
        }
        scope = scope.children.get(*next?)?;
    }
    None
}

pub(crate) fn check_static_binding(
    root: &Scope,
    chain: &[String],
    field: &str,
) -> Result<(), String> {
    let Some(owner) = copied_ancestor(root, chain) else {
        return Ok(());
    };
    let name = if owner.target_field.is_empty() {
        "root"
    } else {
        &owner.target_field
    };
    Err(format!(
        "target field `{field}` belongs to the whole source group copy `{name}`; use a constructed target scope to bind individual fields"
    ))
}
