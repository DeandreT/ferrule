//! Physical scalar count/filter lowering. This deliberately does not unwrap a
//! selection into a collection-only Count or change other aggregate operations.
//! Predicates read only exact literals and the non-nil counted field, whose
//! occurrence is already protected by this adapter's value-presence boundary.

use std::collections::BTreeSet;

use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{AggregateOp, Node, NodeId};

use super::super::constructed_variable_driver::declared_producer_count;
use super::super::function::{FnComponent, is_filter, map_component_name, parse_constant};
use super::super::graph::GraphBuilder;
use super::super::iteration::split_at_innermost_repeating;
use super::super::schema::{ComponentFormat, schema_node_at};
use super::super::source::SourcePath;

const MAX_PREDICATE_DEPTH: usize = 32;
const MAX_PREDICATE_NODES: usize = 128;

/// Dedicated raw proof for this adapter. The ordinary function pin reader is
/// intentionally repair-oriented and can normalize malformed positions away.
pub(in crate::import) fn raw_signature_valid(component: &roxmltree::Node<'_, '_>) -> bool {
    let arity = match (
        component.attribute("library"),
        component.attribute("kind"),
        component.attribute("name"),
    ) {
        (Some("core"), Some("3"), Some("filter")) => 2,
        (Some("core"), Some("5"), Some("equal" | "not-equal" | "logical-and" | "logical-or")) => 2,
        (Some("core"), Some("5"), Some("logical-not")) => 1,
        (Some("core" | "lang"), Some("5"), Some("normalize-space" | "trim" | "empty")) => 1,
        _ => return false,
    };
    if component.tag_name().namespace().is_some()
        || component.attributes().any(|attribute| {
            attribute.namespace().is_some()
                || !matches!(attribute.name(), "name" | "library" | "kind" | "uid")
        })
    {
        return false;
    }
    let mut seen = BTreeSet::new();
    for child in component.children() {
        if child.is_text() && child.text().is_some_and(|text| !text.trim().is_empty()) {
            return false;
        }
        if !child.is_element() {
            continue;
        }
        let name = child.tag_name().name();
        if child.tag_name().namespace().is_some()
            || !seen.insert(name)
            || !matches!(name, "sources" | "targets" | "view" | "properties")
            || name == "properties"
                && (child.attributes().len() != 0
                    || child.children().any(|node| {
                        node.is_element()
                            || node.is_text()
                                && node.text().is_some_and(|text| !text.trim().is_empty())
                    }))
        {
            return false;
        }
    }
    let pins = |tag: &str| -> Option<Vec<u32>> {
        let container = component.children().find(|node| node.has_tag_name(tag))?;
        if container.attributes().len() != 0
            || container.children().any(|node| {
                node.is_text() && node.text().is_some_and(|text| !text.trim().is_empty())
            })
        {
            return None;
        }
        container
            .children()
            .filter(roxmltree::Node::is_element)
            .enumerate()
            .map(|(position, point)| {
                if !point.has_tag_name("datapoint")
                    || point.tag_name().namespace().is_some()
                    || point.attributes().any(|attribute| {
                        attribute.namespace().is_some()
                            || !matches!(attribute.name(), "pos" | "key")
                    })
                    || point.children().any(|node| {
                        node.is_element()
                            || node.is_text()
                                && node.text().is_some_and(|text| !text.trim().is_empty())
                    })
                    || point
                        .attribute("pos")
                        .is_some_and(|text| text.parse::<usize>().ok() != Some(position))
                {
                    return None;
                }
                point.attribute("key")?.parse().ok()
            })
            .collect()
    };
    let (Some(inputs), Some(outputs)) = (pins("sources"), pins("targets")) else {
        return false;
    };
    if inputs.len() != arity
        || outputs.is_empty()
        || outputs.len()
            > if component.attribute("kind") == Some("3") {
                2
            } else {
                1
            }
    {
        return false;
    }
    let mut keys = BTreeSet::new();
    if inputs.iter().chain(&outputs).any(|key| !keys.insert(*key)) {
        return false;
    }
    let Some(siblings) = component.parent() else {
        return false;
    };
    let Some(structure) = siblings
        .parent()
        .filter(|node| node.has_tag_name("structure"))
    else {
        return false;
    };
    let owners = || {
        siblings
            .children()
            .filter(|node| node.has_tag_name("component"))
    };
    let count = |key| super::super::constructed_parser_scalar::declared_output_count(owners(), key);
    let edges = super::super::graph::read_edge_inputs(&structure, structure.parent().as_ref());
    outputs.iter().all(|output| count(*output) == 1)
        && inputs
            .iter()
            .all(|input| edges.feeds.get(input).is_some_and(|feed| count(*feed) == 1))
}

/// Proof before allocation keeps malformed or unowned dependencies from becoming
/// cached Null expressions. Each field retains the exact physical source frame.
enum Predicate {
    Field {
        frame: Vec<String>,
        path: Vec<String>,
        ty: ScalarType,
    },
    Literal(Value),
    Call {
        name: &'static str,
        args: Vec<Predicate>,
        ty: ScalarType,
    },
}

impl Predicate {
    fn ty(&self) -> ScalarType {
        match self {
            Self::Field { ty, .. } | Self::Call { ty, .. } => *ty,
            Self::Literal(Value::String(_)) => ScalarType::String,
            Self::Literal(Value::Bool(_)) => ScalarType::Bool,
            _ => unreachable!("the proof admits only String and Bool literals"),
        }
    }

    fn lower(self, builder: &mut GraphBuilder<'_>) -> NodeId {
        match self {
            Self::Field { frame, path, .. } => builder.alloc(Node::SourceField {
                frame: Some(frame),
                path,
            }),
            Self::Literal(value) => builder.alloc(Node::Const { value }),
            Self::Call { name, args, .. } => {
                let args = args.into_iter().map(|arg| arg.lower(builder)).collect();
                builder.alloc(Node::Call {
                    function: name.into(),
                    args,
                })
            }
        }
    }
}

impl GraphBuilder<'_> {
    pub(super) fn physical_filtered_count(
        &mut self,
        count_index: usize,
        sequence: u32,
        active_anchor: Option<&[String]>,
    ) -> Result<Option<Node>, String> {
        let traced = self.resolve_iteration_feed(sequence);
        let direct = self
            .fn_by_output
            .get(&sequence)
            .copied()
            .filter(|index| is_filter(&self.fn_components[*index]));
        if !traced.has_filter && direct.is_none() {
            return Ok(None);
        }
        let refused = |detail: &str| format!("physical filtered count {detail}");
        let filter_index = direct.ok_or_else(|| {
            refused("requires a direct filter output without additional sequence controls")
        })?;
        let count = &self.fn_components[count_index];
        let filter = &self.fn_components[filter_index];
        if !count.raw_count_signature_valid
            || !filter.raw_filtered_count_pins_valid
            || declared_producer_count(sequence, self) != 1
            || !matches!(filter.inputs.as_slice(), [Some(_), Some(_)])
        {
            return Err(refused("has malformed or ambiguous count/filter pins"));
        }
        let output_position = filter
            .output_pins
            .iter()
            .position(|output| *output == Some(sequence))
            .filter(|position| *position < 2)
            .ok_or_else(|| refused("requires the inclusion or exclusion output"))?;
        let values = self
            .input_feed(filter_index, 0)
            .ok_or_else(|| refused("has an unresolved value feed"))?;
        let predicate = self
            .input_feed(filter_index, 1)
            .ok_or_else(|| refused("has an unresolved predicate feed"))?;
        let source_path = self
            .source_abs_path(values)
            .ok_or_else(|| refused("requires a direct physical scalar value feed"))?;
        let source = self
            .sources
            .get(source_path.source)
            .ok_or_else(|| refused("has no physical source"))?;
        if !matches!(source.format, ComponentFormat::Xml | ComponentFormat::Edi)
            || !source.is_source
            || source.is_variable
            || source.is_pass_through
            || source.compute_when_key.is_some()
            || source.dynamic_json.is_some()
            || !source.input_keys.is_empty()
            || source.has_target_node_controls
            || !source.unambiguous_declared_output(values)
            || declared_producer_count(values, self) != 1
            || self.has_source_node_functions(values)
        {
            return Err(refused("requires one ordinary unambiguous physical source"));
        }
        if source.format == ComponentFormat::Edi
            && !source
                .port_declarations
                .as_ref()
                .is_some_and(|facts| facts.complete_x12_config())
        {
            return Err(refused("requires a complete physical X12 configuration"));
        }
        let leaf = schema_node_at(&source.schema, &source_path.path)
            .filter(|node| {
                matches!(
                    node.kind,
                    SchemaKind::Scalar {
                        ty: ScalarType::String | ScalarType::Bool
                    }
                )
            })
            .ok_or_else(|| refused("requires a physical String or Bool scalar"))?;
        let (collection_abs, value) =
            split_at_innermost_repeating(&source.schema, &source_path.path);
        if collection_abs.is_empty() {
            return Err(refused("requires an explicit repeated physical owner"));
        }
        let repeating_scalar = leaf.repeating;
        if source.format == ComponentFormat::Edi && repeating_scalar {
            // Empty X12 repeats can materialize Null items. The XML occurrence
            // proof below does not establish their scalar sequence semantics.
            return Err(refused("does not admit repeating X12 scalar values"));
        }
        let owner_path = collection_abs.clone();
        if !plain_group_ancestry(&source.schema, &source_path.path) {
            return Err(refused("has an unproved nullable or alternative owner"));
        }
        let mut budget = 0;
        let predicate = prove_predicate(
            self,
            predicate,
            &source_path,
            &owner_path,
            0,
            &mut BTreeSet::new(),
            &mut budget,
        )
        .filter(|predicate| predicate.ty() == ScalarType::Bool)
        .ok_or_else(|| {
            refused(
                "predicate must be a bounded Bool expression of exact literals or the guarded non-nil counted field; independent or nil-reading operands are not admitted",
            )
        })?;
        let absolute_owner = self.context_path(&SourcePath {
            source: source_path.source,
            path: owner_path,
        });
        // No global framing/cache is changed. The collection is shortened only
        // by a receiving frame that is actually active at this binding.
        let collection = match active_anchor {
            Some(anchor)
                if source_path.source > 0
                    && !anchor.starts_with(&self.context_path(&SourcePath {
                        source: source_path.source,
                        path: Vec::new(),
                    })) =>
            {
                Some(absolute_owner.clone())
            }
            Some(anchor) => {
                self.collection_path_at_anchor(source_path.source, &collection_abs, anchor)
            }
            None => Some(absolute_owner.clone()),
        }
        .ok_or_else(|| refused("has no representable collection path"))?;
        let mut condition = predicate.lower(self);
        if output_position == 1 {
            condition = self.alloc(Node::Call {
                function: "not".into(),
                args: vec![condition],
            });
        }
        let one = self.alloc(Node::Const {
            value: Value::Int(1),
        });
        let zero = self.alloc(Node::Const {
            value: Value::Int(0),
        });
        let contribution = self.alloc(Node::If {
            condition,
            then: one,
            else_: zero,
        });
        let expression = if repeating_scalar {
            contribution
        } else {
            // XML/EDI readers represent absent scalar data as Null. Empty text
            // and xsi:nil remain present and must not be discarded. Pinning the
            // owner makes absence final instead of borrowing a sibling's value.
            let field = self.alloc(Node::SourceField {
                frame: Some(absolute_owner.clone()),
                path: value,
            });
            let presence = self.alloc(Node::Call {
                function: "exists".into(),
                args: vec![field],
            });
            self.alloc(Node::If {
                condition: presence,
                then: contribution,
                else_: zero,
            })
        };
        if active_anchor.is_some_and(|anchor| anchor.starts_with(&absolute_owner)) {
            return Ok(Some(self.graph.nodes[&expression].clone()));
        }
        Ok(Some(Node::Aggregate {
            function: AggregateOp::Sum,
            collection,
            value: Vec::new(),
            expression: Some(expression),
            arg: None,
        }))
    }
}

fn prove_predicate(
    builder: &GraphBuilder<'_>,
    feed: u32,
    selected: &SourcePath,
    owner: &[String],
    depth: usize,
    active: &mut BTreeSet<u32>,
    budget: &mut usize,
) -> Option<Predicate> {
    *budget += 1;
    if depth > MAX_PREDICATE_DEPTH
        || *budget > MAX_PREDICATE_NODES
        || !active.insert(feed)
        || declared_producer_count(feed, builder) != 1
    {
        return None;
    }
    let result = (|| {
        if let Some(path) = builder.source_abs_path(feed) {
            let source = builder.sources.get(path.source)?;
            let node = schema_node_at(&source.schema, &path.path)?;
            let SchemaKind::Scalar { ty } = node.kind else {
                return None;
            };
            let (frame, suffix) = split_at_innermost_repeating(&source.schema, &path.path);
            if path != *selected
                || !owner.starts_with(&frame)
                || !source.unambiguous_declared_output(feed)
                || builder.has_source_node_functions(feed)
                || frame.is_empty()
                || node.json_any
                || node.recursive_ref.is_some()
                || !node.effective_xml_view().is_ok_and(|xml| !xml.nil())
                || !matches!(ty, ScalarType::String | ScalarType::Bool)
                || !plain_group_ancestry(&source.schema, &path.path)
            {
                return None;
            }
            return Some(Predicate::Field {
                frame: builder.context_path(&SourcePath {
                    source: path.source,
                    path: frame,
                }),
                path: suffix,
                ty,
            });
        }
        let index = *builder.fn_by_output.get(&feed)?;
        let function = &builder.fn_components[index];
        if function.library == "core" && function.kind == 2 && function.name == "constant" {
            if !function.bounded_scalar_pins_valid
                || !function.raw_literal_valid
                || !function.raw_single_output_valid
            {
                return None;
            }
            let (text, datatype) = function.constant.as_ref()?;
            let value = parse_constant(text, datatype);
            return matches!(value, Value::String(_) | Value::Bool(_))
                .then_some(Predicate::Literal(value));
        }
        if function.kind != 5
            || !function.raw_filtered_count_pins_valid
            || !function.raw_single_output_valid
        {
            return None;
        }
        let (name, types, ty) = predicate_signature(function)?;
        let args = (0..types.len())
            .map(|position| {
                let arg = prove_predicate(
                    builder,
                    builder.input_feed(index, position)?,
                    selected,
                    owner,
                    depth + 1,
                    active,
                    budget,
                )?;
                (arg.ty() == types[position]).then_some(arg)
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Predicate::Call { name, args, ty })
    })();
    active.remove(&feed);
    result
}

fn plain_group_ancestry(schema: &SchemaNode, path: &[String]) -> bool {
    (0..path.len()).all(|length| {
        schema_node_at(schema, &path[..length]).is_some_and(|node| {
            matches!(node.kind, SchemaKind::Group { .. })
                && node.dynamic_fields().is_none()
                && node.alternatives().is_empty()
                && node.recursive_ref.is_none()
                && !node.json_any
                && !node.container_nullable
                && node.json_object_item_nullable != Some(true)
                && !node.json_array_wrapper
                && !node.nillable
        })
    })
}

fn predicate_signature(
    function: &FnComponent,
) -> Option<(&'static str, &'static [ScalarType], ScalarType)> {
    use ScalarType::{Bool, String as Text};
    match (function.library.as_str(), map_component_name(function)?) {
        ("core", "equal") => Some(("equal", &[Text, Text], Bool)),
        ("core", "not_equal") => Some(("not_equal", &[Text, Text], Bool)),
        ("core", "and") => Some(("and", &[Bool, Bool], Bool)),
        ("core", "or") => Some(("or", &[Bool, Bool], Bool)),
        ("core", "not") => Some(("not", &[Bool], Bool)),
        ("core" | "lang", "normalize_space") => Some(("normalize_space", &[Text], Text)),
        ("core" | "lang", "trim") => Some(("trim", &[Text], Text)),
        ("core" | "lang", "is_empty") => Some(("is_empty", &[Text], Bool)),
        _ => None,
    }
}
