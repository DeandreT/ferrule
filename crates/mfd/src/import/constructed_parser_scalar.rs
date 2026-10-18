use ir::{ScalarType, SchemaNode};

use super::graph::GraphBuilder;
use super::schema::{parse_u32, schema_node_at};

/// Raw metadata witnesses are separate from normalized function pins.
/// Normalization must not make duplicate or keyless declarations admissible.
pub(super) struct RequiredStringInput {
    output: u32,
    name: String,
    default_pins: Vec<Option<u32>>,
    default_connected: bool,
}

pub(super) fn required_string_inputs(parser: roxmltree::Node<'_, '_>) -> Vec<RequiredStringInput> {
    let Some(siblings) = parser.parent() else {
        return Vec::new();
    };
    let components: Vec<_> = siblings
        .children()
        .filter(|node| node.has_tag_name("component"))
        .collect();
    components
        .iter()
        .filter_map(|component| required_string_input(*component, &components))
        .collect()
}

fn required_string_input(
    component: roxmltree::Node<'_, '_>,
    siblings: &[roxmltree::Node<'_, '_>],
) -> Option<RequiredStringInput> {
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
        .any(|node| !matches!(node.tag_name().name(), "input" | "parameter"))
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
        || input
            .attribute("datatype")
            .is_some_and(|datatype| datatype != "string")
        || input
            .attribute("type")
            .is_some_and(|datatype| datatype != "string")
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
        || !matches!(parameter.attribute("optional"), None | Some("0"))
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
    // Preserve the saved absence grammar before normalizing pins. A keyless
    // position-zero placeholder is not an unknown or connected default.
    let default_pins = super::optional_parser_input::default_pins(component)?;
    if default_pins.contains(&Some(output)) {
        return None;
    }
    let default_connected = default_pins
        .iter()
        .flatten()
        .any(|key| super::optional_parser_input::raw_default_is_connected(component, *key));
    Some(RequiredStringInput {
        output,
        name: name.to_string(),
        default_pins,
        default_connected,
    })
}

pub(super) fn exact_pins(
    component: roxmltree::Node<'_, '_>,
    tag: &str,
    count: usize,
) -> Option<Vec<u32>> {
    let mut containers = component.children().filter(|node| node.has_tag_name(tag));
    let Some(container) = containers.next() else {
        return (count == 0).then(Vec::new);
    };
    if containers.next().is_some() {
        return None;
    }
    let points: Vec<_> = container
        .children()
        .filter(roxmltree::Node::is_element)
        .collect();
    if points.len() != count {
        return None;
    }
    let mut keys = vec![0; count];
    let mut positions = std::collections::BTreeSet::new();
    let mut distinct = std::collections::BTreeSet::new();
    for (index, point) in points.iter().enumerate() {
        if !point.has_tag_name("datapoint") {
            return None;
        }
        let position = match point.attribute("pos") {
            Some(raw) => usize::try_from(parse_u32(Some(raw))?).ok()?,
            None => index,
        };
        let key = parse_u32(point.attribute("key"))?;
        if position >= count || !positions.insert(position) || !distinct.insert(key) {
            return None;
        }
        keys[position] = key;
    }
    Some(keys)
}

pub(super) fn raw_parser_output_is_unique(parser: roxmltree::Node<'_, '_>, output: u32) -> bool {
    let parameters: Vec<_> = parser
        .descendants()
        .filter(|node| node.has_tag_name("parameter"))
        .collect();
    matches!(parameters.as_slice(), [parameter] if parameter.attribute("usageKind") == Some("stringparse"))
        && !parser.descendants().any(|node| {
            node.has_tag_name("entry")
                && matches!(
                    node.attribute("name"),
                    Some("compute-when" | "parent-context")
                )
        })
        && parser
            .descendants()
            .filter(|node| {
                node.has_tag_name("entry") && parse_u32(node.attribute("outkey")) == Some(output)
            })
            .count()
            == 1
        && parser.parent().is_some_and(|siblings| {
            declared_output_count(
                siblings
                    .children()
                    .filter(|node| node.has_tag_name("component")),
                output,
            ) == 1
        })
}

pub(super) fn declared_output_count<'a, 'input: 'a>(
    components: impl Iterator<Item = roxmltree::Node<'a, 'input>>,
    output: u32,
) -> usize {
    components
        .map(|component| {
            let schema = component
                .children()
                .filter(|node| node.has_tag_name("data"))
                .flat_map(|data| data.children().filter(|node| node.has_tag_name("root")))
                .flat_map(|root| root.descendants())
                .filter(|node| {
                    node.has_tag_name("entry")
                        && parse_u32(node.attribute("outkey")) == Some(output)
                })
                .count();
            let function = component
                .children()
                .filter(|node| node.has_tag_name("targets"))
                .flat_map(|targets| targets.children())
                .filter(|node| {
                    node.has_tag_name("datapoint")
                        && parse_u32(node.attribute("key")) == Some(output)
                })
                .count();
            schema + function
        })
        .sum()
}

/// A required named parameter is global to the execution context. A parser
/// scalar depending only on it introduces no source iteration or parent frame.
pub(super) fn schema<'a>(output: u32, builder: &'a GraphBuilder<'_>) -> Option<&'a SchemaNode> {
    let mut recipes = builder
        .json_parsers
        .iter()
        .filter(|recipe| recipe.outputs.contains_key(&output));
    let recipe = recipes.next()?;
    if recipes.next().is_some()
        || !recipe.runtime_scalar_outputs.contains(&output)
        || recipe.unresolved_schema_reference.is_some()
        || output_is_shadowed(output, builder)
    {
        return None;
    }
    let path = recipe.outputs.get(&output)?;
    let scalar = schema_node_at(&recipe.schema, path)?;
    if !scalar.is_scalar()
        || (0..=path.len()).any(|length| {
            schema_node_at(&recipe.schema, &path[..length]).is_none_or(|node| node.repeating)
        })
    {
        return None;
    }
    (input_is_required_string(recipe, builder)
        || super::optional_parser_input::input_is_optional_string(recipe, builder))
    .then_some(scalar)
}

pub(super) fn input_is_required_string(
    recipe: &super::json_parser::Recipe,
    builder: &GraphBuilder<'_>,
) -> bool {
    required_string_parameter(recipe, builder).is_some()
}

fn required_string_parameter(
    recipe: &super::json_parser::Recipe,
    builder: &GraphBuilder<'_>,
) -> Option<()> {
    let feed = *builder.edge_from.get(&recipe.input)?;
    let witness = recipe
        .required_string_inputs
        .iter()
        .find(|input| input.output == feed)?;
    if output_is_nonfunction_shadowed(feed, builder)
        || builder.json_parsers.iter().any(|parser| {
            parser.outputs.contains_key(&feed) || parser.object_outputs.contains_key(&feed)
        })
    {
        return None;
    }
    let mut producers = builder
        .fn_components
        .iter()
        .filter(|function| function.declared_output_keys.contains(&feed));
    let parameter = producers.next()?;
    if producers.next().is_some()
        || parameter.library != "core"
        || parameter.kind != 6
        || !parameter.raw_single_output_valid
        || parameter.input_type != Some(ScalarType::String)
        || parameter.outputs.as_slice() != [feed]
        || parameter.output_pins.as_slice() != [Some(feed)]
        || parameter.inputs != witness.default_pins
        || witness.default_connected
        || parameter
            .inputs
            .iter()
            .flatten()
            .any(|input| builder.edge_from.contains_key(input))
        || !parameter
            .input_parameter_name
            .as_ref()
            .is_some_and(|parameter| !parameter.optional && parameter.name == witness.name)
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
    Some(())
}

pub(super) fn output_is_nonfunction_shadowed(output: u32, builder: &GraphBuilder<'_>) -> bool {
    builder
        .sources
        .iter()
        .any(|source| source.declares_output(output) || source.ports.contains_key(&output))
        || builder
            .intermediates
            .iter()
            .any(|owner| owner.declares_output(output))
        || builder.udf_by_output.contains_key(&output)
        || builder.has_source_node_functions(output)
}

pub(super) fn output_is_shadowed(output: u32, builder: &GraphBuilder<'_>) -> bool {
    output_is_nonfunction_shadowed(output, builder)
        || builder
            .fn_components
            .iter()
            .any(|function| function.declared_output_keys.contains(&output))
}
