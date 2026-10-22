use std::collections::BTreeSet;

use ir::{ScalarType, Value};
use mapping::AggregateOp;

use crate::canonical_function;

use super::schema::parse_u32;

mod clock;
mod count;
mod parse_date;
#[cfg(test)]
#[path = "function/total_string_tests.rs"]
mod total_string_tests;
pub(super) use parse_date::RawParseDateOwnership;
mod group_by;
pub(super) use group_by::RawGroupByOwnership;
mod presence;
pub(super) use presence::RawPresenceOperator;

#[cfg(test)]
#[path = "function/conditional_string_tests.rs"]
mod conditional_string_tests;

/// Typed lookup data declared by a native .mfd value-map component.
#[derive(Clone, Default)]
pub(super) struct ValueMapData {
    pub(super) table: Vec<(Value, Value)>,
    pub(super) default: Option<Value>,
    pub(super) input_type: Option<ScalarType>,
    /// Retains an unsupported native fallback mode until import admission.
    pub(super) unsupported_input_mode: bool,
    /// Exact bounded raw metadata for total enclosing-frame String maps.
    /// General value-map parsing keeps its independent compatibility rules.
    pub(super) total_string: Option<TotalStringValueMap>,
}

const VALUE_MAP_INPUT_MODE_UNSUPPORTED: &str =
    "value-map defaultValueMode=\"input\" is unsupported; input fallback is not imported";

impl ValueMapData {
    pub(super) fn unsupported_reason(&self) -> Option<&'static str> {
        self.unsupported_input_mode
            .then_some(VALUE_MAP_INPUT_MODE_UNSUPPORTED)
    }
}

/// Only physical native components own value-map semantics, not preview or
/// schema metadata containing similarly named elements.
pub(super) fn unsupported_value_map_input_mode(
    component: &roxmltree::Node<'_, '_>,
) -> Option<&'static str> {
    (component.has_tag_name("component")
        && component.tag_name().namespace().is_none()
        && component.attribute("library") == Some("core")
        && parse_u32(component.attribute("kind")) == Some(23)
        && component
            .children()
            .filter(|node| node.has_tag_name("data") && node.tag_name().namespace().is_none())
            .flat_map(|data| data.children())
            .any(|node| {
                node.has_tag_name("valuemap")
                    && node.tag_name().namespace().is_none()
                    && node.attributes().any(|attribute| {
                        attribute.namespace().is_none()
                            && attribute.name() == "defaultValueMode"
                            && attribute.value() == "input"
                    })
            }))
    .then_some(VALUE_MAP_INPUT_MODE_UNSUPPORTED)
}

fn body_components<'a, 'input>(
    owner: &roxmltree::Node<'a, 'input>,
) -> impl Iterator<Item = roxmltree::Node<'a, 'input>> {
    owner
        .children()
        .filter(|node| node.has_tag_name("structure") && node.tag_name().namespace().is_none())
        .flat_map(|structure| structure.children())
        .filter(|node| node.has_tag_name("children") && node.tag_name().namespace().is_none())
        .flat_map(|children| children.children())
        .filter(|node| node.has_tag_name("component") && node.tag_name().namespace().is_none())
}

pub(super) fn unsupported_value_map_input_mode_in_definition(
    component: &roxmltree::Node<'_, '_>,
) -> Option<&'static str> {
    body_components(component).find_map(|child| unsupported_value_map_input_mode(&child))
}

pub(super) fn value_map_input_mode_warning(library: &str, name: &str) -> String {
    format!("function `{name}` ({library}): {VALUE_MAP_INPUT_MODE_UNSUPPORTED}")
}

/// Census only the wrapper's actual body and the declaration bodies already
/// examined by the UDF registry. This also retains diagnostics for unused
/// definitions and specialized lowering routes which never call fn_node.
pub(super) fn warn_unsupported_value_map_input_modes(
    mapping: &roxmltree::Node<'_, '_>,
    warnings: &mut Vec<String>,
) {
    if let Some(wrapper) = mapping
        .children()
        .find(|node| node.has_tag_name("component") && node.tag_name().namespace().is_none())
    {
        for component in body_components(&wrapper) {
            if unsupported_value_map_input_mode(&component).is_some() {
                let name = component.attribute("name").unwrap_or_default();
                let warning = value_map_input_mode_warning("core", name);
                if !warnings.contains(&warning) {
                    warnings.push(warning);
                }
            }
        }
    }
    for component in mapping
        .children()
        .filter(|node| node.has_tag_name("component") && node.tag_name().namespace().is_none())
    {
        let library = component.attribute("library").unwrap_or_default();
        if !library.is_empty()
            && let Some(reason) = unsupported_value_map_input_mode_in_definition(&component)
        {
            let name = component.attribute("name").unwrap_or_default();
            let warning =
                format!("user-defined function `{name}` ({library}) is unsupported: {reason}");
            if !warnings.contains(&warning) {
                warnings.push(warning);
            }
        }
    }
}

#[derive(Clone)]
pub(super) struct TotalStringValueMap {
    pub(super) table: Vec<(String, String)>,
    pub(super) default: String,
    pub(super) ownership: Option<RawValueMapOwnership>,
}

#[derive(Clone)]
pub(super) struct RawValueMapOwnership {
    pub(super) output: u32,
    pub(super) source: u32,
}

/// Raw output and feed ownership across every structure in the document.
/// Skipped component kinds must not disappear from a bounded scalar proof.
pub(super) struct RawStringSubstitution {
    pub(super) left_pin: u32,
    pub(super) fallback_pin: u32,
    pub(super) output: u32,
    pub(super) left: u32,
    pub(super) fallback: u32,
    pub(super) constant: Option<String>,
    pub(super) parser_input: Option<u32>,
}

pub(super) struct RawConditionalOwnership {
    pub(super) output: u32,
    pub(super) sources: [u32; 3],
    pub(super) primary_outputs: BTreeSet<u32>,
}

#[derive(Clone)]
pub(super) enum DbWhereComponent {
    Supported(DbWhere),
    Unsupported(String),
}

#[derive(Clone)]
pub(super) struct DbWhere {
    pub(super) predicate: DbPredicate,
    pub(super) order: Option<DbOrder>,
    pub(super) parameter_type: ScalarType,
}

#[derive(Clone)]
pub(super) struct DbPredicate {
    pub(super) column: Vec<String>,
    pub(super) operator: DbPredicateOperator,
}

#[derive(Clone, Copy)]
pub(super) enum DbPredicateOperator {
    Equal,
    Like,
}

#[derive(Clone)]
pub(super) struct DbOrder {
    pub(super) column: Vec<String>,
    pub(super) descending: bool,
}

pub(super) struct FnComponent {
    pub(super) library: String,
    pub(super) name: String,
    pub(super) kind: u32,
    /// Input pins in `pos` order; `None` for declared-but-keyless pins.
    pub(super) inputs: Vec<Option<u32>>,
    /// Output pin keys in `pos` order.
    pub(super) outputs: Vec<u32>,
    /// Declared output positions, retaining keyless pins so a secondary
    /// output cannot be mistaken for the primary result.
    pub(super) output_pins: Vec<Option<u32>>,
    /// Raw direct output keys, including declarations discarded by pin normalization.
    pub(super) declared_output_keys: BTreeSet<u32>,
    /// One exact declared output matching the normalized primary result.
    pub(super) raw_single_output_valid: bool,
    /// Exact raw pin metadata for bounded scalar constants and conditionals.
    /// The general pin reader may normalize malformed positions; this proof may not.
    pub(super) bounded_scalar_pins_valid: bool,
    pub(super) lazy_string_substitution: Option<RawStringSubstitution>,
    /// Exact saved owners and feeds for physical String conditional branches.
    /// Skipped libraries and omitted schema roots still participate in this census.
    pub(super) conditional_ownership: Option<RawConditionalOwnership>,
    /// Dedicated exact raw two-input parse-date contract. This never broadens
    /// the shared bounded scalar contract used by other importer routes.
    pub(super) raw_parse_date_pins_valid: bool,
    /// Raw owners and connected operand feeds from the entire owning structure,
    /// including components which the ordinary reader may skip.
    pub(super) parse_date_ownership: Option<RawParseDateOwnership>,
    /// Exact inputless `lang` `now` / `xpath2` `current-dateTime` signature.
    /// Separate from the bounded scalar contract and the general clock lowering.
    pub(super) raw_clock_signature_valid: bool,
    /// Exact `core` `count` signature: keyless context slot, keyed sequence
    /// input, and one keyed output. Separate from general aggregate lowering.
    pub(super) raw_count_signature_valid: bool,
    /// Dedicated raw owner and source-position proof for String group-by keys.
    pub(super) group_by_ownership: Option<RawGroupByOwnership>,
    /// Exact raw pins of a group-presence operand operator, retaining keyless
    /// `if-else` branches. Independent of `bounded_scalar_pins_valid`.
    pub(super) presence_operator: Option<RawPresenceOperator>,
    /// Scalar coercion declared by a transparent kind=6 input parameter.
    pub(super) input_type: Option<ScalarType>,
    /// Public host parameter declaration of a kind=6 input component.
    pub(super) input_parameter_name: Option<InputParameter>,
    /// Raw lexical value used only during design-time preview. Keep it
    /// separate from a connected runtime default and preserve empty values.
    pub(super) input_preview: Option<String>,
    /// One raw literal datum with explicit value/type, independent of the
    /// general reader's first-data/first-constant normalization.
    pub(super) raw_literal_valid: bool,
    pub(super) constant: Option<(String, String)>,
    pub(super) valuemap: Option<ValueMapData>,
    /// Sort directions in key-index order; `Some` also identifies a sort
    /// component whose key declarations are malformed or absent.
    pub(super) sort_directions: Option<Vec<bool>>,
    pub(super) db_where: Option<DbWhereComponent>,
    pub(super) recursive: Option<RecursiveComponent>,
}

#[derive(Clone)]
pub(super) struct InputParameter {
    pub(super) name: String,
    pub(super) optional: bool,
}

#[derive(Clone)]
pub(super) enum RecursiveComponent {
    Invalid,
    Collect {
        collection: Vec<String>,
        children: Vec<String>,
        descent_value: Vec<String>,
        values: Vec<String>,
        value: Vec<String>,
    },
    Filter {
        children: String,
        items: String,
    },
    PathHierarchy {
        collection: Vec<String>,
        separator: String,
        directories: String,
        files: String,
        name: String,
    },
    AdjacencyTree {
        collection: Vec<String>,
        key: Vec<String>,
        parent: Vec<String>,
        target_key: String,
        target_children: String,
        has_root: bool,
    },
}

pub(super) fn read(component: &roxmltree::Node) -> FnComponent {
    let library = component
        .attribute("library")
        .unwrap_or_default()
        .to_string();
    let mut name = component.attribute("name").unwrap_or_default().to_string();
    if library == "edifact" && name == "to-datetime" {
        name = "edifact-to-datetime".to_string();
    }
    let kind = parse_u32(component.attribute("kind")).unwrap_or(0);
    let pins = |tag: &str| -> Vec<Option<u32>> {
        component
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == tag)
            .map(|pins| {
                let datapoints: Vec<_> = pins
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "datapoint")
                    .collect();
                let mut ordered = Vec::new();
                for (index, datapoint) in datapoints.into_iter().enumerate() {
                    let position = parse_u32(datapoint.attribute("pos"))
                        .and_then(|position| usize::try_from(position).ok())
                        .filter(|position| *position < 64)
                        .unwrap_or(index);
                    if ordered.len() <= position {
                        ordered.resize(position + 1, None);
                    }
                    if ordered[position].is_none() {
                        ordered[position] = parse_u32(datapoint.attribute("key"));
                    }
                }
                ordered
            })
            .unwrap_or_default()
    };
    let inputs = pins("sources");
    let output_pins = pins("targets");
    let outputs: Vec<u32> = output_pins.iter().flatten().copied().collect();
    let declared_output_keys = component
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "targets")
        .flat_map(|targets| {
            targets
                .children()
                .filter(|node| node.is_element() && node.tag_name().name() == "datapoint")
        })
        .filter_map(|point| parse_u32(point.attribute("key")))
        .collect();
    let raw_single_output_valid = exact_pin_keys(component, "targets", 1)
        .is_some_and(|keys| keys == outputs && output_pins.as_slice() == [Some(keys[0])]);
    let bounded_scalar_pins_valid = library == "core"
        && match (kind, name.as_str()) {
            (2, "constant") => Some(0),
            (4, "if-else") => Some(3),
            (23, "value-map") => Some(1),
            _ => None,
        }
        .is_some_and(|count| {
            exact_pin_keys(component, "sources", count).is_some_and(|input_keys| {
                exact_pin_keys(component, "targets", 1).is_some_and(|output_keys| {
                    input_keys
                        .iter()
                        .copied()
                        .map(Some)
                        .eq(inputs.iter().copied())
                        && output_keys == outputs
                        && output_keys
                            .iter()
                            .copied()
                            .map(Some)
                            .eq(output_pins.iter().copied())
                        && !input_keys.contains(&output_keys[0])
                })
            })
        });

    let raw_parse_date_pins_valid = parse_date::pins_valid(component, &inputs, &output_pins);
    let parse_date_ownership = raw_parse_date_pins_valid
        .then(|| parse_date::ownership(component))
        .flatten();
    let raw_clock_signature_valid = clock::signature_valid(component, &output_pins);
    let raw_count_signature_valid = count::signature_valid(component, &inputs, &output_pins);

    let data = component
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "data");
    let input_type = (library == "core" && kind == 6)
        .then(|| {
            data.and_then(|data| data.descendants().find(|node| node.has_tag_name("input")))
                .and_then(|input| {
                    input
                        .attribute("datatype")
                        .or_else(|| input.attribute("type"))
                })
                .map(parse_scalar_type)
        })
        .flatten();
    let input_parameter_name = (library == "core" && kind == 6)
        .then(|| {
            data.and_then(|data| {
                data.descendants()
                    .find(|node| {
                        node.has_tag_name("parameter")
                            && node.attribute("usageKind") == Some("input")
                    })
                    .and_then(|parameter| {
                        parameter
                            .attribute("name")
                            .filter(|name| !name.is_empty())
                            .map(|name| InputParameter {
                                name: name.to_string(),
                                optional: parameter.attribute("optional") == Some("1"),
                            })
                    })
            })
        })
        .flatten();
    let input_preview = (library == "core" && kind == 6)
        .then(|| {
            data.and_then(|data| data.descendants().find(|node| node.has_tag_name("input")))
                .filter(|input| input.attribute("usepreviewvalue") == Some("1"))
                .map(|input| {
                    input
                        .attribute("previewvalue")
                        .unwrap_or_default()
                        .to_string()
                })
        })
        .flatten();
    let raw_literal_valid = exact_literal_data(component);
    let constant = data
        .and_then(|d| {
            d.children()
                .find(|n| n.is_element() && n.tag_name().name() == "constant")
        })
        .map(|c| {
            (
                c.attribute("value").unwrap_or_default().to_string(),
                c.attribute("datatype").unwrap_or_default().to_string(),
            )
        });
    let valuemap = data
        .and_then(|d| {
            d.children()
                .find(|n| n.is_element() && n.tag_name().name() == "valuemap")
        })
        .map(|vm| {
            let unsupported_input_mode = unsupported_value_map_input_mode(component).is_some();
            let input_type = vm
                .descendants()
                .find(|node| node.has_tag_name("input"))
                .and_then(|node| node.attribute("type"))
                .map(parse_scalar_type)
                .or(Some(ScalarType::String));
            let result_type = vm
                .descendants()
                .find(|node| node.has_tag_name("result"))
                .and_then(|node| node.attribute("type"))
                .map(parse_scalar_type)
                .unwrap_or(ScalarType::String);
            let table = vm
                .descendants()
                .filter(|n| n.has_tag_name("entry"))
                .map(|e| {
                    (
                        parse_constant(
                            e.attribute("from").unwrap_or_default(),
                            scalar_type_name(input_type.unwrap_or(ScalarType::String)),
                        ),
                        parse_constant(
                            e.attribute("to").unwrap_or_default(),
                            scalar_type_name(result_type),
                        ),
                    )
                })
                .collect();
            let default = vm
                .descendants()
                .find(|n| n.has_tag_name("result"))
                .and_then(|r| r.attribute("defaultValue"))
                .map(|value| parse_constant(value, scalar_type_name(result_type)))
                .filter(|_| {
                    !unsupported_input_mode
                        && (vm.attribute("defaultValueMode") == Some("custom")
                            || vm.attribute("enableDefaultValue") == Some("1"))
                });
            ValueMapData {
                table,
                default,
                input_type,
                unsupported_input_mode,
                total_string: exact_total_string_value_map(component, vm),
            }
        });
    let sort_directions = data
        .and_then(|data| data.descendants().find(|node| node.has_tag_name("sort")))
        .map(|sort| {
            let mut keys = sort
                .descendants()
                .filter(|node| node.has_tag_name("key"))
                .map(|key| {
                    let index = key
                        .attribute("index")
                        .and_then(|index| index.parse::<usize>().ok())
                        .unwrap_or(0);
                    (index, key.attribute("direction") == Some("descending"))
                })
                .collect::<Vec<_>>();
            keys.sort_by_key(|(index, _)| *index);
            keys.into_iter().map(|(_, descending)| descending).collect()
        });
    let db_where =
        (library == "db" && kind == 21).then(|| parse_db_where(component, &inputs, &outputs));
    let conditional_ownership = if library == "core" && name == "if-else" && kind == 4 {
        exact_conditional_ownership(component)
    } else {
        None
    };

    FnComponent {
        library,
        name,
        kind,
        inputs,
        outputs,
        output_pins,
        declared_output_keys,
        raw_single_output_valid,
        bounded_scalar_pins_valid,
        lazy_string_substitution: exact_string_substitution(component),
        conditional_ownership,
        raw_parse_date_pins_valid,
        parse_date_ownership,
        raw_clock_signature_valid,
        raw_count_signature_valid,
        group_by_ownership: group_by::ownership(component),
        presence_operator: presence::operator(component),
        input_type,
        input_parameter_name,
        input_preview,
        raw_literal_valid,
        constant,
        valuemap,
        sort_directions,
        db_where,
        recursive: None,
    }
}

fn exact_string_substitution(component: &roxmltree::Node) -> Option<RawStringSubstitution> {
    if !matches!(
        (
            component.attribute("library"),
            component.attribute("kind"),
            component.attribute("name")
        ),
        (Some("core"), Some("5"), Some("substitute-missing"))
            | (Some("db"), Some("5"), Some("substitute-null"))
    ) || !exact_component_children(component, &["sources", "targets"])
    {
        return None;
    }
    let inputs = exact_substitution_pins(component, "sources", 2)?;
    let output = exact_substitution_pins(component, "targets", 1)?[0];
    let structure = component
        .parent()?
        .parent()
        .filter(|node| node.has_tag_name("structure"))?;
    let parent = structure.parent();
    let edges = super::graph::read_edge_inputs(&structure, parent.as_ref());
    let left = *edges.feeds.get(&inputs[0])?;
    let fallback = *edges.feeds.get(&inputs[1])?;
    let owners = || {
        component.document().root().descendants().filter(|node| {
            node.has_tag_name("component")
                && node.parent().is_some_and(|children| {
                    children.has_tag_name("children")
                        && children
                            .parent()
                            .is_some_and(|node| node.has_tag_name("structure"))
                })
        })
    };
    if [output, left, fallback]
        .iter()
        .any(|key| super::constructed_parser_scalar::declared_output_count(owners(), *key) != 1)
    {
        return None;
    }
    let constant = owners().find_map(|owner| exact_string_constant(owner, fallback));
    let parser_input = owners().find_map(|owner| {
        if !owner.descendants().any(|node| {
            node.has_tag_name("parameter") && node.attribute("usageKind") == Some("stringparse")
        }) || super::constructed_parser_scalar::declared_output_count(
            std::iter::once(owner),
            fallback,
        ) != 1
        {
            return None;
        }
        let inputs = owner
            .descendants()
            .filter(|node| node.has_tag_name("entry"))
            .filter_map(|node| parse_u32(node.attribute("inpkey")))
            .collect::<Vec<_>>();
        let [input] = inputs.as_slice() else {
            return None;
        };
        let feed = *edges.feeds.get(input)?;
        (super::constructed_parser_scalar::declared_output_count(owners(), feed) == 1)
            .then_some(feed)
    });
    Some(RawStringSubstitution {
        left_pin: inputs[0],
        fallback_pin: inputs[1],
        output,
        left,
        fallback,
        constant,
        parser_input,
    })
}

fn exact_component_children(component: &roxmltree::Node, allowed: &[&str]) -> bool {
    component.tag_name().namespace().is_none()
        && component.attributes().all(|attribute| {
            attribute.namespace().is_none()
                && matches!(attribute.name(), "name" | "library" | "kind" | "uid")
        })
        && component.children().all(|node| {
            !node.is_element()
                || node.tag_name().namespace().is_none()
                    && allowed.contains(&node.tag_name().name())
        })
        && !component
            .children()
            .any(|node| node.is_text() && node.text().is_some_and(|text| !text.trim().is_empty()))
}

fn exact_substitution_pins(
    component: &roxmltree::Node,
    tag: &str,
    count: usize,
) -> Option<Vec<u32>> {
    let keys = exact_pin_keys(component, tag, count)?;
    if let Some(container) = component.children().find(|node| node.has_tag_name(tag))
        && (container.tag_name().namespace().is_some()
            || container.attributes().len() != 0
            || container.children().any(|node| {
                node.is_text() && node.text().is_some_and(|text| !text.trim().is_empty())
                    || node.is_element()
                        && !exact_metadata_leaf(
                            node,
                            if node.attribute("pos").is_some() {
                                &["pos", "key"]
                            } else {
                                &["key"]
                            },
                        )
            }))
    {
        return None;
    }
    Some(keys)
}

fn exact_string_constant(component: roxmltree::Node, output: u32) -> Option<String> {
    if component.attribute("library") != Some("core")
        || component.attribute("kind") != Some("2")
        || !exact_component_children(&component, &["sources", "targets", "data"])
        || !exact_substitution_pins(&component, "sources", 0)?.is_empty()
        || exact_substitution_pins(&component, "targets", 1)? != [output]
    {
        return None;
    }
    let mut data = component
        .children()
        .filter(|node| node.has_tag_name("data"));
    let data_node = data.next()?;
    let constants = data_node
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [constant] = constants.as_slice() else {
        return None;
    };
    if data.next().is_some()
        || data_node.tag_name().namespace().is_some()
        || data_node.attributes().len() != 0
        || data_node
            .children()
            .any(|node| node.is_text() && node.text().is_some_and(|text| !text.trim().is_empty()))
        || !constant.has_tag_name("constant")
        || !exact_metadata_leaf(*constant, &["datatype", "value"])
        || constant.attribute("datatype") != Some("string")
    {
        return None;
    }
    Some(constant.attribute("value")?.to_string())
}

fn exact_total_string_value_map(
    component: &roxmltree::Node,
    map: roxmltree::Node,
) -> Option<TotalStringValueMap> {
    const MAX_ENTRIES: usize = 256;
    const MAX_STRING_BYTES: usize = 65_536;

    let mut data = component
        .children()
        .filter(|node| node.has_tag_name("data"));
    let container = data.next()?;
    if data.next().is_some()
        || container.tag_name().namespace().is_some()
        || container.attributes().len() != 0
        || container
            .children()
            .filter(roxmltree::Node::is_element)
            .collect::<Vec<_>>()
            != [map]
        || map.tag_name().namespace().is_some()
        || map.attributes().len() != 1
        || !(map.attribute("defaultValueMode") == Some("custom")
            || map.attribute("enableDefaultValue") == Some("1"))
        || !exact_single_value_map_pin(component, "sources")
        || !exact_single_value_map_pin(component, "targets")
    {
        return None;
    }
    let children = map
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    if children.len() != 3 {
        return None;
    }
    let one = |name| {
        let mut found = children
            .iter()
            .copied()
            .filter(|node| node.has_tag_name(name));
        let node = found.next()?;
        found.next().is_none().then_some(node)
    };
    let input = one("input")?;
    let result = one("result")?;
    let table = one("valuemapTable")?;
    if !exact_metadata_leaf(input, &["name", "type"])
        || input.attribute("name") != Some("input")
        || input.attribute("type") != Some("string")
        || !exact_metadata_leaf(result, &["name", "type", "defaultValue"])
        || result.attribute("name") != Some("result")
        || result.attribute("type") != Some("string")
        || table.tag_name().namespace().is_some()
        || table.attributes().len() != 0
    {
        return None;
    }
    let default = result.attribute("defaultValue")?;
    let mut bytes = default.len();
    if bytes > MAX_STRING_BYTES {
        return None;
    }
    let mut entries = Vec::new();
    for entry in table.children().filter(roxmltree::Node::is_element) {
        if entries.len() == MAX_ENTRIES
            || !entry.has_tag_name("entry")
            || !exact_metadata_leaf(entry, &["from", "to"])
        {
            return None;
        }
        let from = entry.attribute("from")?;
        let to = entry.attribute("to")?;
        bytes = bytes.checked_add(from.len())?.checked_add(to.len())?;
        if bytes > MAX_STRING_BYTES {
            return None;
        }
        entries.push((from.to_string(), to.to_string()));
    }
    // Unknown text is not silently treated as an empty or partial table.
    if [container, map, table].iter().any(|node| {
        node.children().any(|child| {
            child.is_text() && child.text().is_some_and(|text| !text.trim().is_empty())
        })
    }) {
        return None;
    }
    Some(TotalStringValueMap {
        table: entries,
        default: default.to_string(),
        ownership: exact_value_map_ownership(component),
    })
}

fn exact_value_map_ownership(component: &roxmltree::Node) -> Option<RawValueMapOwnership> {
    let input = exact_pin_keys(component, "sources", 1)?[0];
    let output = exact_pin_keys(component, "targets", 1)?[0];
    let siblings = component.parent()?;
    let structure = siblings
        .parent()
        .filter(|node| node.has_tag_name("structure"))?;
    let owners = || {
        siblings
            .children()
            .filter(|node| node.has_tag_name("component"))
    };
    if super::constructed_parser_scalar::declared_output_count(owners(), output) != 1 {
        return None;
    }
    let parent = structure.parent();
    let edges = super::graph::read_edge_inputs(&structure, parent.as_ref());
    let source = *edges.feeds.get(&input)?;
    if super::constructed_parser_scalar::declared_output_count(owners(), source) != 1 {
        return None;
    }
    Some(RawValueMapOwnership { output, source })
}

fn exact_conditional_ownership(component: &roxmltree::Node) -> Option<RawConditionalOwnership> {
    let inputs = exact_pin_keys(component, "sources", 3)?;
    let output = exact_pin_keys(component, "targets", 1)?[0];
    let siblings = component.parent()?;
    let structure = siblings
        .parent()
        .filter(|node| node.has_tag_name("structure"))?;
    let owners = || {
        siblings
            .children()
            .filter(|node| node.has_tag_name("component"))
    };
    let count = |key| super::constructed_parser_scalar::declared_output_count(owners(), key);
    if count(output) != 1 {
        return None;
    }
    let parent = structure.parent();
    let edges = super::graph::read_edge_inputs(&structure, parent.as_ref());
    let sources = [
        *edges.feeds.get(&inputs[0])?,
        *edges.feeds.get(&inputs[1])?,
        *edges.feeds.get(&inputs[2])?,
    ];
    if sources.iter().any(|key| count(*key) != 1) {
        return None;
    }
    let primary = owners().find(|owner| {
        super::constructed_parser_scalar::declared_output_count(std::iter::once(*owner), sources[0])
            == 1
    })?;
    if primary.attribute("library") != Some("json") || primary.attribute("kind") != Some("31") {
        return None;
    }
    let primary_outputs = primary
        .children()
        .filter(|node| node.has_tag_name("data"))
        .flat_map(|data| data.children().filter(|node| node.has_tag_name("root")))
        .flat_map(|root| root.descendants())
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|entry| parse_u32(entry.attribute("outkey")))
        .collect::<BTreeSet<_>>();
    if !primary_outputs.contains(&sources[0]) || primary_outputs.iter().any(|key| count(*key) != 1)
    {
        return None;
    }
    Some(RawConditionalOwnership {
        output,
        sources,
        primary_outputs,
    })
}

fn exact_single_value_map_pin(component: &roxmltree::Node, name: &str) -> bool {
    let mut containers = component.children().filter(|node| node.has_tag_name(name));
    let Some(container) = containers.next() else {
        return false;
    };
    if containers.next().is_some()
        || container.tag_name().namespace().is_some()
        || container.attributes().len() != 0
    {
        return false;
    }
    let points = container
        .children()
        .filter(roxmltree::Node::is_element)
        .collect::<Vec<_>>();
    let [point] = points.as_slice() else {
        return false;
    };
    point.has_tag_name("datapoint")
        && exact_metadata_leaf(
            *point,
            if point.attribute("pos").is_some() {
                &["key", "pos"]
            } else {
                &["key"]
            },
        )
        && point
            .attribute("pos")
            .is_none_or(|position| position == "0")
}

fn exact_metadata_leaf(node: roxmltree::Node, attributes: &[&str]) -> bool {
    node.tag_name().namespace().is_none()
        && node.attributes().len() == attributes.len()
        && node.attributes().all(|attribute| {
            attribute.namespace().is_none() && attributes.contains(&attribute.name())
        })
        && attributes
            .iter()
            .all(|name| node.attribute(*name).is_some())
        && !node.children().any(|child| {
            child.is_element()
                || child.is_text() && child.text().is_some_and(|text| !text.trim().is_empty())
        })
}

fn exact_pin_keys(component: &roxmltree::Node, tag: &str, count: usize) -> Option<Vec<u32>> {
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
    let mut seen_keys = std::collections::BTreeSet::new();
    let mut positions = std::collections::BTreeSet::new();
    for (index, point) in points.iter().enumerate() {
        if !point.has_tag_name("datapoint") {
            return None;
        }
        let position = match point.attribute("pos") {
            Some(raw) => usize::try_from(parse_u32(Some(raw))?).ok()?,
            None => index,
        };
        let key = parse_u32(point.attribute("key"))?;
        if position >= count || !positions.insert(position) || !seen_keys.insert(key) {
            return None;
        }
        keys[position] = key;
    }
    Some(keys)
}

fn parse_db_where(
    component: &roxmltree::Node,
    inputs: &[Option<u32>],
    outputs: &[u32],
) -> DbWhereComponent {
    let parse = || -> Result<DbWhere, &'static str> {
        if inputs.len() != 2 || inputs.iter().any(Option::is_none) || outputs.len() != 1 {
            return Err("expected one collection input, one parameter input, and one output");
        }
        let where_node = component
            .descendants()
            .find(|node| node.has_tag_name("where"))
            .ok_or("missing where metadata")?;
        let parameters = where_node
            .descendants()
            .filter(|node| node.has_tag_name("parameter"))
            .collect::<Vec<_>>();
        let [parameter] = parameters.as_slice() else {
            return Err("expected exactly one declared parameter");
        };
        let parameter_name = parameter
            .attribute("name")
            .filter(|name| valid_identifier_segment(name))
            .ok_or("parameter name is not a safe SQL identifier")?;
        let parameter_type = match parameter.attribute("type").unwrap_or_default() {
            "string" | "text" => ScalarType::String,
            _ => return Err("only string parameters are supported in database where controls"),
        };

        let condition = where_node
            .attribute("condition")
            .ok_or("missing where condition")?;
        let tokens = condition.split_ascii_whitespace().collect::<Vec<_>>();
        let [column, operator, bound] = tokens.as_slice() else {
            return Err("condition must be `Identifier (=|LIKE) :Parameter`");
        };
        let column =
            parse_identifier(column).ok_or("condition column is not a safe SQL identifier")?;
        let operator = if *operator == "=" {
            DbPredicateOperator::Equal
        } else if operator.eq_ignore_ascii_case("LIKE") {
            DbPredicateOperator::Like
        } else {
            return Err("condition operator must be `=` or `LIKE`");
        };
        if bound.strip_prefix(':') != Some(parameter_name) {
            return Err("condition parameter does not match its declaration");
        }

        let order = where_node
            .attribute("order")
            .filter(|order| !order.trim().is_empty())
            .map(parse_db_order)
            .transpose()?;
        Ok(DbWhere {
            predicate: DbPredicate { column, operator },
            order,
            parameter_type,
        })
    };
    match parse() {
        Ok(where_control) => DbWhereComponent::Supported(where_control),
        Err(reason) => DbWhereComponent::Unsupported(reason.to_string()),
    }
}

fn parse_db_order(order: &str) -> Result<DbOrder, &'static str> {
    let tokens = order.split_ascii_whitespace().collect::<Vec<_>>();
    let (column, descending) = match tokens.as_slice() {
        [column] => (*column, false),
        [column, direction] if direction.eq_ignore_ascii_case("ASC") => (*column, false),
        [column, direction] if direction.eq_ignore_ascii_case("DESC") => (*column, true),
        _ => return Err("order must contain one identifier with optional ASC or DESC"),
    };
    let column = parse_identifier(column).ok_or("order column is not a safe SQL identifier")?;
    Ok(DbOrder { column, descending })
}

fn parse_identifier(identifier: &str) -> Option<Vec<String>> {
    let segments = identifier.split('.').collect::<Vec<_>>();
    (!segments.is_empty()
        && segments
            .iter()
            .all(|segment| valid_identifier_segment(segment)))
    .then(|| segments.into_iter().map(str::to_string).collect())
}

fn valid_identifier_segment(segment: &str) -> bool {
    let mut bytes = segment.bytes();
    bytes
        .next()
        .is_some_and(|first| first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

pub(super) fn is_filter(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 3
}

pub(super) fn is_db_where(component: &FnComponent) -> bool {
    component.library == "db" && component.kind == 21 && component.db_where.is_some()
}

pub(super) fn is_db_function_component(component: &roxmltree::Node<'_, '_>) -> bool {
    component.attribute("library") == Some("db")
        && (component.attribute("kind") == Some("21") || is_db_scalar_function_component(component))
}

pub(super) fn is_db_scalar_function_component(component: &roxmltree::Node<'_, '_>) -> bool {
    component.attribute("library") == Some("db")
        && component.attribute("kind") == Some("5")
        && matches!(
            component.attribute("name"),
            Some("substitute-null" | "is-null" | "is-not-null")
        )
}

pub(super) fn is_xbrl_measure_component(component: &roxmltree::Node<'_, '_>) -> bool {
    component.attribute("library") == Some("xbrl")
        && component.attribute("kind") == Some("5")
        && matches!(
            component.attribute("name"),
            Some("xbrl-measure-currency" | "xbrl-measure-shares")
        )
}

pub(super) fn is_isbn_converter_component(component: &roxmltree::Node<'_, '_>) -> bool {
    component.attribute("library") == Some("IsbnConverterService")
        && component.attribute("kind") == Some("20")
        && matches!(
            component.attribute("name"),
            Some("convertToISBN13" | "convertToEAN")
        )
}

pub(super) fn read_isbn_converter_component(
    component: &roxmltree::Node<'_, '_>,
) -> Result<FnComponent, &'static str> {
    let mut function = read(component);
    if !function.inputs.is_empty() || !function.outputs.is_empty() {
        return (matches!(function.inputs.as_slice(), [Some(_)])
            && matches!(function.output_pins.as_slice(), [Some(_)]))
        .then_some(function)
        .ok_or("expected one scalar request input and one scalar response output");
    }
    let data = component
        .children()
        .find(|node| node.has_tag_name("data"))
        .ok_or("missing request and response entry trees")?;
    let inputs = data
        .descendants()
        .filter_map(|node| parse_u32(node.attribute("inpkey")))
        .collect::<Vec<_>>();
    let outputs = data
        .descendants()
        .filter_map(|node| parse_u32(node.attribute("outkey")))
        .collect::<Vec<_>>();
    let ([input], [output]) = (inputs.as_slice(), outputs.as_slice()) else {
        return Err("expected one scalar request input and one scalar response output");
    };
    function.inputs = vec![Some(*input)];
    function.outputs = vec![*output];
    function.output_pins = vec![Some(*output)];
    Ok(function)
}

pub(super) fn produces_scalar(component: &FnComponent) -> bool {
    component.name == "constant"
        || component.library == "xpath2"
            && component.kind == 5
            && component.name == "current-dateTime"
        || matches!(
            component.name.as_str(),
            "if-else"
                | "value-map"
                | "position"
                | "mfd-filepath"
                | "main-mfd-filepath"
                | "now"
                | "set-empty"
                | "set-xsi-nil"
                | "auto-number"
        )
        || component.kind == 5
            && component.library == "db"
            && matches!(component.name.as_str(), "is-null" | "is-not-null")
        || component.kind == 5
            && component.library == "xbrl"
            && matches!(
                component.name.as_str(),
                "xbrl-measure-currency" | "xbrl-measure-shares"
            )
        || component.kind == 5 && component.library == "core" && component.name == "not-exists"
        || component.kind == 5 && aggregate_op(&component.name).is_some()
        || map_component_name(component).is_some()
}

pub(super) fn map_component_name(component: &FnComponent) -> Option<&str> {
    if component.name == "format-guid-string" {
        return (component.library == "lang" && component.kind == 5)
            .then_some("format_guid_string");
    }
    if component.name == "random" {
        return (component.kind == 5 && component.library == "lang").then_some("random");
    }
    if component.library == "ferrule"
        && component.kind == 5
        && canonical_function::is_internal(&component.name)
    {
        Some(component.name.as_str())
    } else {
        map_name(&component.name)
    }
}

pub(super) fn is_input(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 6
}

pub(super) fn is_sort(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 30 && component.sort_directions.is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SequenceWindowComponent {
    SkipFirst,
    First,
    From,
    FromTo,
    Last,
}

pub(super) fn sequence_window_component(
    component: &FnComponent,
) -> Option<SequenceWindowComponent> {
    if component.library != "core" || component.kind != 5 {
        return None;
    }
    match component.name.as_str() {
        "skip-first-items" => Some(SequenceWindowComponent::SkipFirst),
        "first-items" => Some(SequenceWindowComponent::First),
        "items-from" => Some(SequenceWindowComponent::From),
        "items-from-to" | "items-from-till" => Some(SequenceWindowComponent::FromTo),
        "last-items" => Some(SequenceWindowComponent::Last),
        _ => None,
    }
}

pub(super) fn is_sequence_window(component: &FnComponent) -> bool {
    sequence_window_component(component).is_some()
}

pub(super) fn is_group_into_blocks(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 5 && component.name == "group-into-blocks"
}

pub(super) fn is_group_starting_with(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 5 && component.name == "group-starting-with"
}

pub(super) fn is_group_adjacent(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 5 && component.name == "group-adjacent"
}

pub(super) fn is_group_ending_with(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 5 && component.name == "group-ending-with"
}

pub(super) fn is_distinct_values(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 5 && component.name == "distinct-values"
}

pub(super) fn is_sequence_producer(component: &FnComponent) -> bool {
    component.kind == 5
        && ((component.library == "core"
            && matches!(
                component.name.as_str(),
                "tokenize" | "tokenize-regexp" | "tokenize-by-length" | "generate-sequence"
            ))
            || (component.library == "ferrule"
                && matches!(
                    component.recursive,
                    Some(RecursiveComponent::Collect { .. })
                )))
}

pub(super) fn is_recursive_construction(component: &FnComponent) -> bool {
    component.library == "ferrule"
        && component.kind == 5
        && matches!(
            component.recursive,
            Some(
                RecursiveComponent::Filter { .. }
                    | RecursiveComponent::PathHierarchy { .. }
                    | RecursiveComponent::AdjacencyTree { .. }
                    | RecursiveComponent::Invalid
            )
        )
}

fn exact_literal_data(component: &roxmltree::Node<'_, '_>) -> bool {
    let data = component
        .children()
        .filter(|node| node.has_tag_name("data"))
        .collect::<Vec<_>>();
    let [data] = data.as_slice() else {
        return false;
    };
    let children = data
        .children()
        .filter(|node| node.is_element())
        .collect::<Vec<_>>();
    let [literal] = children.as_slice() else {
        return false;
    };
    data.attributes().len() == 0
        && data.tag_name().namespace().is_none()
        && data
            .children()
            .all(|node| !node.is_text() || node.text().is_none_or(|text| text.trim().is_empty()))
        && literal.has_tag_name("constant")
        && literal.tag_name().namespace().is_none()
        && literal.attribute("value").is_some()
        && matches!(
            literal.attribute("datatype"),
            Some("string" | "boolean" | "integer" | "int" | "long")
        )
        && literal.attributes().all(|attribute| {
            attribute.namespace().is_none() && matches!(attribute.name(), "value" | "datatype")
        })
        && literal
            .children()
            .all(|node| !node.is_element() && node.text().is_none_or(|text| text.trim().is_empty()))
}

pub(super) fn parse_constant(value: &str, datatype: &str) -> Value {
    match datatype {
        "integer" | "int" | "long" => value.parse().map(Value::Int).unwrap_or(Value::Null),
        "decimal" | "double" | "float" => value.parse().map(Value::Float).unwrap_or(Value::Null),
        "boolean" => match value {
            "true" | "1" => Value::Bool(true),
            "false" | "0" => Value::Bool(false),
            _ => Value::Null,
        },
        _ => Value::String(value.to_string()),
    }
}

fn parse_scalar_type(datatype: &str) -> ScalarType {
    match datatype {
        "integer" | "int" | "long" => ScalarType::Int,
        "decimal" | "double" | "float" | "number" => ScalarType::Float,
        "boolean" => ScalarType::Bool,
        _ => ScalarType::String,
    }
}

fn scalar_type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "decimal",
        ScalarType::Bool => "boolean",
    }
}

pub(super) fn aggregate_op(name: &str) -> Option<AggregateOp> {
    Some(match name {
        "count" => AggregateOp::Count,
        "sum" => AggregateOp::Sum,
        "avg" => AggregateOp::Avg,
        "min" => AggregateOp::Min,
        "max" => AggregateOp::Max,
        "string-join" => AggregateOp::Join,
        "item-at" => AggregateOp::ItemAt,
        _ => return None,
    })
}

pub(super) fn map_name(name: &str) -> Option<&'static str> {
    Some(match name {
        "concat" => "concat",
        "add" => "add",
        "subtract" => "subtract",
        "multiply" => "multiply",
        "divide" => "divide",
        "equal" => "equal",
        "not-equal" => "not_equal",
        "greater" => "greater_than",
        "less" => "less_than",
        "greater-equal" | "greater-or-equal" | "equal-or-greater" => "greater_or_equal",
        "less-equal" | "less-or-equal" | "equal-or-less" => "less_or_equal",
        "logical-and" => "and",
        "logical-or" => "or",
        "logical-not" => "not",
        "string-length" => "length",
        "contains" => "contains",
        "starts-with" => "starts_with",
        "ends-with" => "ends_with",
        "matches" => "matches",
        "replace" => "replace",
        "upper-case" | "uppercase" => "upper",
        "lower-case" | "lowercase" => "lower",
        "string" => "string",
        "number" => "to_number",
        "numeric" => "is_numeric",
        "boolean" => "boolean",
        "positive" => "positive",
        "floor" => "floor",
        "create-guid" => "create_guid",
        "random" => "random",
        "format-guid-string" => "format_guid_string",
        "format-number" => "format_number",
        "format-date" => "format_date",
        "format-dateTime" => "format_datetime",
        "format-time" => "format_time",
        "trim" => "trim",
        "left" => "left",
        "right" => "right",
        "left-trim" => "left_trim",
        "right-trim" => "right_trim",
        "pad-string-left" => "pad_string_left",
        "pad-string-right" => "pad_string_right",
        "substring" => "substring",
        "substring-before" => "substring_before",
        "substring-after" => "substring_after",
        "normalize-space" => "normalize_space",
        "empty" => "is_empty",
        "get-folder" => "get_folder",
        "remove-folder" => "remove_folder",
        "resolve-filepath" => "resolve_filepath",
        "is-xsi-nil" => "is_xml_nil",
        "substitute-missing-with-xsi-nil" => "substitute_missing_with_xml_nil",
        "exists" => "exists",
        "round" | "round-precision" => "round",
        "date-from-datetime" => "date_from_datetime",
        "year-from-datetime" => "year_from_datetime",
        "month-from-datetime" => "month_from_datetime",
        "day-from-datetime" => "day_from_datetime",
        "weekday" => "weekday",
        "hour-from-datetime" | "hours-from-datetime" => "hours_from_datetime",
        "minute-from-datetime" | "minutes-from-datetime" => "minutes_from_datetime",
        "time-from-datetime" => "time_from_datetime",
        "datetime-from-date-and-time" => "datetime_from_date_and_time",
        "datetime-from-parts" => "datetime_from_parts",
        "duration-from-parts" => "duration_from_parts",
        "datetime-add" => "datetime_add",
        "parse-date" => "parse_date",
        "parse-dateTime" => "parse_datetime",
        "parse-time" => "parse_time",
        "edifact-to-datetime" => "edifact_to_datetime",
        "substitute-missing" | "substitute-null" => "substitute_missing",
        "get-fileext" => "get_fileext",
        "convertToISBN13" | "convertToEAN" => "isbn10_to_isbn13",
        "sleep" => "delay_passthrough",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use ir::{ScalarType, Value};

    use super::{
        VALUE_MAP_INPUT_MODE_UNSUPPORTED, is_db_function_component, is_isbn_converter_component,
        is_xbrl_measure_component, map_component_name, map_name, produces_scalar, read,
        read_isbn_converter_component, unsupported_value_map_input_mode,
        value_map_input_mode_warning, warn_unsupported_value_map_input_modes,
    };

    #[test]
    fn scalar_names_use_canonical_ir_spelling() {
        assert_eq!(map_name("string"), Some("string"));
        assert_eq!(map_name("numeric"), Some("is_numeric"));
        assert_eq!(map_name("format-number"), Some("format_number"));
        assert_eq!(map_name("boolean"), Some("boolean"));
        assert_eq!(map_name("positive"), Some("positive"));
        assert_eq!(map_name("floor"), Some("floor"));
        assert_eq!(map_name("create-guid"), Some("create_guid"));
        assert_eq!(map_name("format-guid-string"), Some("format_guid_string"));
        assert_eq!(map_name("format-date"), Some("format_date"));
        assert_eq!(map_name("format-dateTime"), Some("format_datetime"));
        assert_eq!(map_name("format-time"), Some("format_time"));
        assert_eq!(map_name("normalize-space"), Some("normalize_space"));
        assert_eq!(map_name("ends-with"), Some("ends_with"));
        assert_eq!(map_name("matches"), Some("matches"));
        assert_eq!(map_name("replace"), Some("replace"));
        assert_eq!(map_name("left"), Some("left"));
        assert_eq!(map_name("right"), Some("right"));
        assert_eq!(map_name("empty"), Some("is_empty"));
        assert_eq!(map_name("time-from-datetime"), Some("time_from_datetime"));
        assert_eq!(map_name("year-from-datetime"), Some("year_from_datetime"));
        assert_eq!(map_name("month-from-datetime"), Some("month_from_datetime"));
        assert_eq!(map_name("day-from-datetime"), Some("day_from_datetime"));
        assert_eq!(map_name("weekday"), Some("weekday"));
        assert_eq!(map_name("hours-from-datetime"), Some("hours_from_datetime"));
        assert_eq!(map_name("hour-from-datetime"), Some("hours_from_datetime"));
        assert_eq!(
            map_name("minutes-from-datetime"),
            Some("minutes_from_datetime")
        );
        assert_eq!(
            map_name("minute-from-datetime"),
            Some("minutes_from_datetime")
        );
        assert_eq!(
            map_name("datetime-from-date-and-time"),
            Some("datetime_from_date_and_time")
        );
        assert_eq!(map_name("datetime-from-parts"), Some("datetime_from_parts"));
        assert_eq!(map_name("duration-from-parts"), Some("duration_from_parts"));
        assert_eq!(map_name("datetime-add"), Some("datetime_add"));
        assert_eq!(map_name("parse-date"), Some("parse_date"));
        assert_eq!(map_name("parse-dateTime"), Some("parse_datetime"));
        assert_eq!(map_name("edifact-to-datetime"), Some("edifact_to_datetime"));
        assert_eq!(map_name("substitute-missing"), Some("substitute_missing"));
        assert_eq!(map_name("substitute-null"), Some("substitute_missing"));
        assert_eq!(
            map_name("substitute-missing-with-xsi-nil"),
            Some("substitute_missing_with_xml_nil")
        );
        assert_eq!(map_name("get-fileext"), Some("get_fileext"));
        assert_eq!(map_name("convertToISBN13"), Some("isbn10_to_isbn13"));
        assert_eq!(map_name("convertToEAN"), Some("isbn10_to_isbn13"));
        assert_eq!(map_name("sleep"), Some("delay_passthrough"));
    }

    #[test]
    fn guid_formatting_requires_the_standard_lang_scalar_component() {
        for (library, kind, expected) in [
            ("lang", "5", Some("format_guid_string")),
            ("core", "5", None),
            ("vendor", "5", None),
            ("lang", "6", None),
            ("lang", "19", None),
        ] {
            let text = format!(
                r#"<component library="{library}" name="format-guid-string" kind="{kind}"/>"#
            );
            let document = roxmltree::Document::parse(&text).expect("authored component XML");
            let component = read(&document.root_element());
            assert_eq!(map_component_name(&component), expected);
        }
    }

    #[test]
    fn current_datetime_is_exactly_the_xpath2_scalar_component() {
        for (library, name, kind, accepted) in [
            ("xpath2", "current-dateTime", "5", true),
            ("xpath2", "current-dateTime", "4", false),
            ("xpath2", "current-datetime", "5", false),
            ("vendor", "current-dateTime", "5", false),
        ] {
            let xml = format!(r#"<component library="{library}" name="{name}" kind="{kind}"/>"#);
            let document = roxmltree::Document::parse(&xml).unwrap();
            assert_eq!(produces_scalar(&read(&document.root_element())), accepted);
        }
    }

    #[test]
    fn only_explicit_ferrule_components_map_internal_names() {
        let ferrule = roxmltree::Document::parse(
            r#"<component library="ferrule" name="to_number" kind="5"/>"#,
        )
        .unwrap();
        let ferrule = read(&ferrule.root_element());
        assert_eq!(map_component_name(&ferrule), Some("to_number"));
        assert!(produces_scalar(&ferrule));

        let vendor =
            roxmltree::Document::parse(r#"<component library="core" name="to_number" kind="5"/>"#)
                .unwrap();
        let vendor = read(&vendor.root_element());
        assert_eq!(map_component_name(&vendor), None);
        assert!(!produces_scalar(&vendor));
    }

    #[test]
    fn specialized_scalar_component_classifiers_are_exact() -> Result<(), Box<dyn std::error::Error>>
    {
        for (library, name, accepted) in [
            ("db", "is-null", true),
            ("db", "is-not-null", true),
            ("db", "substitute-null", true),
            ("db", "other", false),
            ("xbrl", "xbrl-measure-currency", true),
            ("xbrl", "xbrl-measure-shares", true),
            ("xbrl", "other", false),
        ] {
            let xml = format!(
                r#"<component library="{library}" name="{name}" kind="5"><targets><datapoint pos="0" key="1"/></targets></component>"#
            );
            let document = roxmltree::Document::parse(&xml)?;
            let node = document.root_element();
            let classified = if library == "db" {
                is_db_function_component(&node)
            } else {
                is_xbrl_measure_component(&node)
            };
            assert_eq!(classified, accepted, "{library}/{name}");
            if accepted && name != "substitute-null" {
                assert!(produces_scalar(&read(&node)), "{library}/{name}");
            }
        }

        let wrong_db_library =
            roxmltree::Document::parse(r#"<component library="core" name="is-null" kind="5"/>"#)?;
        assert!(!is_db_function_component(&wrong_db_library.root_element()));
        let wrong_xbrl_library = roxmltree::Document::parse(
            r#"<component library="core" name="xbrl-measure-shares" kind="5"/>"#,
        )?;
        assert!(!is_xbrl_measure_component(
            &wrong_xbrl_library.root_element()
        ));

        for (name, kind, accepted) in [
            ("convertToISBN13", "20", true),
            ("convertToEAN", "20", true),
            ("convertToISBN10", "20", false),
            ("convertToISBN13", "5", false),
        ] {
            let xml = format!(
                r#"<component library="IsbnConverterService" name="{name}" kind="{kind}"/>"#
            );
            let document = roxmltree::Document::parse(&xml)?;
            assert_eq!(
                is_isbn_converter_component(&document.root_element()),
                accepted,
                "IsbnConverterService/{name}/{kind}"
            );
        }
        let wrong_isbn_library = roxmltree::Document::parse(
            r#"<component library="other" name="convertToISBN13" kind="20"/>"#,
        )?;
        assert!(!is_isbn_converter_component(
            &wrong_isbn_library.root_element()
        ));
        let entry_pins = roxmltree::Document::parse(
            r#"<component library="IsbnConverterService" name="convertToEAN" kind="20"><data><root><entry inpkey="4"/></root><root><entry outkey="7"/></root></data></component>"#,
        )?;
        let converted = read_isbn_converter_component(&entry_pins.root_element())?;
        assert_eq!(converted.inputs, [Some(4)]);
        assert_eq!(converted.outputs, [7]);
        Ok(())
    }

    #[test]
    fn datapoint_positions_preserve_keyless_primary_outputs() {
        let document = roxmltree::Document::parse(
            r#"<component name="filter" library="core" kind="3">
                <sources><datapoint pos="1" key="2"/><datapoint pos="0" key="1"/></sources>
                <targets><datapoint pos="1" key="9"/><datapoint pos="0"/></targets>
            </component>"#,
        )
        .unwrap();
        let component = read(&document.root_element());
        assert_eq!(component.inputs, vec![Some(1), Some(2)]);
        assert_eq!(component.output_pins, vec![None, Some(9)]);
        assert_eq!(component.outputs, vec![9]);
    }

    #[test]
    fn value_map_data_preserves_declared_scalar_types() {
        let document = roxmltree::Document::parse(
            r#"<component name="value-map" library="core" kind="23">
                <sources><datapoint pos="0" key="1"/></sources>
                <targets><datapoint pos="0" key="2"/></targets>
                <data><valuemap defaultValueMode="custom">
                    <valuemapTable><entry from="7" to="1"/></valuemapTable>
                    <input name="input" type="integer"/>
                    <result name="result" type="boolean" defaultValue="0"/>
                </valuemap></data>
            </component>"#,
        )
        .unwrap();

        let value_map = read(&document.root_element()).valuemap.unwrap();
        assert_eq!(value_map.input_type, Some(ScalarType::Int));
        assert_eq!(value_map.table, vec![(Value::Int(7), Value::Bool(true))]);
        assert_eq!(value_map.default, Some(Value::Bool(false)));
    }

    #[test]
    fn value_map_accepts_enabled_default_value_flag() {
        let document = roxmltree::Document::parse(
            r#"<component name="value-map" library="core" kind="23">
                <data><valuemap enableDefaultValue="1">
                    <valuemapTable><entry from="Admin" to="true"/></valuemapTable>
                    <input name="input" type="string"/>
                    <result name="result" type="boolean" defaultValue="false"/>
                </valuemap></data>
            </component>"#,
        )
        .unwrap();

        let value_map = read(&document.root_element()).valuemap.unwrap();
        assert_eq!(value_map.default, Some(Value::Bool(false)));
    }

    #[test]
    fn value_map_input_mode_retains_refusal_without_enabling_a_stale_default() {
        for legacy_flag in ["", " enableDefaultValue=\"0\"", " enableDefaultValue=\"1\""] {
            let xml = format!(
                r#"<component name="value-map" library="core" kind="23">
                    <sources><datapoint pos="0" key="1"/></sources>
                    <targets><datapoint pos="0" key="2"/></targets>
                    <data><valuemap defaultValueMode="input"{legacy_flag}>
                        <valuemapTable><entry from="7" to="8"/></valuemapTable>
                        <input name="input" type="integer"/>
                        <result name="result" type="integer" defaultValue="9"/>
                    </valuemap></data>
                </component>"#
            );
            let document = roxmltree::Document::parse(&xml).unwrap();
            let value_map = read(&document.root_element()).valuemap.unwrap();
            assert!(value_map.unsupported_input_mode);
            assert_eq!(
                value_map.unsupported_reason(),
                Some(VALUE_MAP_INPUT_MODE_UNSUPPORTED)
            );
            assert_eq!(value_map.input_type, Some(ScalarType::Int));
            assert_eq!(value_map.table, [(Value::Int(7), Value::Int(8))]);
            assert_eq!(value_map.default, None);
            assert!(value_map.total_string.is_none());
        }
    }

    #[test]
    fn value_map_input_mode_does_not_widen_the_total_string_proof() {
        let document = roxmltree::Document::parse(
            r#"<component name="renamed" library="core" kind="23">
                <sources><datapoint pos="0" key="1"/></sources>
                <targets><datapoint pos="0" key="2"/></targets>
                <data><valuemap defaultValueMode="input">
                    <valuemapTable><entry from="a" to="b"/></valuemapTable>
                    <input name="input" type="string"/>
                    <result name="result" type="string" defaultValue="stale"/>
                </valuemap></data>
            </component>"#,
        )
        .unwrap();
        let value_map = read(&document.root_element()).valuemap.unwrap();
        assert!(value_map.unsupported_input_mode);
        assert!(value_map.total_string.is_none());
    }

    #[test]
    fn value_map_input_mode_uses_the_lowerers_parsed_kind() {
        for kind in ["23", "023", "+23"] {
            let xml = format!(
                r#"<component name="value-map" library="core" kind="{kind}">
                    <data><valuemap defaultValueMode="input" enableDefaultValue="1">
                        <valuemapTable><entry from="A" to="B"/></valuemapTable>
                        <input name="input" type="string"/>
                        <result name="result" type="string" defaultValue="stale"/>
                    </valuemap></data>
                </component>"#
            );
            let document = roxmltree::Document::parse(&xml).unwrap();
            let component = document.root_element();
            let function = read(&component);
            assert_eq!(function.kind, 23, "{kind}");
            assert_eq!(
                unsupported_value_map_input_mode(&component),
                Some(VALUE_MAP_INPUT_MODE_UNSUPPORTED),
                "{kind}"
            );
            let value_map = function.valuemap.unwrap();
            assert_eq!(
                value_map.unsupported_reason(),
                Some(VALUE_MAP_INPUT_MODE_UNSUPPORTED),
                "{kind}"
            );
            assert_eq!(value_map.default, None, "{kind}");
        }
    }

    #[test]
    fn value_map_other_default_modes_keep_existing_compatibility() {
        for (attributes, expected) in [
            ("", None),
            (r#"defaultValueMode="none""#, None),
            (r#"defaultValueMode="unknown""#, None),
            (r#"defaultValueMode="custom""#, Some(Value::Null)),
            (r#"enableDefaultValue="1""#, Some(Value::Null)),
            (
                r#"defaultValueMode="custom" enableDefaultValue="0""#,
                Some(Value::Null),
            ),
            (
                r#"defaultValueMode="unknown" enableDefaultValue="1""#,
                Some(Value::Null),
            ),
        ] {
            let xml = format!(
                r#"<component name="value-map" library="core" kind="23">
                    <data><valuemap {attributes}>
                        <valuemapTable/>
                        <input name="input" type="string"/>
                        <result name="result" type="integer" defaultValue="not-an-integer"/>
                    </valuemap></data>
                </component>"#
            );
            let document = roxmltree::Document::parse(&xml).unwrap();
            let value_map = read(&document.root_element()).valuemap.unwrap();
            assert_eq!(value_map.unsupported_reason(), None, "{attributes}");
            assert_eq!(value_map.default, expected, "{attributes}");
        }
    }

    #[test]
    fn value_map_input_mode_census_keeps_body_and_definition_ownership() {
        let document = roxmltree::Document::parse(
            r#"<mapping>
                <component name="main"><structure><children>
                    <component name="value-map" library="core" kind="23">
                        <data><valuemap defaultValueMode="input"/></data>
                    </component>
                </children></structure></component>
                <component name="unused" library="example"><structure><children>
                    <component name="value-map" library="core" kind="23">
                        <data><valuemap defaultValueMode="input"/></data>
                    </component>
                </children></structure></component>
            </mapping>"#,
        )
        .unwrap();
        let mut warnings = Vec::new();
        warn_unsupported_value_map_input_modes(&document.root_element(), &mut warnings);
        assert_eq!(
            warnings,
            [
                value_map_input_mode_warning("core", "value-map"),
                format!(
                    "user-defined function `unused` (example) is unsupported: {VALUE_MAP_INPUT_MODE_UNSUPPORTED}"
                ),
            ]
        );
    }

    #[test]
    fn value_map_input_mode_census_ignores_preview_and_foreign_metadata() {
        let document = roxmltree::Document::parse(
            r#"<mapping xmlns:foreign="urn:foreign">
                <component name="main">
                    <properties><preview><structure><children>
                        <component name="value-map" library="core" kind="23">
                            <data><valuemap defaultValueMode="input"/></data>
                        </component>
                    </children></structure></preview></properties>
                    <structure><children>
                        <component name="constant" library="core" kind="2">
                            <data><valuemap defaultValueMode="input"/></data>
                        </component>
                        <component name="value-map" library="foreign" kind="23">
                            <data><valuemap defaultValueMode="input"/></data>
                        </component>
                        <component name="value-map" library="core" kind="5">
                            <data><valuemap defaultValueMode="input"/></data>
                        </component>
                        <component name="value-map" library="core" kind="23">
                            <data><preview><valuemap defaultValueMode="input"/></preview></data>
                        </component>
                        <foreign:component name="value-map" library="core" kind="23">
                            <data><valuemap defaultValueMode="input"/></data>
                        </foreign:component>
                        <component name="value-map" library="core" kind="23">
                            <data><foreign:valuemap defaultValueMode="input"/></data>
                        </component>
                        <component name="value-map" library="core" kind="23">
                            <data><valuemap foreign:defaultValueMode="input"/></data>
                        </component>
                    </children></structure>
                </component>
            </mapping>"#,
        )
        .unwrap();
        let mut warnings = Vec::new();
        warn_unsupported_value_map_input_modes(&document.root_element(), &mut warnings);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn scalar_constant_pin_proof_keeps_raw_metadata_refusals() {
        let original = r#"<component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="4"/></targets><data><constant datatype="string" value="created"/></data></component>"#;
        for valid in [
            original.to_string(),
            original.replace("<targets>", "<sources/><targets>"),
            original.replace(" pos=\"0\"", ""),
        ] {
            let document = roxmltree::Document::parse(&valid).unwrap();
            let component = read(&document.root_element());
            assert!(component.bounded_scalar_pins_valid, "{valid}");
            assert!(component.inputs.is_empty());
            assert_eq!(component.outputs, [4]);
            assert_eq!(component.output_pins, [Some(4)]);
        }
        for invalid in [
            original.replace("pos=\"0\"", "pos=\"bad\""),
            original.replace("pos=\"0\"", "pos=\"64\""),
            original.replace("pos=\"0\"", "pos=\"1\""),
            original.replace("key=\"4\"", "key=\"bad\""),
            original.replace("key=\"4\"", "key=\"4294967296\""),
            original.replace(" key=\"4\"", ""),
            original.replace("<datapoint pos=\"0\" key=\"4\"/>", ""),
            original.replace("<targets>", "<sources/><sources/><targets>"),
            original.replace(
                "<targets>",
                "<sources><datapoint pos=\"0\" key=\"1\"/></sources><targets>",
            ),
            original.replace("<targets>", "<sources><other/></sources><targets>"),
            original.replace("</targets>", "</targets><targets/>"),
            original.replace("<datapoint", "<other"),
            original.replace("kind=\"2\"", "kind=\"4\""),
            original.replace("library=\"core\"", "library=\"foreign\""),
            original.replace("name=\"constant\"", "name=\"other\""),
        ] {
            let document = roxmltree::Document::parse(&invalid).unwrap();
            assert!(
                !read(&document.root_element()).bounded_scalar_pins_valid,
                "{invalid}"
            );
        }

        // Duplicate positions collapse to one output in the general reader.
        let duplicate = original.replace(
            "<datapoint pos=\"0\" key=\"4\"/>",
            "<datapoint pos=\"0\" key=\"4\"/><datapoint pos=\"0\" key=\"5\"/>",
        );
        let document = roxmltree::Document::parse(&duplicate).unwrap();
        let component = read(&document.root_element());
        assert!(component.inputs.is_empty());
        assert_eq!(component.outputs, [4]);
        assert_eq!(component.output_pins, [Some(4)]);
        assert!(!component.bounded_scalar_pins_valid);
    }

    #[test]
    fn raw_output_footprint_retains_keys_discarded_by_pin_normalization() {
        for targets in [
            r#"<targets><datapoint pos="0" key="4"/><datapoint pos="0" key="5"/></targets>"#,
            r#"<targets><datapoint pos="0" key="4"/></targets><targets><datapoint pos="0" key="5"/></targets>"#,
        ] {
            let text =
                format!(r#"<component name="other" library="core" kind="4">{targets}</component>"#);
            let document = roxmltree::Document::parse(&text).unwrap();
            let component = read(&document.root_element());
            assert_eq!(component.outputs, [4]);
            assert_eq!(component.output_pins, [Some(4)]);
            assert_eq!(component.declared_output_keys, BTreeSet::from([4, 5]));
            assert!(!component.raw_single_output_valid);
        }
    }

    #[test]
    fn raw_single_output_proof_is_independent_of_scalar_input_admission() {
        let original = r#"<component name="if-else" library="core" kind="4"><sources><datapoint pos="0" key="1"/><datapoint pos="1" key="2"/><datapoint pos="2"/></sources><targets><datapoint pos="0" key="4"/></targets></component>"#;
        let document = roxmltree::Document::parse(original).unwrap();
        let component = read(&document.root_element());
        assert!(component.raw_single_output_valid);
        assert!(!component.bounded_scalar_pins_valid);
        assert_eq!(component.declared_output_keys, BTreeSet::from([4]));
        for malformed in [
            original.replace("pos=\"0\" key=\"4\"", "pos=\"bad\" key=\"4\""),
            original.replace("pos=\"0\" key=\"4\"", "pos=\"64\" key=\"4\""),
            original.replace(" key=\"4\"", ""),
            original.replace("key=\"4\"", "key=\"bad\""),
            original.replace("</targets>", "</targets><targets/>"),
            original.replace("<datapoint pos=\"0\" key=\"4\"/>", "<other/>"),
            original.replace(
                "<datapoint pos=\"0\" key=\"4\"/>",
                "<datapoint pos=\"0\" key=\"4\"/><datapoint pos=\"0\" key=\"4\"/>",
            ),
        ] {
            let document = roxmltree::Document::parse(&malformed).unwrap();
            assert!(
                !read(&document.root_element()).raw_single_output_valid,
                "{malformed}"
            );
        }
    }

    #[test]
    fn scalar_conditional_pin_proof_keeps_raw_metadata_refusals() {
        let original = r#"<component name="if-else" library="core" kind="4"><sources><datapoint pos="0" key="1"/><datapoint pos="1" key="2"/><datapoint pos="2" key="3"/></sources><targets><datapoint pos="0" key="4"/></targets></component>"#;
        for valid in [
            original.to_string(),
            original.replace(" pos=\"0\"", "").replace(" pos=\"1\"", "").replace(" pos=\"2\"", ""),
            original.replace(
                "<datapoint pos=\"0\" key=\"1\"/><datapoint pos=\"1\" key=\"2\"/><datapoint pos=\"2\" key=\"3\"/>",
                "<datapoint pos=\"2\" key=\"3\"/><datapoint pos=\"0\" key=\"1\"/><datapoint pos=\"1\" key=\"2\"/>",
            ),
        ] {
            let document = roxmltree::Document::parse(&valid).unwrap();
            let component = read(&document.root_element());
            assert!(component.bounded_scalar_pins_valid, "{valid}");
            assert_eq!(component.inputs, [Some(1), Some(2), Some(3)]);
            assert_eq!(component.outputs, [4]);
            assert_eq!(component.output_pins, [Some(4)]);
        }
        for invalid in [
            original.replace("pos=\"2\"", "pos=\"bad\""),
            original.replace("pos=\"2\"", "pos=\"64\""),
            original.replace("pos=\"2\"", "pos=\"1\""),
            original.replace("key=\"3\"", "key=\"bad\""),
            original.replace("key=\"4\"", "key=\"bad\""),
            original.replace(" key=\"3\"", ""),
            original.replace(" key=\"4\"", ""),
            original.replace("<datapoint pos=\"2\" key=\"3\"/>", ""),
            original.replace("<datapoint pos=\"0\" key=\"4\"/>", ""),
            original.replace(
                "<datapoint pos=\"2\" key=\"3\"/>",
                "<datapoint pos=\"2\" key=\"3\"/><datapoint pos=\"2\" key=\"5\"/>",
            ),
            original.replace(
                "<datapoint pos=\"0\" key=\"4\"/>",
                "<datapoint pos=\"0\" key=\"4\"/><datapoint pos=\"0\" key=\"5\"/>",
            ),
            original.replace("key=\"3\"", "key=\"2\""),
            original.replace("key=\"4\"", "key=\"1\""),
            original.replace("</sources>", "</sources><sources/>"),
            original.replace("</targets>", "</targets><targets/>"),
            original.replace("name=\"if-else\"", "name=\"other\""),
            original.replace("kind=\"4\"", "kind=\"5\""),
            original.replace("library=\"core\"", "library=\"foreign\""),
        ] {
            let document = roxmltree::Document::parse(&invalid).unwrap();
            assert!(
                !read(&document.root_element()).bounded_scalar_pins_valid,
                "{invalid}"
            );
        }

        // General pin normalization preserves a valid-looking three-input shape.
        for malformed in [
            original.replace("pos=\"2\"", "pos=\"bad\""),
            original.replace(
                "<datapoint pos=\"2\" key=\"3\"/>",
                "<datapoint pos=\"2\" key=\"3\"/><datapoint pos=\"2\" key=\"5\"/>",
            ),
        ] {
            let document = roxmltree::Document::parse(&malformed).unwrap();
            let component = read(&document.root_element());
            assert_eq!(component.inputs, [Some(1), Some(2), Some(3)]);
            assert_eq!(component.outputs, [4]);
            assert_eq!(component.output_pins, [Some(4)]);
            assert!(!component.bounded_scalar_pins_valid, "{malformed}");
        }
    }
}
