//! Bounded, endpoint-local aggregate authoring. The common owned-expression
//! admission gate remains in source_ownership; this module only offers paths.
use super::*;
use mapping::{AggregateOp, OwnedAggregate, OwnedAggregateValue};

impl SourcePathCatalog {
    pub(crate) fn aggregate_form_supported(&self, aggregate: &OwnedAggregate) -> bool {
        if aggregate.arg.is_some() {
            return false;
        }
        match (&aggregate.function, &aggregate.value) {
            (AggregateOp::Count, OwnedAggregateValue::Path(path)) => path.is_empty(),
            (AggregateOp::Sum, OwnedAggregateValue::Path(_)) => true,
            _ => false,
        }
    }

    fn aggregate_choices(&self, op: AggregateOp) -> impl Iterator<Item = &OwnedLookupChoice> {
        self.aggregate_choices.iter().filter(move |choice| {
            op == AggregateOp::Count || op == AggregateOp::Sum && !choice.values.is_empty()
        })
    }

    pub(crate) fn aggregate_owner_available(&self, owner: &SourceOwner, op: AggregateOp) -> bool {
        self.aggregate_choices(op)
            .any(|choice| &choice.source.owner == owner)
    }

    pub(crate) fn first_owned_aggregate(&self, op: AggregateOp) -> Option<OwnedAggregate> {
        let choice = self.aggregate_choices(op).next()?;
        Some(OwnedAggregate {
            function: op,
            collection: choice.source.clone(),
            value: OwnedAggregateValue::Path(if op == AggregateOp::Count {
                Vec::new()
            } else {
                choice.values.first()?.path.clone()
            }),
            arg: None,
        })
    }

    /// Inspection never normalizes saved paths. Only an explicit choice resets
    /// a dependent path, and it always uses the newly selected owner's fields.
    pub(crate) fn show_owned_aggregate_editor(&self, ui: &mut Ui, aggregate: &mut OwnedAggregate) {
        ui.set_max_width(crate::theme::editor_size::TYPE_PATH_PICKER);
        let OwnedAggregateValue::Path(value) = &mut aggregate.value else {
            return;
        };
        egui::Grid::new(ui.scope_id().with("owned_aggregate_paths")).show(ui, |ui| {
            ui.label("Operation");
            let previous = aggregate.function;
            egui::ComboBox::from_id_salt(ui.scope_id().with("owned_aggregate_op"))
                .selected_text(operation_label(previous))
                .show_ui(ui, |ui| {
                    for op in [AggregateOp::Count, AggregateOp::Sum] {
                        ui.add_enabled_ui(
                            self.aggregate_owner_available(&aggregate.collection.owner, op),
                            |ui| {
                                ui.selectable_value(
                                    &mut aggregate.function,
                                    op,
                                    operation_label(op),
                                );
                            },
                        );
                    }
                });
            if previous != aggregate.function {
                if aggregate.function == AggregateOp::Count {
                    value.clear();
                } else if let Some(choice) = self
                    .aggregate_choices(aggregate.function)
                    .find(|choice| choice.source == aggregate.collection)
                {
                    reset_value(value, choice);
                } else if let Some(choice) = self
                    .aggregate_choices(aggregate.function)
                    .find(|choice| choice.source.owner == aggregate.collection.owner)
                {
                    aggregate.collection = choice.source.clone();
                    reset_value(value, choice);
                }
            }
            ui.end_row();
            ui.label("Input");
            let mut owner = aggregate.collection.owner.clone();
            egui::ComboBox::from_id_salt(ui.scope_id().with("aggregate_owner"))
                .selected_text(lookup_owner_label(&owner))
                .width(crate::theme::editor_size::PATH_PICKER)
                .show_ui(ui, |ui| {
                    for candidate in self.ownership.lookup_owners() {
                        if self.aggregate_owner_available(&candidate, aggregate.function) {
                            ui.selectable_value(
                                &mut owner,
                                candidate.clone(),
                                lookup_owner_label(&candidate),
                            );
                        }
                    }
                });
            if owner != aggregate.collection.owner {
                let choice = self
                    .aggregate_choices(aggregate.function)
                    .find(|choice| {
                        choice.source.owner == owner
                            && choice.source.path == aggregate.collection.path
                    })
                    .or_else(|| {
                        self.aggregate_choices(aggregate.function)
                            .find(|choice| choice.source.owner == owner)
                    });
                if let Some(choice) = choice {
                    aggregate.collection = choice.source.clone();
                    if aggregate.function == AggregateOp::Sum {
                        reset_value(value, choice);
                    }
                }
            }
            ui.end_row();
            ui.label("Collection");
            let previous = aggregate.collection.path.clone();
            egui::ComboBox::from_id_salt(ui.scope_id().with("aggregate_owned_collection"))
                .selected_text(collection_label(&previous))
                .width(crate::theme::editor_size::PATH_PICKER)
                .show_ui(ui, |ui| {
                    for choice in self.aggregate_choices(aggregate.function) {
                        if choice.source.owner == aggregate.collection.owner {
                            ui.selectable_value(
                                &mut aggregate.collection.path,
                                choice.source.path.clone(),
                                collection_label(&choice.source.path),
                            );
                        }
                    }
                });
            let choice = self
                .aggregate_choices(aggregate.function)
                .find(|choice| choice.source == aggregate.collection);
            if previous != aggregate.collection.path
                && aggregate.function == AggregateOp::Sum
                && let Some(choice) = choice
            {
                reset_value(value, choice);
            }
            ui.end_row();
            if aggregate.function == AggregateOp::Sum {
                ui.label("Value");
                let fields = choice.map_or_else(Vec::new, |choice| choice.values.iter().collect());
                show_path_picker(
                    ui,
                    ui.scope_id().with("aggregate_owned_value"),
                    value,
                    &fields,
                    value_label,
                );
                ui.end_row();
            }
        });
        if aggregate.function == AggregateOp::Sum {
            ui.weak("Ordinary numeric Sum; Decimal values are not supported.");
        }
    }
}

fn operation_label(op: AggregateOp) -> &'static str {
    match op {
        AggregateOp::Count => "Count",
        AggregateOp::Sum => "Sum",
        _ => "Unsupported aggregate",
    }
}

fn reset_value(value: &mut Vec<String>, choice: &OwnedLookupChoice) {
    if !choice.values.iter().any(|field| field.path == *value)
        && let Some(first) = choice.values.first()
    {
        *value = first.path.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::ScalarType;

    #[test]
    fn catalogue_keeps_empty_groups_and_owner_local_scalar_boundaries() {
        let schema = |field| {
            SchemaNode::group(
                "Input",
                vec![
                    SchemaNode::group("Empty", vec![]).repeating(),
                    SchemaNode::group(
                        "Rows",
                        vec![
                            SchemaNode::scalar(field, ScalarType::Int),
                            SchemaNode::group(
                                "Wrapper",
                                vec![SchemaNode::scalar("Amount", ScalarType::Float)],
                            ),
                            SchemaNode::group(
                                "Nested",
                                vec![SchemaNode::scalar("Deep", ScalarType::Int)],
                            )
                            .repeating(),
                        ],
                    )
                    .repeating(),
                ],
            )
        };
        let named = NamedSource {
            name: "Ref".into(),
            path: "named.json".into(),
            schema: schema("Named"),
            options: Default::default(),
            dynamic_path: None,
        };
        let paths = SourcePathCatalog::new(&schema("Primary"), &[named]);
        for owner in [
            SourceOwner::Primary,
            SourceOwner::Named { name: "Ref".into() },
        ] {
            assert!(
                paths
                    .aggregate_choices(AggregateOp::Count)
                    .any(|choice| choice.source.owner == owner && choice.source.path == ["Empty"])
            );
            assert!(
                !paths
                    .aggregate_choices(AggregateOp::Sum)
                    .any(|choice| choice.source.owner == owner && choice.source.path == ["Empty"])
            );
            let row = paths
                .aggregate_choices(AggregateOp::Sum)
                .find(|choice| choice.source.owner == owner && choice.source.path == ["Rows"])
                .unwrap();
            assert_eq!(row.values.len(), 2);
            assert!(
                row.values
                    .iter()
                    .any(|field| field.path == ["Wrapper", "Amount"])
            );
            assert!(!row.values.iter().any(|field| {
                field
                    .path
                    .iter()
                    .any(|name| name == "Nested" || name == "Deep")
            }));
            let foreign = if owner == SourceOwner::Primary {
                "Named"
            } else {
                "Primary"
            };
            assert!(!row.values.iter().any(|field| field.path == [foreign]));
        }
        assert!(
            !paths
                .lookup_choices
                .iter()
                .any(|choice| choice.source.path == ["Empty"])
        );
    }

    #[test]
    fn all_saved_aggregate_forms_participate_in_the_common_admission_gate() {
        let schema = SchemaNode::group(
            "Input",
            vec![SchemaNode::scalar("Value", ScalarType::Int).repeating()],
        );
        let mut project = mapping::Project {
            source: schema,
            target: SchemaNode::group("Output", vec![]),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![],
            failure_rules: vec![],
            user_functions: Default::default(),
            graph: Default::default(),
            root: Default::default(),
        };
        for (op, value, arg, editable) in [
            (
                AggregateOp::Count,
                OwnedAggregateValue::Path(vec![]),
                None,
                true,
            ),
            (
                AggregateOp::Sum,
                OwnedAggregateValue::Path(vec![]),
                None,
                true,
            ),
            (
                AggregateOp::Count,
                OwnedAggregateValue::Path(vec!["Value".into()]),
                None,
                false,
            ),
            (
                AggregateOp::Sum,
                OwnedAggregateValue::Expression(2),
                None,
                false,
            ),
            (
                AggregateOp::Sum,
                OwnedAggregateValue::Path(vec![]),
                Some(2),
                false,
            ),
            (
                AggregateOp::Avg,
                OwnedAggregateValue::Path(vec![]),
                None,
                false,
            ),
            (
                AggregateOp::DecimalSum,
                OwnedAggregateValue::Path(vec![]),
                None,
                false,
            ),
        ] {
            project.graph.nodes.insert(
                1,
                mapping::Node::OwnedAggregate(OwnedAggregate {
                    function: op,
                    collection: OwnedSourceRef {
                        owner: SourceOwner::Primary,
                        path: vec!["Value".into()],
                    },
                    value,
                    arg,
                }),
            );
            project.graph.nodes.insert(
                2,
                mapping::Node::Const {
                    value: ir::Value::Int(1),
                },
            );
            project.graph.nodes.insert(
                3,
                mapping::Node::Call {
                    function: "add".into(),
                    args: vec![1, 2],
                },
            );
            let paths =
                SourcePathCatalog::new(&project.source, &[]).with_project_ownership(&project);
            assert_eq!(paths.lookup_editable(&project.graph, 1), editable, "{op:?}");
            assert_eq!(
                crate::source_ownership::lookup_dependencies(&project.graph, 3),
                BTreeSet::from([1])
            );
            assert!(!crate::source_ownership::lookup_assignment_allowed(
                &project.graph,
                3,
                false,
                |_| true
            ));
            assert_eq!(
                crate::source_ownership::lookup_assignment_allowed(&project.graph, 3, true, |id| {
                    paths.lookup_editable(&project.graph, id)
                }),
                editable
            );
        }
    }
}
