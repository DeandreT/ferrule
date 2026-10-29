use ir::ScalarType;
use mapping::{Node, NodeId};

use super::constructed_parser_scalar::{declared_output_count, exact_pins};
use super::graph::GraphBuilder;
use super::schema::parse_u32;

/// Raw author metadata is retained independently of normalized function pins.
pub(super) struct OptionalStringInput {
    output: u32,
    name: String,
    default_pins: Vec<Option<u32>>,
}

pub(super) fn read(parser: roxmltree::Node<'_, '_>) -> Vec<OptionalStringInput> {
    let Some(children) = parser.parent() else {
        return Vec::new();
    };
    let components: Vec<_> = children
        .children()
        .filter(|node| node.has_tag_name("component"))
        .collect();
    components
        .iter()
        .filter_map(|component| read_input(*component, &components))
        .collect()
}

fn read_input(
    component: roxmltree::Node<'_, '_>,
    siblings: &[roxmltree::Node<'_, '_>],
) -> Option<OptionalStringInput> {
    if component.attribute("library") != Some("core") || component.attribute("kind") != Some("6") {
        return None;
    }
    let data: Vec<_> = component
        .children()
        .filter(|node| node.has_tag_name("data"))
        .collect();
    let [data] = data.as_slice() else {
        return None;
    };
    if data
        .children()
        .filter(roxmltree::Node::is_element)
        .any(|node| {
            !matches!(node.tag_name().name(), "input" | "parameter" | "wsdl")
                || (node.has_tag_name("wsdl")
                    && (node.attributes().len() != 0
                        || node.children().any(|child| child.is_element())))
        })
        || data
            .children()
            .filter(|node| node.has_tag_name("wsdl"))
            .count()
            > 1
    {
        return None;
    }
    let inputs: Vec<_> = data
        .children()
        .filter(|node| node.has_tag_name("input"))
        .collect();
    let [input] = inputs.as_slice() else {
        return None;
    };
    if input
        .attribute("datatype")
        .or_else(|| input.attribute("type"))
        != Some("string")
        || input.attribute("datatype").is_some_and(|ty| ty != "string")
        || input.attribute("type").is_some_and(|ty| ty != "string")
        || input.children().any(|node| node.is_element())
    {
        return None;
    }
    let parameters: Vec<_> = data
        .children()
        .filter(|node| node.has_tag_name("parameter"))
        .collect();
    let [parameter] = parameters.as_slice() else {
        return None;
    };
    let name = parameter
        .attribute("name")
        .filter(|name| !name.is_empty())?;
    if parameter.attribute("usageKind") != Some("input")
        || parameter.attribute("optional") != Some("1")
        || parameter.children().any(|node| node.is_element())
        || siblings
            .iter()
            .filter(|sibling| {
                sibling.attribute("library") == Some("core")
                    && sibling.attribute("kind") == Some("6")
                    && sibling.descendants().any(|node| {
                        node.has_tag_name("parameter")
                            && node.attribute("usageKind") == Some("input")
                            && node.attribute("name") == Some(name)
                    })
            })
            .count()
            != 1
    {
        return None;
    }
    let output = exact_pins(component, "targets", 1)?[0];
    if declared_output_count(siblings.iter().copied(), output) != 1 {
        return None;
    }
    let default_pins = default_pins(component)?;
    if default_pins.contains(&Some(output))
        || default_pins
            .iter()
            .flatten()
            .any(|key| raw_default_is_connected(component, *key))
    {
        return None;
    }
    Some(OptionalStringInput {
        output,
        name: name.into(),
        default_pins,
    })
}

/// A default placeholder is either absent, empty, or one exact position-zero
/// datapoint. A missing key is author-declared absence, not a normalized hole.
pub(super) fn default_pins(component: roxmltree::Node<'_, '_>) -> Option<Vec<Option<u32>>> {
    let mut sources = component
        .children()
        .filter(|node| node.has_tag_name("sources"));
    let Some(source) = sources.next() else {
        return Some(Vec::new());
    };
    if sources.next().is_some() {
        return None;
    }
    let points: Vec<_> = source
        .children()
        .filter(roxmltree::Node::is_element)
        .collect();
    match points.as_slice() {
        [] => Some(Vec::new()),
        [point]
            if point.has_tag_name("datapoint")
                && matches!(point.attribute("pos"), None | Some("0"))
                && !point.children().any(|node| node.is_element()) =>
        {
            let key = match point.attribute("key") {
                Some(raw) => Some(parse_u32(Some(raw))?),
                None => None,
            };
            Some(vec![key])
        }
        _ => None,
    }
}

/// Census saved connections before conflict normalization can remove a feed.
pub(super) fn raw_default_is_connected(component: roxmltree::Node<'_, '_>, key: u32) -> bool {
    let Some(structure) = component.parent().and_then(|children| children.parent()) else {
        return true;
    };
    let local = structure
        .children()
        .filter(|node| node.has_tag_name("graph") || node.has_tag_name("connections"))
        .any(|container| {
            container.descendants().any(|node| {
                node.has_tag_name("edge")
                    && (parse_u32(node.attribute("to")) == Some(key)
                        || parse_u32(node.attribute("vertexkey")) == Some(key))
            })
        });
    local
        || structure.parent().is_some_and(|wrapper| {
            wrapper
                .children()
                .filter(|node| node.has_tag_name("connections"))
                .any(|container| {
                    container.children().any(|node| {
                        node.has_tag_name("edge") && parse_u32(node.attribute("to")) == Some(key)
                    })
                })
        })
}

pub(super) fn input_is_optional_string(
    recipe: &super::json_parser::Recipe,
    builder: &GraphBuilder<'_>,
) -> bool {
    producer(recipe, builder).is_some()
}

fn producer(
    recipe: &super::json_parser::Recipe,
    builder: &GraphBuilder<'_>,
) -> Option<(u32, usize)> {
    let feed = *builder.edge_from.get(&recipe.input)?;
    let witness = recipe
        .optional_string_inputs
        .iter()
        .find(|input| input.output == feed)?;
    if super::constructed_parser_scalar::output_is_nonfunction_shadowed(feed, builder)
        || builder.json_parsers.iter().any(|parser| {
            parser.outputs.contains_key(&feed) || parser.object_outputs.contains_key(&feed)
        })
    {
        return None;
    }
    let mut producers = builder
        .fn_components
        .iter()
        .enumerate()
        .filter(|(_, function)| function.declared_output_keys.contains(&feed));
    let (index, parameter) = producers.next()?;
    if producers.next().is_some()
        || parameter.library != "core"
        || parameter.kind != 6
        || !parameter.raw_single_output_valid
        || parameter.input_type != Some(ScalarType::String)
        || parameter.outputs.as_slice() != [feed]
        || parameter.output_pins.as_slice() != [Some(feed)]
        || parameter.inputs != witness.default_pins
        || parameter
            .inputs
            .iter()
            .flatten()
            .any(|key| builder.edge_from.contains_key(key))
        || !parameter
            .input_parameter_name
            .as_ref()
            .is_some_and(|parameter| parameter.optional && parameter.name == witness.name)
        || builder
            .fn_components
            .iter()
            .filter(|function| {
                function
                    .input_parameter_name
                    .as_ref()
                    .is_some_and(|parameter| parameter.name == witness.name)
            })
            .count()
            != 1
    {
        return None;
    }
    Some((feed, index))
}

impl GraphBuilder<'_> {
    pub(super) fn parser_input_node(&mut self, input: u32) -> Option<NodeId> {
        let feed = *self.edge_from.get(&input)?;
        let optional = self
            .json_parsers
            .iter()
            .filter(|recipe| recipe.input == input)
            .find_map(|recipe| producer(recipe, self));
        let Some((feed, index)) = optional else {
            return self.value_node(feed);
        };
        if let Some(node) = self.parser_input_nodes.get(&feed) {
            return Some(*node);
        }
        let parameter = &self.fn_components[index];
        let name = parameter.input_parameter_name.as_ref()?.name.clone();
        let preview = parameter.input_preview.clone();
        let default = self.const_null();
        let node = self.alloc(Node::RuntimeParameterDefault {
            name: name.clone(),
            ty: ScalarType::String,
            default,
            preview,
        });
        self.parser_input_nodes.insert(feed, node);
        self.optional_parser_input_names.insert(node, name);
        Some(node)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use ir::{SchemaNode, Value};
    use mapping::Graph;

    use super::*;

    #[test]
    fn parser_cache_never_makes_direct_optional_demand_supported_in_either_order() {
        let document = roxmltree::Document::parse(include_str!(
            "../../tests/fixtures/optional-parser-input/mixed.mfd"
        ))
        .unwrap();
        let parser = document
            .descendants()
            .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("parsed"))
            .unwrap();
        let parameter = document
            .descendants()
            .find(|node| {
                node.has_tag_name("component") && node.attribute("name") == Some("payload")
            })
            .unwrap();
        let functions = [super::super::function::read(&parameter)];
        let recipes = [super::super::json_parser::Recipe {
            input: 410,
            schema: SchemaNode::group("Payload", vec![]),
            unresolved_schema_reference: None,
            outputs: BTreeMap::new(),
            object_outputs: BTreeMap::new(),
            runtime_object_outputs: BTreeSet::new(),
            runtime_scalar_outputs: BTreeSet::new(),
            required_string_inputs: Vec::new(),
            optional_string_inputs: read(parser),
            lookup_pins: Default::default(),
        }];
        let edges = BTreeMap::from([(410, 401)]);
        let sources = [];
        let names = [];
        let intermediates = [];
        let json_serializers = [];
        let xml_serializers = [];
        let external_scalar = [];
        let external_xslt = [];
        let flextext_parsers = [];
        let source_functions = super::super::source_node_function::Rules::default();
        let udf_calls = [];
        let udf_registry = super::super::udf::Registry::default();
        for parser_first in [false, true] {
            let mut builder = GraphBuilder {
                graph: Graph::default(),
                native_decimal_input_names: BTreeMap::new(),
                optional_parser_input_names: BTreeMap::new(),
                parser_input_nodes: BTreeMap::new(),
                next_id: 0,
                fn_nodes: BTreeMap::new(),
                sequence_items: BTreeMap::new(),
                sequence_scope_components: BTreeSet::new(),
                sequence_predicate_components: BTreeSet::new(),
                warned_sequence_uses: BTreeSet::new(),
                warned_scalar_filters: BTreeSet::new(),
                warned_join_controls: BTreeSet::new(),
                warned_variable_constructions: BTreeSet::new(),
                rejected_join_paths: BTreeSet::new(),
                source_fields: BTreeMap::new(),
                json_serializer_nodes: BTreeMap::new(),
                xml_serializer_nodes: BTreeMap::new(),
                external_scalar_nodes: BTreeMap::new(),
                external_xslt_nodes: BTreeMap::new(),
                json_parser_nodes: BTreeMap::new(),
                flextext_parser_nodes: BTreeMap::new(),
                source_node_function_nodes: BTreeMap::new(),
                claimed_dynamic_ports: BTreeSet::new(),
                query_scope_sources: BTreeSet::new(),
                warned_unscoped_queries: BTreeSet::new(),
                xml_type_conditions: BTreeMap::new(),
                xml_condition_ports: BTreeSet::new(),
                edge_from: &edges,
                csv_occurrence_raw_authority: false,
                sources: &sources,
                source_names: &names,
                intermediates: &intermediates,
                json_serializers: &json_serializers,
                xml_serializers: &xml_serializers,
                external_scalar_recipes: &external_scalar,
                external_xslt_aggregates: &external_xslt,
                json_parsers: &recipes,
                flextext_parsers: &flextext_parsers,
                source_node_functions: &source_functions,
                fn_components: &functions,
                fn_by_output: BTreeMap::from([(401, 0)]),
                udf_nodes: BTreeMap::new(),
                udf_by_output: BTreeMap::new(),
                udf_calls: &udf_calls,
                udf_registry: &udf_registry,
                joins: super::super::join::Registry::default(),
                framed: BTreeSet::new(),
                warnings: Vec::new(),
            };
            let first = parser_first.then(|| builder.parser_input_node(410).unwrap());
            assert!(builder.value_node(401).is_none());
            let adapter = builder.parser_input_node(410).unwrap();
            if let Some(first) = first {
                assert_eq!(first, adapter);
            }
            assert_eq!(builder.parser_input_node(410), Some(adapter));
            assert!(builder.value_node(401).is_none());
            assert!(builder.fn_nodes.is_empty());
            assert_eq!(builder.next_id, 2);
            assert_eq!(builder.parser_input_nodes, BTreeMap::from([(401, adapter)]));
            assert!(matches!(
                builder.graph.nodes.get(&0),
                Some(Node::Const { value: Value::Null })
            ));
            assert!(matches!(
                builder.graph.nodes.get(&adapter),
                Some(Node::RuntimeParameterDefault { default: 0, .. })
            ));
            assert_eq!(
                builder.optional_parser_input_names,
                BTreeMap::from([(adapter, "payload".into())])
            );
        }
    }
}
