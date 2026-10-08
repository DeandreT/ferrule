//! Ordered scalar properties on an ordinary JSON target object.

use egui::Ui;
use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{
    DynamicBinding, FormatOptions, Graph, IterationOutput, Scope, ScopeConstruction, ScopeIteration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Profile {
    ScalarObject(ScalarType),
    ReadOnly(&'static str),
}

fn plain_scope(scope: &Scope) -> bool {
    scope.construction == ScopeConstruction::Constructed
        && matches!(scope.iteration, ScopeIteration::None)
        && scope.iteration_output == IterationOutput::Repeated
        && scope.filter.is_none()
        && scope.post_group_filter.is_none()
        && !scope.has_grouping()
        && !scope.has_sort()
        && scope.windows.is_empty()
        && scope.dynamic_children.is_empty()
        && !scope.merge_dynamic_fields
}

fn ordinary_object(schema: &SchemaNode) -> bool {
    !schema.repeating
        && !schema.attribute
        && !schema.text
        && schema.recursive_ref.is_none()
        && schema.alternatives().is_empty()
        && matches!(schema.kind, SchemaKind::Group { .. })
}

/// Resolve the exact static owner before the caller borrows it for editing.
/// Ineligible saved forms retain their model; no schema or scope is repaired.
pub(crate) fn profile(
    root: &Scope,
    target: &SchemaNode,
    path: &[usize],
    options: &FormatOptions,
    mapping_document: bool,
) -> Profile {
    let unavailable = Profile::ReadOnly;
    if !mapping_document {
        return unavailable(
            "Computed properties are available on the primary mapping or a named target.",
        );
    }
    if *options
        != (FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        })
    {
        return unavailable("Computed properties require an ordinary JSON target.");
    }
    let mut scope = root;
    let mut schema = target;
    for next in path.iter().copied().map(Some).chain(std::iter::once(None)) {
        if !ordinary_object(schema) {
            return unavailable(
                "Computed properties require a singular target object without alternatives or recursion.",
            );
        }
        if !plain_scope(scope) {
            return unavailable(
                "This saved scope uses other construction or iteration controls; its computed properties are retained.",
            );
        }
        let Some(index) = next else {
            let Some(dynamic) = schema.dynamic_fields() else {
                return unavailable(
                    "The selected target object does not allow computed properties.",
                );
            };
            if dynamic.repeating
                || dynamic.attribute
                || dynamic.text
                || dynamic.recursive_ref.is_some()
                || dynamic.json_any
            {
                return unavailable(
                    "Scalar property editing requires singular typed values; this saved value schema is retained.",
                );
            }
            return match dynamic.kind {
                SchemaKind::Scalar { ty } => Profile::ScalarObject(ty),
                SchemaKind::ScalarUnion { .. } | SchemaKind::Group { .. } => unavailable(
                    "Scalar property editing requires one String, Int, Float or Bool value type; this saved value schema is retained.",
                ),
            };
        };
        let Some(child) = scope.children.get(index) else {
            return unavailable("The selected target scope is unavailable.");
        };
        let Some(child_schema) = schema.child(&child.target_field) else {
            return unavailable("The selected target scope has no matching object in its schema.");
        };
        scope = child;
        schema = child_schema;
    }
    unavailable("The selected target scope is unavailable.")
}

pub(crate) fn show(ui: &mut Ui, scope: &mut Scope, graph: &Graph, profile: Profile) {
    ui.separator();
    ui.strong("Computed properties");
    // The preceding scope controls can change this owner in the same frame.
    // Recheck it before accepting a property edit based on the earlier profile.
    let profile = if matches!(profile, Profile::ScalarObject(_)) && !plain_scope(scope) {
        Profile::ReadOnly(
            "This scope now uses other construction or iteration controls; its computed properties are retained.",
        )
    } else {
        profile
    };
    let editable = matches!(profile, Profile::ScalarObject(_));
    match profile {
        Profile::ScalarObject(ty) => {
            ui.weak(format!(
                "Names must evaluate to String. Values use the target {} schema.",
                crate::schema_scalar::scalar_type_label(ty)
            ));
        }
        Profile::ReadOnly(reason) => {
            ui.weak(reason);
        }
    }
    let mut remove = None;
    ui.add_enabled_ui(editable, |ui| {
        for (index, binding) in scope.dynamic_bindings.iter_mut().enumerate() {
            ui.push_id(("computed_property", index), |ui| {
                ui.group(|ui| {
                    ui.label(format!("Property {}", index + 1));
                    ui.horizontal(|ui| {
                        ui.label("Name expression");
                        super::node_picker(ui, "computed_property_name", &mut binding.key, graph);
                    });
                    if !graph.nodes.contains_key(&binding.key) {
                        ui.weak(format!(
                            "Saved name expression #{} is missing.",
                            binding.key
                        ));
                    }
                    ui.horizontal(|ui| {
                        ui.label("Value expression");
                        super::node_picker(
                            ui,
                            "computed_property_value",
                            &mut binding.value,
                            graph,
                        );
                    });
                    if !graph.nodes.contains_key(&binding.value) {
                        ui.weak(format!(
                            "Saved value expression #{} is missing.",
                            binding.value
                        ));
                    }
                    if ui
                        .small_button(format!("Remove property {}", index + 1))
                        .clicked()
                    {
                        remove = Some(index);
                    }
                });
            });
        }
    });
    if let Some(index) = remove {
        scope.dynamic_bindings.remove(index);
    }
    let first = super::first_node_id(graph);
    if ui
        .add_enabled(
            editable && first.is_some(),
            egui::Button::new("+ property").small(),
        )
        .on_disabled_hover_text(if editable {
            "Add a graph expression before creating a computed property."
        } else {
            "This target or scope keeps its saved computed properties read-only."
        })
        .clicked()
        && let Some(node) = first
    {
        scope.dynamic_bindings.push(DynamicBinding {
            key: node,
            value: node,
        });
    }
}
