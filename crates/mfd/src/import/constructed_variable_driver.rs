use std::collections::BTreeSet;

use ir::{SchemaKind, SchemaNode};
use mapping::{ScopeConstruction, ScopeIteration};

use super::graph::GraphBuilder;
use super::schema::{ComponentFormat, SchemaComponent, schema_node_at};
use super::scope::ScopeBuilder;
use super::target_iteration::literal_constant_value;

/// A constructed driver cannot select a literal or variable over another
/// declaration of the same output. Keep this local to constructed admission.
pub(super) fn uniquely_declared_output(output: u32, builder: &GraphBuilder<'_>) -> bool {
    declared_producer_count(output, builder) == 1
        && builder
            .fn_components
            .iter()
            .filter(|component| component.declared_output_keys.contains(&output))
            .all(|component| {
                component.raw_single_output_valid
                    && (!(component.library == "core"
                        && component.name == "constant"
                        && component.kind == 2)
                        || component.bounded_scalar_pins_valid)
            })
        && builder
            .intermediates
            .iter()
            .filter(|component| component.declares_output(output))
            .all(|component| component.unambiguous_declared_output(output))
        && !builder.sources.iter().any(|component| {
            component.declares_output(output) || component.ports.contains_key(&output)
        })
        && (builder
            .fn_components
            .iter()
            .any(|component| component.declared_output_keys.contains(&output))
            || builder
                .intermediates
                .iter()
                .any(|component| component.declares_output(output)))
}

pub(super) fn declared_producer_count(output: u32, builder: &GraphBuilder<'_>) -> usize {
    builder
        .fn_components
        .iter()
        .filter(|component| component.declared_output_keys.contains(&output))
        .count()
        + builder
            .intermediates
            .iter()
            .filter(|component| component.declares_output(output))
            .count()
        + builder
            .sources
            .iter()
            .filter(|component| {
                component.declares_output(output) || component.ports.contains_key(&output)
            })
            .count()
        + usize::from(builder.udf_by_output.contains_key(&output))
        + usize::from(builder.has_source_node_functions(output))
        + builder
            .json_parsers
            .iter()
            .filter(|recipe| recipe.outputs.contains_key(&output))
            .count()
        + builder
            .flextext_parsers
            .iter()
            .filter(|recipe| recipe.outputs.contains_key(&output))
            .count()
        + builder
            .json_serializers
            .iter()
            .filter(|recipe| recipe.output == output)
            .count()
        + builder
            .xml_serializers
            .iter()
            .filter(|recipe| recipe.output == output)
            .count()
        + builder
            .external_scalar_recipes
            .iter()
            .filter(|recipe| recipe.output == output)
            .count()
        + builder
            .external_xslt_aggregates
            .iter()
            .filter(|recipe| recipe.output == output)
            .count()
        + usize::from(builder.joins.hierarchical_source(output).is_some())
}

/// Connected inputs must keep their selected record ownership after normalization.
/// An unrelated conflicting input does not become an assignment into this record.
pub(super) fn unambiguous_record_inputs(output: u32, builder: &GraphBuilder<'_>) -> bool {
    let Some(variable) = builder
        .intermediates
        .iter()
        .find(|component| component.declares_output(output))
    else {
        return true;
    };
    let Some(path) = variable.ports.get(&output) else {
        return false;
    };
    if !schema_node_at(&variable.schema, path)
        .is_some_and(|node| matches!(node.kind, SchemaKind::Group { .. }))
    {
        return true;
    }
    variable
        .input_keys
        .iter()
        .filter(|key| builder.edge_from.contains_key(key))
        .all(|key| {
            let selected = variable
                .ports
                .get(key)
                .is_some_and(|input| input.starts_with(path) || path.starts_with(input));
            !(selected || variable.ambiguous_input_affects(*key, path))
                || (variable.unambiguous_declared_input(*key)
                    && RawOperandProof::default().prove(builder.edge_from[key], builder, 0, 0))
        })
}

/// A physical-source collision can resolve before source-less admission.
/// Identify only declarations that could otherwise choose a constructed driver.
pub(super) fn declares_constructed_output(output: u32, builder: &GraphBuilder<'_>) -> bool {
    builder.intermediates.iter().any(|component| {
        component.is_variable
            && !component.is_pass_through
            && matches!(
                component.format,
                ComponentFormat::Json | ComponentFormat::Edi
            )
            && component.declares_output(output)
    }) || builder.fn_components.iter().any(|component| {
        component.library == "core"
            && component.name == "constant"
            && component.kind == 2
            && component.declared_output_keys.contains(&output)
    })
}

/// A flat variable constructs one present record without introducing a frame.
/// This is a presence driver; scalar consumers retain their ordinary bindings.
pub(super) fn recognize(
    output: u32,
    target_path: &[String],
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
) -> bool {
    recognize_record(output, target_path, builder, Some(scopes), false)
}

/// Defer classification only: parent frames and target compatibility are checked
/// by `projection` after the enclosing scopes have been installed.
pub(super) fn defer_projection(output: u32, builder: &GraphBuilder<'_>) -> bool {
    builder.intermediates.iter().any(|variable| {
        variable.format == ComponentFormat::Json
            && variable.schema.repeating
            && variable.ports.get(&output).is_some_and(Vec::is_empty)
            && variable.declares_output(output)
    }) && recognize_record(output, &[], builder, None, true)
}

fn recognize_record(
    output: u32,
    target_path: &[String],
    builder: &GraphBuilder<'_>,
    scopes: Option<&ScopeBuilder>,
    singleton_collection: bool,
) -> bool {
    if !uniquely_declared_output(output, builder) || !unambiguous_record_inputs(output, builder) {
        return false;
    }
    let mut owners = builder
        .intermediates
        .iter()
        .filter(|component| component.declares_output(output));
    let Some(variable) = owners.next() else {
        return false;
    };
    if owners.next().is_some()
        || !variable.is_variable
        || variable.is_pass_through
        || !matches!(
            variable.format,
            ComponentFormat::Json | ComponentFormat::Edi
        )
        || variable.compute_when_key.is_some()
    {
        return false;
    }
    let Some(output_path) = variable.ports.get(&output) else {
        return false;
    };
    let Some(record) = schema_node_at(&variable.schema, output_path) else {
        return false;
    };
    let SchemaKind::Group { children, .. } = &record.kind else {
        return false;
    };
    if scopes.is_some_and(|scopes| !plain_parent_chain(target_path, scopes))
        || record.repeating && !singleton_collection
        || record
            .item_count_range
            .is_some_and(|range| !range.contains_count(1))
        || (0..output_path.len()).any(|length| {
            schema_node_at(&variable.schema, &output_path[..length])
                .is_none_or(|node| node.repeating)
        })
        || record.dynamic_fields().is_some()
        || !record.alternatives().is_empty()
        || children
            .iter()
            .any(|child| child.repeating || !child.is_scalar() || !child.alternatives().is_empty())
    {
        return false;
    }
    let mut assigned = BTreeSet::new();
    let mut present = false;
    for (key, path) in &variable.ports {
        if !variable.input_keys.contains(key) {
            continue;
        }
        let Some(feed) = builder.edge_from.get(key).copied() else {
            continue;
        };
        if path.len() < output_path.len() && output_path.starts_with(path) {
            return false;
        }
        let Some(relative) = path.strip_prefix(output_path.as_slice()) else {
            continue;
        };
        if relative.len() != 1
            || !schema_node_at(record, relative).is_some_and(|node| node.is_scalar())
            || !assigned.insert(relative.to_vec())
        {
            return false;
        }
        if direct_scalar_literal(feed, builder) {
            present = true;
        } else if scopes.is_some_and(|scopes| {
            !ScalarAssignmentProof::default().prove(feed, target_path, builder, scopes, 0)
        }) {
            return false;
        }
    }
    present
}

/// Copy-All assigns one flat record while retaining its declared collection type.
pub(super) fn projection(
    output: u32,
    target_path: &[String],
    target: &SchemaComponent,
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
) -> Option<Vec<(String, u32)>> {
    if !recognize_record(output, target_path, builder, Some(scopes), true) {
        return None;
    }
    let variable = builder
        .intermediates
        .iter()
        .find(|component| component.declares_output(output))?;
    let output_path = variable.ports.get(&output)?;
    let record = schema_node_at(&variable.schema, output_path)?;
    let target_record = schema_node_at(&target.schema, target_path)?;
    let SchemaKind::Group { children, .. } = &record.kind else {
        return None;
    };
    if children.len() > 256
        || target_record
            .item_count_range
            .is_some_and(|range| !range.contains_count(1))
        || record.json_contains.is_some()
        || target_record.json_contains.is_some()
    {
        return None;
    }
    // Compare complete item schemas. Names identify the local ports; cardinality
    // is proved above and uniqueness is automatic for the constructed singleton.
    // These are comparison copies: the declared schemas stay unchanged.
    let mut source_item = record.clone();
    let mut target_item = target_record.clone();
    for item in [&mut source_item, &mut target_item] {
        item.name.clear();
        item.repeating = false;
        item.item_count_range = None;
        item.json_unique_items = false;
    }
    if source_item != target_item {
        return None;
    }
    let mut fields = Vec::new();
    for child in children {
        let mut path = output_path.clone();
        path.push(child.name.clone());
        let feed = variable.ports.iter().find_map(|(key, candidate)| {
            (variable.input_keys.contains(key) && *candidate == path)
                .then(|| builder.edge_from.get(key).copied())
                .flatten()
        });
        if let Some(feed) = feed {
            fields.push((child.name.clone(), feed));
        }
    }
    Some(fields)
}

const MAX_SCALAR_ALIAS_HOPS: usize = 64;

pub(super) fn direct_scalar_literal(feed: u32, builder: &GraphBuilder<'_>) -> bool {
    literal_constant_value(feed, builder).is_some()
        && uniquely_declared_output(feed, builder)
        && builder
            .fn_components
            .iter()
            .filter(|owner| owner.declared_output_keys.contains(&feed))
            .count()
            == 1
        && builder.fn_by_output.get(&feed).is_some_and(|index| {
            builder.fn_components[*index].output_pins.as_slice() == [Some(feed)]
                && builder.fn_components[*index].bounded_scalar_pins_valid
        })
        && !builder
            .sources
            .iter()
            .any(|owner| owner.ports.contains_key(&feed))
        && !builder
            .intermediates
            .iter()
            .any(|owner| owner.declares_output(feed))
        && !builder.udf_by_output.contains_key(&feed)
        && !builder.has_source_node_functions(feed)
}

const MAX_CONDITIONAL_DEPTH: usize = 32;
const MAX_SCALAR_OPERANDS: usize = 256;

/// Raw ownership can be checked before enclosing frames are installed. A refusal
/// here removes the whole constructed target before explicit bindings are added.
#[derive(Default)]
struct RawOperandProof {
    active: BTreeSet<u32>,
    visited: usize,
}

impl RawOperandProof {
    fn prove(
        &mut self,
        feed: u32,
        builder: &GraphBuilder<'_>,
        depth: usize,
        aliases: usize,
    ) -> bool {
        // A transparent alias chain is one scalar operand, as in the assignment
        // proof. Its independent hop limit still bounds every recursive walk.
        if aliases == 0 {
            self.visited += 1;
        }
        if self.visited > MAX_SCALAR_OPERANDS || !self.active.insert(feed) {
            return false;
        }
        let admitted = self.prove_operand(feed, builder, depth, aliases);
        self.active.remove(&feed);
        admitted
    }

    fn prove_operand(
        &mut self,
        feed: u32,
        builder: &GraphBuilder<'_>,
        depth: usize,
        aliases: usize,
    ) -> bool {
        if declared_producer_count(feed, builder) != 1 {
            return false;
        }
        if let Some(source) = builder
            .sources
            .iter()
            .find(|source| source.declares_output(feed) || source.ports.contains_key(&feed))
        {
            return source.unambiguous_declared_output(feed);
        }
        if let Some(function) = builder
            .fn_components
            .iter()
            .find(|function| function.declared_output_keys.contains(&feed))
        {
            if !function.raw_single_output_valid {
                return false;
            }
            if function.library != "core"
                || !matches!(
                    (function.kind, function.name.as_str()),
                    (2, "constant") | (4, "if-else")
                )
            {
                // Other scalar function shapes remain subject to ordinary admission.
                return true;
            }
            if !function.bounded_scalar_pins_valid {
                return false;
            }
            if function.kind == 2 {
                return true;
            }
            return depth < MAX_CONDITIONAL_DEPTH
                && function.inputs.iter().all(|input| {
                    input
                        .and_then(|key| builder.edge_from.get(&key))
                        .is_some_and(|operand| self.prove(*operand, builder, depth + 1, 0))
                });
        }
        let Some(alias) = builder
            .intermediates
            .iter()
            .find(|alias| alias.declares_output(feed))
        else {
            return true;
        };
        if aliases == MAX_SCALAR_ALIAS_HOPS || !alias.unambiguous_declared_output(feed) {
            return false;
        }
        let path = &alias.ports[&feed];
        alias
            .input_keys
            .iter()
            .filter(|key| builder.edge_from.contains_key(key))
            .all(|key| {
                let selected = alias
                    .ports
                    .get(key)
                    .is_some_and(|input| input.starts_with(path) || path.starts_with(input));
                !(selected || alias.ambiguous_input_affects(*key, path))
                    || (alias.unambiguous_declared_input(*key)
                        && self.prove(builder.edge_from[key], builder, depth, aliases + 1))
            })
    }
}

#[derive(Default)]
struct ScalarAssignmentProof {
    active: BTreeSet<u32>,
    visited: usize,
}

impl ScalarAssignmentProof {
    fn prove(
        &mut self,
        feed: u32,
        target_path: &[String],
        builder: &GraphBuilder<'_>,
        scopes: &ScopeBuilder,
        depth: usize,
    ) -> bool {
        self.visited += 1;
        if self.visited > MAX_SCALAR_OPERANDS || !self.active.insert(feed) {
            return false;
        }
        let admitted = self.prove_operand(feed, target_path, builder, scopes, depth);
        self.active.remove(&feed);
        admitted
    }

    fn prove_operand(
        &mut self,
        feed: u32,
        target_path: &[String],
        builder: &GraphBuilder<'_>,
        scopes: &ScopeBuilder,
        depth: usize,
    ) -> bool {
        if builder.udf_by_output.contains_key(&feed) {
            return false;
        }
        if parent_scalar(feed, target_path, builder, scopes) {
            return true;
        }
        if builder
            .sources
            .iter()
            .any(|source| source.declares_output(feed) || source.ports.contains_key(&feed))
            || builder
                .intermediates
                .iter()
                .any(|owner| owner.declares_output(feed))
            || builder.udf_by_output.contains_key(&feed)
            || builder.has_source_node_functions(feed)
        {
            return false;
        }
        let mut owners = builder
            .fn_components
            .iter()
            .filter(|owner| owner.declared_output_keys.contains(&feed));
        let Some(function) = owners.next() else {
            return false;
        };
        if owners.next().is_some()
            || !uniquely_declared_output(feed, builder)
            || function.output_pins.as_slice() != [Some(feed)]
        {
            return false;
        }
        if literal_constant_value(feed, builder).is_some() {
            return function.bounded_scalar_pins_valid;
        }
        if function.raw_clock_signature_valid {
            return super::constructed_runtime_clock::assignment(feed, builder);
        }
        if function.raw_count_signature_valid {
            return super::constructed_count::assignment(
                feed,
                function,
                target_path,
                builder,
                scopes,
            );
        }
        if function.raw_parse_date_pins_valid {
            return super::constructed_parse_date::assignment(feed, function, builder, |value| {
                parent_scalar(value, target_path, builder, scopes)
            });
        }
        if depth == MAX_CONDITIONAL_DEPTH
            || !function.bounded_scalar_pins_valid
            || function.library != "core"
            || function.name != "if-else"
            || function.kind != 4
            || function.outputs.as_slice() != [feed]
        {
            return false;
        }
        let [Some(condition), Some(then), Some(otherwise)] = function.inputs.as_slice() else {
            return false;
        };
        [condition, then, otherwise].into_iter().all(|input| {
            builder.edge_from.get(input).is_some_and(|operand| {
                self.prove(*operand, target_path, builder, scopes, depth + 1)
            })
        })
    }
}

fn parent_scalar(
    mut feed: u32,
    target_path: &[String],
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
) -> bool {
    let mut seen = BTreeSet::new();
    let mut expected: Option<&SchemaNode> = None;
    for hop in 0..=MAX_SCALAR_ALIAS_HOPS {
        if !seen.insert(feed) {
            return false;
        }
        if direct_parent_scalar(feed, target_path, builder, scopes) {
            let source = builder.sources[0];
            let node = schema_node_at(&source.schema, &source.ports[&feed]);
            return expected.is_none_or(|expected| {
                node.is_some_and(|node| same_scalar_schema(expected, node))
            });
        }
        if let Some(node) = super::constructed_parser_scalar::schema(feed, builder) {
            return plain_parent_chain(target_path, scopes)
                && expected.is_none_or(|expected| same_scalar_schema(expected, node));
        }
        if hop == MAX_SCALAR_ALIAS_HOPS
            || builder
                .sources
                .iter()
                .any(|source| source.declares_output(feed) || source.ports.contains_key(&feed))
            || builder
                .fn_components
                .iter()
                .any(|function| function.declared_output_keys.contains(&feed))
            || builder.udf_by_output.contains_key(&feed)
            || builder.has_source_node_functions(feed)
        {
            return false;
        }
        let mut owners = builder
            .intermediates
            .iter()
            .filter(|owner| owner.declares_output(feed));
        let Some(alias) = owners.next() else {
            return false;
        };
        if owners.next().is_some()
            || !alias.is_variable
            || alias.is_pass_through
            || alias.compute_when_key.is_some()
            || !matches!(alias.format, ComponentFormat::Json | ComponentFormat::Edi)
            || !alias.unambiguous_declared_output(feed)
        {
            return false;
        }
        let Some(path) = alias.ports.get(&feed) else {
            return false;
        };
        let Some(node) = schema_node_at(&alias.schema, path) else {
            return false;
        };
        if !node.is_scalar()
            || node.repeating
            || !node.alternatives().is_empty()
            || expected.is_some_and(|expected| !same_scalar_schema(expected, node))
            || (0..path.len()).any(|length| {
                schema_node_at(&alias.schema, &path[..length]).is_none_or(|ancestor| {
                    ancestor.repeating
                        || ancestor.dynamic_fields().is_some()
                        || !ancestor.alternatives().is_empty()
                })
            })
        {
            return false;
        }
        let mut inputs = alias.ports.iter().filter(|(key, candidate)| {
            alias.input_keys.contains(key)
                && builder.edge_from.contains_key(key)
                && (path.starts_with(candidate) || candidate.starts_with(path))
        });
        let Some((input, input_path)) = inputs.next() else {
            return false;
        };
        // Only exact scalar pass-throughs qualify. A structural ancestor or
        // descendant override needs a separate complete structural proof.
        if inputs.next().is_some()
            || input_path != path
            || !alias.unambiguous_declared_input(*input)
        {
            return false;
        }
        expected = Some(node);
        feed = builder.edge_from[input];
    }
    false
}

fn same_scalar_schema(left: &SchemaNode, right: &SchemaNode) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.name.clear();
    right.name.clear();
    left == right
}

fn direct_parent_scalar(
    feed: u32,
    target_path: &[String],
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
) -> bool {
    // Inspect physical ports directly: computed/variable aliases do not prove ownership.
    let Some(owner) = builder.sources.first() else {
        return false;
    };
    let Some(path) = owner.ports.get(&feed) else {
        return false;
    };
    if declared_producer_count(feed, builder) != 1
        || !owner.unambiguous_declared_output(feed)
        || !matches!(owner.format, ComponentFormat::Json | ComponentFormat::Xml)
        || owner.options.local_xml_file_set
        || owner.schema.repeating
        || !schema_node_at(&owner.schema, path)
            .is_some_and(|node| node.is_scalar() && !node.repeating)
        || builder
            .sources
            .iter()
            .skip(1)
            .any(|source| source.declares_output(feed) || source.ports.contains_key(&feed))
        || builder
            .fn_components
            .iter()
            .any(|function| function.declared_output_keys.contains(&feed))
        || builder.udf_by_output.contains_key(&feed)
        || builder.has_source_node_functions(feed)
        || builder
            .intermediates
            .iter()
            .any(|variable| variable.declares_output(feed))
        || path.first().is_some_and(|first| {
            builder
                .source_names
                .iter()
                .skip(1)
                .any(|name| name == first)
        })
        || !plain_parent_chain(target_path, scopes)
    {
        return false;
    }
    for length in 1..=path.len() {
        let Some(node) = schema_node_at(&owner.schema, &path[..length]) else {
            return false;
        };
        if node.repeating && !active_parent_frame(&path[..length], target_path, builder, scopes) {
            return false;
        }
    }
    true
}

pub(super) fn active_parent_frame(
    source_path: &[String],
    target_path: &[String],
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
) -> bool {
    if !builder.framed.contains(source_path) {
        return false;
    }
    // Global framed metadata alone is insufficient: a sibling may own the frame.
    // Require a plain source-backed ancestor with this exact physical anchor.
    plain_parent_chain(target_path, scopes)
        && (0..target_path.len()).any(|length| {
            scopes.scope(&target_path[..length]).is_some_and(|scope| {
                matches!(scope.iteration, ScopeIteration::Source(_))
                    && scopes
                        .anchors
                        .get(&target_path[..length])
                        .is_some_and(|anchor| anchor == source_path)
            })
        })
}

pub(super) fn plain_parent_chain(target_path: &[String], scopes: &ScopeBuilder) -> bool {
    for length in 0..target_path.len() {
        let Some(scope) = scopes.scope(&target_path[..length]) else {
            continue;
        };
        if !matches!(
            scope.iteration,
            ScopeIteration::None | ScopeIteration::Source(_)
        ) || !matches!(scope.construction, ScopeConstruction::Constructed)
            || scope.filter.is_some()
            || scope.post_group_filter.is_some()
            || scope.group_by.is_some()
            || scope.group_adjacent_by.is_some()
            || scope.group_starting_with.is_some()
            || scope.group_ending_with.is_some()
            || scope.group_into_blocks.is_some()
            || scope.sort_by.is_some()
            || !scope.sort_then_by.is_empty()
            || !scope.windows.is_empty()
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_all_fixture_selections_retain_independent_complete_collection_schemas() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/constructed-variable-copy-all");
        for (name, component_name, repeated) in [
            ("json-object.mfd", "buffer", false),
            ("json-array.mfd", "buffer", true),
            ("edi-array.mfd", "segment", true),
            ("nested-edi-array.mfd", "segment", true),
        ] {
            let path = directory.join(name);
            let text = std::fs::read_to_string(&path).unwrap();
            let document = roxmltree::Document::parse(&text).unwrap();
            let component = document
                .descendants()
                .find(|node| {
                    node.has_tag_name("component") && node.attribute("name") == Some(component_name)
                })
                .unwrap();
            let mut warnings = Vec::new();
            let resources =
                crate::resource::ResourceResolver::new(&path, Some(directory.parent().unwrap()))
                    .unwrap();
            let selected = if component_name == "buffer" {
                super::super::schema::read_json_component_in_package(
                    &component,
                    &resources,
                    &mut warnings,
                )
            } else {
                super::super::schema::read_edi_component(&component, &resources, &mut warnings)
            }
            .unwrap();
            assert!(warnings.is_empty(), "{name}: {warnings:?}");
            let mut expected = SchemaNode::group(
                if component_name == "buffer" {
                    "Stored"
                } else {
                    "N1"
                },
                vec![
                    SchemaNode::scalar("Text", ir::ScalarType::String),
                    SchemaNode::scalar("Count", ir::ScalarType::Int),
                ],
            );
            expected.repeating = repeated;
            if name == "json-array.mfd" {
                expected.item_count_range = Some(ir::ItemCountRange::new(0, Some(4)).unwrap());
                expected.json_unique_items = true;
            }
            assert_eq!(selected.schema, expected, "{name}");
        }
    }
}
