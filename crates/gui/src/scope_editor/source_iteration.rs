//! Source iteration controls, including exact-owner collection selection.

use egui::Ui;
use mapping::{Scope, ScopeConstruction, ScopeIteration};

use crate::path_picker::SourcePathCatalog;

/// Owned iteration is authorable for ordinary constructed scopes; specialized
/// owned constructions and per-item document outputs stay read-only.
pub(super) fn owned_editable(scope: &Scope) -> bool {
    matches!(
        scope.iteration,
        ScopeIteration::OwnedSource {
            output_path: None,
            ..
        }
    ) && scope.construction == ScopeConstruction::Constructed
}

/// Source iteration controls. When an endpoint path is ambiguous, or the
/// project already uses ownership, a collection can be chosen with its exact
/// owner; legacy selections convert only on that explicit choice.
pub(super) fn show(ui: &mut Ui, scope: &mut Scope, source_paths: &SourcePathCatalog, nested: bool) {
    ui.horizontal(|ui| {
        ui.label("source path:");
        let mut has_source = scope.source_selector().is_some();
        if ui.checkbox(&mut has_source, "iterates").changed() {
            scope.set_source(has_source.then(Vec::new));
        }
    });
    let owned_choices = source_paths.ownership().active() && scope.output_path().is_none();
    let owned_picker = |ui: &mut Ui| {
        owned_choices
            .then(|| {
                ui.horizontal(|ui| {
                    ui.label("  owner:");
                    source_paths.show_owned_collection_picker(
                        ui,
                        "scope_owned_source",
                        "Iterate one input's collection",
                        None,
                    )
                })
                .inner
            })
            .flatten()
    };
    let chosen = match scope.source_selector_mut() {
        Some(mapping::SourceSelectorMut::Legacy(source)) => {
            ui.horizontal(|ui| {
                ui.label("  path:");
                source_paths.show_scope_picker(ui, "scope_source_path", source, nested);
            });
            owned_picker(ui)
        }
        Some(mapping::SourceSelectorMut::Owned(source)) => {
            let label = crate::source_ownership::collection_label(&source.owner, &source.path);
            let selected = source.clone();
            ui.horizontal(|ui| {
                ui.label("  owned:");
                source_paths.show_owned_collection_picker(
                    ui,
                    "scope_owned_source",
                    &label,
                    Some(&selected),
                )
            })
            .inner
        }
        // Only a non-iterating scope reaches here without a selector.
        None => owned_picker(ui),
    };
    if let Some(source) = chosen {
        scope.set_source_selector(Some(mapping::SourceSelector::Owned(source)));
    }
}
