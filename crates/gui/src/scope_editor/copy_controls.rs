use egui::Ui;
use mapping::{Scope, ScopeConstruction};

pub(super) fn show_reason(ui: &mut Ui) {
    ui.strong("Whole source group copy");
    crate::ui_kit::help(
        ui,
        "Copies every field and nested group. Individual bindings, child scopes and grouping are unavailable.",
    );
}

/// Check every existing ancestor before a target route can create new scopes.
/// A missing descendant cannot hide an enclosing whole-group copy.
fn copied_ancestor<'a>(root: &'a Scope, chain: &[String]) -> Option<&'a Scope> {
    let mut scope = root;
    for next in chain.iter().map(Some).chain(std::iter::once(None)) {
        if matches!(
            scope.construction,
            ScopeConstruction::CopyCurrentSource | ScopeConstruction::CopyCurrentRows
        ) {
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
        if matches!(
            scope.construction,
            ScopeConstruction::CopyCurrentSource | ScopeConstruction::CopyCurrentRows
        ) {
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
    if matches!(root.construction, ScopeConstruction::CollectDocuments) {
        return Err(format!(
            "target field `{field}` belongs to a read-only document collection"
        ));
    }
    let Some(owner) = copied_ancestor(root, chain) else {
        return Ok(());
    };
    if matches!(owner.construction, ScopeConstruction::CopyCurrentRows) {
        return Err(format!(
            "target field `{field}` belongs to a read-only CSV row document"
        ));
    }
    let name = if owner.target_field.is_empty() {
        "root"
    } else {
        &owner.target_field
    };
    Err(format!(
        "target field `{field}` belongs to the whole source group copy `{name}`; use a constructed target scope to bind individual fields"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_row_documents_block_bindings_and_child_construction() {
        let scope = Scope {
            construction: ScopeConstruction::CopyCurrentRows,
            ..Scope::default()
        };
        assert!(check_static_binding(&scope, &[], "Value").is_err());
        assert!(check_static_binding(&scope, &["Missing".into()], "Value").is_err());
        assert!(copied_ancestor_at_path(&scope, &[]).is_some());
        assert!(copied_ancestor_at_path(&scope, &[0]).is_some());
    }
}
