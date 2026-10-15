use ir::Value;
use mapping::{
    AggregateOp, EdiBoundaryKind, IterationOutput, Node, ScopeConstruction, ScopeIteration,
    SortFilterOrder,
};

use super::graph::GraphBuilder;
use super::schema::{ComponentFormat, SchemaComponent, schema_node_at};
use super::scope::{IterationNodes, ScopeBuilder};

/// A global String input without a connected default supplies at most one ordinary JSON object in the
/// inherited source context. Its presence validates the complete document;
/// scalar fields remain independent parser calls with their original types.
pub(super) fn install(
    targets: &[(Vec<String>, u32, u32, bool)],
    target: &SchemaComponent,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) -> Vec<Vec<String>> {
    let mut skipped = Vec::new();
    let mut ordered = targets.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|(path, _, _, _)| path.len());
    for (path, input, feed, copy_all) in ordered {
        let ordinary = target.format == ComponentFormat::Json
            && target.options.json_document
            && !target.is_variable
            && !target.is_pass_through
            && target.compute_when_key.is_none()
            && (!path.is_empty()
                || builder.sources.first().is_some_and(|source| {
                    builder.sources.len() == 1 && primary_root_source(source)
                }))
            && schema_node_at(&target.schema, path)
                .is_some_and(super::json_parser::ordinary_object)
            && !copy_all
            && target.unambiguous_declared_input(*input)
            && !target.ambiguous_input_affects(*input, path)
            && target
                .ports
                .iter()
                .filter(|(key, candidate)| {
                    target.input_keys.contains(key)
                        && *candidate == path
                        && builder.edge_from.contains_key(key)
                })
                .count()
                == 1
            && builder.edge_from.get(input) == Some(feed)
            && (0..=path.len()).all(|depth| {
                scopes.scope(&path[..depth]).is_none_or(|scope| {
                    matches!(
                        scope.iteration,
                        ScopeIteration::None | ScopeIteration::Source(_)
                    ) && scope.construction == ScopeConstruction::Constructed
                        && scope.filter.is_none()
                        && scope.post_group_filter.is_none()
                        && !scope.has_grouping()
                        && scope.sort_by.is_none()
                        && !scope.sort_descending
                        && scope.sort_then_by.is_empty()
                        && scope.sort_filter_order == SortFilterOrder::SortThenFilter
                        && scope.windows.is_empty()
                        && scope.dynamic_bindings.is_empty()
                        && scope.dynamic_children.is_empty()
                        && !scope.merge_dynamic_fields
                        && scope.output_path().is_none()
                })
            })
            && scopes.scope(path).is_none_or(|scope| !scope.iterates());
        let presence = ordinary
            .then(|| builder.json_parser_object_exists_node(*feed))
            .flatten();
        let Some(presence) = presence else {
            builder.warnings.push(format!(
                "JSON parser object presence into `{}` requires one plain object connection and a uniquely declared global String input without a connected default; skipped",
                path.join("/"),
            ));
            skipped.push(path.clone());
            continue;
        };
        let presence = if path.is_empty() {
            let count = builder.alloc(Node::Aggregate {
                function: AggregateOp::Count,
                collection: Vec::new(),
                value: Vec::new(),
                expression: None,
                arg: None,
            });
            let one = builder.alloc(Node::Const {
                value: Value::Int(1),
            });
            let multiple = builder.alloc(Node::Call {
                function: "greater_than".into(),
                args: vec![count, one],
            });
            let message = builder.alloc(Node::Const {
                value: Value::String(
                    "JSON parser root object presence does not support multiple primary rows"
                        .into(),
                ),
            });
            let refusal = builder.alloc(Node::Raise {
                message: Some(message),
            });
            builder.alloc(Node::If {
                condition: multiple,
                then: refusal,
                else_: presence,
            })
        } else {
            presence
        };
        let anchor = scopes.enclosing_anchor(path);
        let output = if path.is_empty() {
            IterationOutput::First
        } else {
            IterationOutput::MappedSequence
        };
        scopes.add_iteration(
            path,
            &anchor,
            IterationNodes {
                filter: Some(presence),
                ..Default::default()
            },
            output,
        );
    }
    skipped
}

fn primary_root_source(source: &SchemaComponent) -> bool {
    (source.format == ComponentFormat::Json
        && source.options.json_document
        && super::json_parser::ordinary_object(&source.schema)
        && !source.options.local_xml_file_set)
        || (source.format == ComponentFormat::Edi
            && source.options.edi_kind == Some(EdiBoundaryKind::X12)
            && source.options.edi_config_reference.is_none()
            && source.schema.name == "Envelope"
            && super::json_parser::ordinary_object(&source.schema)
            && !source.is_variable
            && !source.is_pass_through
            && source.compute_when_key.is_none()
            && !source.options.local_xml_file_set
            && source.dynamic_json.is_none())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::SchemaNode;
    use mapping::{EdiConfigDependency, FormatOptions};

    fn source() -> SchemaComponent {
        SchemaComponent {
            name: "source".into(),
            format: ComponentFormat::Edi,
            schema: SchemaNode::group("Envelope", vec![]),
            input_instance: Some("input.x12".into()),
            output_instance: None,
            options: FormatOptions {
                edi_kind: Some(EdiBoundaryKind::X12),
                lenient_segments: true,
                ..Default::default()
            },
            is_source: true,
            is_default_output: false,
            is_variable: false,
            is_pass_through: false,
            compute_when_key: None,
            ports: Default::default(),
            input_ancestors: Default::default(),
            input_branch_paths: Default::default(),
            ambiguous_branch_inputs: Default::default(),
            has_target_node_controls: false,
            input_keys: Default::default(),
            output_keys: Default::default(),
            json_schema_authoritative: false,
            port_declarations: None,
            db_queries: Default::default(),
            db_xml_columns: Default::default(),
            dynamic_json: None,
        }
    }

    #[test]
    fn static_x12_primary_eligibility_keeps_the_closed_root_and_physical_options() {
        assert!(primary_root_source(&source()));
        let edits: [fn(&mut SchemaComponent); 10] = [
            |source: &mut SchemaComponent| source.options.edi_kind = Some(EdiBoundaryKind::Edifact),
            |source: &mut SchemaComponent| {
                source.options.edi_config_reference =
                    Some(EdiConfigDependency::missing_configuration())
            },
            |source: &mut SchemaComponent| source.schema.name = "Other".into(),
            |source: &mut SchemaComponent| source.schema.repeating = true,
            |source: &mut SchemaComponent| source.schema.json_any = true,
            |source: &mut SchemaComponent| source.schema.json_array_wrapper = true,
            |source: &mut SchemaComponent| source.is_variable = true,
            |source: &mut SchemaComponent| source.is_pass_through = true,
            |source: &mut SchemaComponent| source.compute_when_key = Some(10),
            |source: &mut SchemaComponent| source.options.local_xml_file_set = true,
        ];
        for edit in edits {
            let mut source = source();
            edit(&mut source);
            assert!(!primary_root_source(&source));
        }
        let mut json = source();
        json.format = ComponentFormat::Json;
        json.schema.name = "Source".into();
        json.options = FormatOptions {
            json_document: true,
            ..Default::default()
        };
        assert!(primary_root_source(&json));
    }
}
