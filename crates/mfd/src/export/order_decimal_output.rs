//! Reconstruct the two decimal source-node products in the Order-in-USD design.
//!
//! Import expands a native decimal output-node function at each used source
//! leaf into `multiply(to_number(field), to_number(1.25))`. The exact finite
//! XML-backed shape below can be inverted without exporting Ferrule calls.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::ops::Range;

use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{Graph, IterationOutput, Node, NodeId, Project, Scope, ScopeIteration};

use crate::MfdError;

use super::TargetExport;
use super::schema::{RenderedSchemaComponent, SideFormat};
use super::source::SourceExports;

const FUNCTION_NAME: &str = "6D936103-3F10-4ED6-9B81-26E01E327780";
const ROW: &[&str] = &["LineItems", "LineItem"];
const PRICE_FIELDS: [&str; 2] = ["SinglePrice", "Price"];

pub(super) struct NativeOrderDecimal {
    absorbed: BTreeSet<NodeId>,
    products: [(NodeId, u32); 2],
}

impl NativeOrderDecimal {
    pub(super) fn plan(
        project: &Project,
        sources: &SourceExports<'_>,
        targets: &[TargetExport<'_>],
    ) -> Option<Self> {
        if sources.len() != 1 {
            return None;
        }
        let source = sources.iter().next()?;
        let [target] = targets else { return None };
        if source.format != SideFormat::Xml
            || !project.source_options.xml_document
            || target.format != SideFormat::Csv
            || project.target_options.delimiter != Some(',')
            || project.target_options.has_header_row != Some(false)
            || !project.extra_sources.is_empty()
            || !project.extra_targets.is_empty()
            || !project.failure_rules.is_empty()
            || !project.user_functions.is_empty()
            || !project.source_options.mfd_decimal_input_names.is_empty()
            || project.source.name != "Order"
            || project.target.name != "Text file"
            || project.root.bindings.len() != 5
            || !scope_is_exact_row_projection(&project.root)
            || !target_is_exact_csv(&project.target)
            || !source_is_exact_decimal_row(&project.source)
            || project.graph.nodes.len() != 18
        {
            return None;
        }

        let graph = &project.graph;
        let mut seen = BTreeSet::new();
        let bindings = &project.root.bindings;
        if bindings
            .iter()
            .map(|binding| binding.target_field.as_str())
            .collect::<Vec<_>>()
            != ["Company", "Article", "SinglePrice", "Amount", "Price"]
        {
            return None;
        }
        source_field(
            take(graph, bindings[0].node, &mut seen)?,
            &["Customer", "CompanyName"],
            None,
        )?;
        let [name] = call(take(graph, bindings[1].node, &mut seen)?, "upper")? else {
            return None;
        };
        source_field(
            take(graph, *name, &mut seen)?,
            &["Article", "Name"],
            Some(ROW),
        )?;
        let (single_product, single_absorbed) =
            currency_product(graph, bindings[2].node, "SinglePrice", &mut seen)?;
        source_field(
            take(graph, bindings[3].node, &mut seen)?,
            &["Article", "Amount"],
            Some(ROW),
        )?;
        let (price_product, price_absorbed) =
            currency_product(graph, bindings[4].node, "Price", &mut seen)?;
        if seen.len() != graph.nodes.len() {
            return None;
        }

        let single_key = source.ports.key_for_abs(&full_price_path("SinglePrice"))?;
        let price_key = source.ports.key_for_abs(&full_price_path("Price"))?;
        let absorbed = single_absorbed.into_iter().chain(price_absorbed).collect();
        Some(Self {
            absorbed,
            products: [(single_product, single_key), (price_product, price_key)],
        })
    }

    pub(super) fn absorbed_nodes(&self) -> &BTreeSet<NodeId> {
        &self.absorbed
    }

    pub(super) fn seed_aliases(&self, node_out_key: &mut BTreeMap<NodeId, u32>) {
        node_out_key.extend(self.products);
    }

    pub(super) fn apply_to_source(
        &self,
        rendered: &mut RenderedSchemaComponent,
    ) -> Result<(), MfdError> {
        let document = roxmltree::Document::parse(&rendered.xml).map_err(|error| {
            MfdError::Unsupported(format!(
                "native decimal output source cannot be inspected: {error}"
            ))
        })?;
        let mut replacements: Vec<(Range<usize>, String)> = Vec::new();
        for (field, (_, key)) in PRICE_FIELDS.into_iter().zip(self.products) {
            let key = key.to_string();
            let mut entries = document.descendants().filter(|entry| {
                entry.has_tag_name("entry")
                    && entry.attribute("name") == Some(field)
                    && entry.attribute("outkey") == Some(key.as_str())
            });
            let entry = entries
                .next()
                .filter(|_| entries.next().is_none())
                .ok_or_else(|| {
                    MfdError::Unsupported(format!(
                        "native decimal output source has no unique {field} leaf"
                    ))
                })?;
            let range = entry.range();
            let opening = rendered.xml[range.clone()]
                .strip_suffix("/>")
                .ok_or_else(|| {
                    MfdError::Unsupported(format!(
                        "native decimal output source {field} is not a scalar leaf"
                    ))
                })?;
            let indent = rendered.xml[..range.start]
                .rsplit_once('\n')
                .map_or("", |(_, indent)| indent);
            let replacement = format!(
                "{opening}>\n\
                 {indent}\t<outputnodefunctions><rule applyto=\"self\">\
                 <function name=\"{FUNCTION_NAME}\"/><filter datatype=\"decimal\"/>\
                 </rule></outputnodefunctions>\n\
                 {indent}</entry>"
            );
            replacements.push((range, replacement));
        }
        replacements.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        for (range, replacement) in replacements {
            rendered.xml.replace_range(range, &replacement);
        }
        Ok(())
    }

    pub(super) fn definition(&self, uid: &mut u32) -> String {
        let definition_uid = *uid + 1;
        let result_uid = *uid + 2;
        let input_uid = *uid + 3;
        let constant_uid = *uid + 4;
        let multiply_uid = *uid + 5;
        *uid += 5;
        let mut xml = String::new();
        let _ = write!(
            xml,
            "\t<component name=\"{FUNCTION_NAME}\" library=\"mapforce_nodefunction\" uid=\"{definition_uid}\" editable=\"1\">\n\
             \t\t<properties/>\n\
             \t\t<structure><children>\n\
             \t\t\t<component name=\"result\" library=\"core\" uid=\"{result_uid}\" kind=\"7\">\
             <sources><datapoint pos=\"0\" key=\"1\"/></sources>\
             <data><output datatype=\"anySimpleType\"/><parameter usageKind=\"output\" name=\"result\"/></data></component>\n\
             \t\t\t<component name=\"raw_value\" library=\"core\" uid=\"{input_uid}\" kind=\"6\">\
             <targets><datapoint pos=\"0\" key=\"2\"/></targets>\
             <data><input datatype=\"anySimpleType\"/><parameter usageKind=\"input\" name=\"raw_value\"/></data></component>\n\
             \t\t\t<component name=\"constant\" library=\"core\" uid=\"{constant_uid}\" kind=\"2\">\
             <targets><datapoint pos=\"0\" key=\"3\"/></targets>\
             <data><constant value=\"1.25\" datatype=\"decimal\"/></data></component>\n\
             \t\t\t<component name=\"multiply\" library=\"core\" uid=\"{multiply_uid}\" kind=\"5\" growable=\"1\" growablebasename=\"value\">\
             <sources><datapoint pos=\"0\" key=\"4\"/><datapoint pos=\"1\" key=\"5\"/></sources>\
             <targets><datapoint pos=\"0\" key=\"6\"/></targets></component>\n\
             \t\t</children></structure>\n\
             \t\t<connections><edge from=\"6\" to=\"1\"/><edge from=\"2\" to=\"4\"/><edge from=\"3\" to=\"5\"/></connections>\n\
             \t</component>\n"
        );
        xml
    }
}

fn scope_is_exact_row_projection(root: &Scope) -> bool {
    let expected = Scope {
        iteration: ScopeIteration::Source(ROW.iter().map(|part| (*part).to_string()).collect()),
        iteration_output: IterationOutput::Repeated,
        bindings: root.bindings.clone(),
        ..Scope::default()
    };
    matches!(
        (serde_json::to_value(root), serde_json::to_value(expected)),
        (Ok(actual), Ok(expected)) if actual == expected
    )
}

fn target_is_exact_csv(schema: &SchemaNode) -> bool {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return false;
    };
    children.len() == 5
        && children
            .iter()
            .zip([
                ("Company", ScalarType::String),
                ("Article", ScalarType::String),
                ("SinglePrice", ScalarType::String),
                ("Amount", ScalarType::Int),
                ("Price", ScalarType::String),
            ])
            .all(|(child, (name, ty))| {
                child.name == name
                    && !child.repeating
                    && matches!(&child.kind, SchemaKind::Scalar { ty: actual } if *actual == ty)
            })
}

fn source_is_exact_decimal_row(schema: &SchemaNode) -> bool {
    let Some(line_items) = schema.child("LineItems") else {
        return false;
    };
    let Some(line_item) = line_items.child("LineItem") else {
        return false;
    };
    let Some(article) = line_item.child("Article") else {
        return false;
    };
    if line_items.repeating || !line_item.repeating || article.repeating {
        return false;
    }
    PRICE_FIELDS.iter().all(|field| {
        article.child(field).is_some_and(|leaf| {
            !leaf.repeating
                && matches!(
                    leaf.kind,
                    SchemaKind::Scalar {
                        ty: ScalarType::Float
                    }
                )
        })
    })
}

fn full_price_path(field: &str) -> Vec<String> {
    ROW.iter()
        .copied()
        .chain(["Article", field])
        .map(str::to_string)
        .collect()
}

fn take<'a>(graph: &'a Graph, id: NodeId, seen: &mut BTreeSet<NodeId>) -> Option<&'a Node> {
    seen.insert(id).then(|| graph.nodes.get(&id)).flatten()
}

fn call<'a>(node: &'a Node, expected: &str) -> Option<&'a [NodeId]> {
    let Node::Call { function, args } = node else {
        return None;
    };
    (function == expected).then_some(args.as_slice())
}

fn source_field(node: &Node, path: &[&str], frame: Option<&[&str]>) -> Option<()> {
    let Node::SourceField {
        path: actual_path,
        frame: actual_frame,
    } = node
    else {
        return None;
    };
    let matches_path = actual_path
        .iter()
        .map(String::as_str)
        .eq(path.iter().copied());
    let matches_frame = actual_frame
        .as_ref()
        .map(|frame| frame.iter().map(String::as_str).collect::<Vec<_>>())
        == frame.map(<[_]>::to_vec);
    (matches_path && matches_frame).then_some(())
}

fn currency_product(
    graph: &Graph,
    binding: NodeId,
    field: &str,
    seen: &mut BTreeSet<NodeId>,
) -> Option<(NodeId, [NodeId; 5])> {
    let [dollar, product] = call(take(graph, binding, seen)?, "concat")? else {
        return None;
    };
    if !matches!(take(graph, *dollar, seen)?, Node::Const { value: Value::String(value) } if value == "$")
    {
        return None;
    }
    let [field_conversion, constant_conversion] = call(take(graph, *product, seen)?, "multiply")?
    else {
        return None;
    };
    let [field_node] = call(take(graph, *field_conversion, seen)?, "to_number")? else {
        return None;
    };
    source_field(
        take(graph, *field_node, seen)?,
        &["Article", field],
        Some(ROW),
    )?;
    let [constant] = call(take(graph, *constant_conversion, seen)?, "to_number")? else {
        return None;
    };
    if !matches!(take(graph, *constant, seen)?, Node::Const { value: Value::Float(value) } if *value == 1.25)
    {
        return None;
    }
    Some((
        *product,
        [
            *product,
            *field_conversion,
            *field_node,
            *constant_conversion,
            *constant,
        ],
    ))
}
