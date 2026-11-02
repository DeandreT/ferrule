//! Exact metadata profile; no signature, context or business-rule inference.
use std::collections::BTreeSet;

use super::{MAX_SYMBOL_BYTES, consumers, limits, reserved_owner};
use crate::import::{function, schema::parse_u32};

fn attributes(node: roxmltree::Node<'_, '_>, allowed: &[&str]) -> Result<(), String> {
    if node.tag_name().namespace().is_some()
        || node
            .attributes()
            .any(|a| a.namespace().is_some() || !allowed.contains(&a.name()))
    {
        return Err("unknown or namespaced declarative metadata".into());
    }
    Ok(())
}

pub(in crate::import::udf) fn validate_definition(
    component: roxmltree::Node<'_, '_>,
) -> Result<(), String> {
    attributes(component, &["library", "name", "inline", "uid", "kind"])?;
    for attribute in ["name", "library"] {
        if component
            .attribute(attribute)
            .is_none_or(|s| s.trim().is_empty() || s.len() > MAX_SYMBOL_BYTES)
        {
            return Err("missing/oversized qualified function identity".into());
        }
    }
    if reserved_owner(component.attribute("library").unwrap_or_default()) {
        return Err("reserved built-in/node-function library owner".into());
    }
    if component.attribute("inline").is_some_and(|s| s != "0")
        || component.attribute("kind").is_some_and(|s| s != "19")
    {
        return Err(
            "inline/unknown declaration kind requires unsupported typed call adaptation".into(),
        );
    }
    let structures = component
        .children()
        .filter(|n| n.is_element())
        .collect::<Vec<_>>();
    let [structure] = structures.as_slice() else {
        return Err("missing/duplicate function structure".into());
    };
    if !structure.has_tag_name("structure") {
        return Err("unsupported declaration structure".into());
    }
    attributes(*structure, &[])?;
    let mut lists = structure.children().filter(|n| n.has_tag_name("children"));
    let children = lists.next().ok_or("missing component list")?;
    if lists.next().is_some()
        || structure
            .children()
            .filter(|n| n.has_tag_name("graph"))
            .count()
            != 1
        || structure
            .children()
            .filter(|n| n.is_element())
            .any(|n| !matches!(n.tag_name().name(), "children" | "graph"))
    {
        return Err("duplicate/unknown function structure metadata".into());
    }
    attributes(children, &[])?;
    let mut all_pins = BTreeSet::new();
    let mut input_pins = BTreeSet::new();
    let mut output_pins = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut output_count = 0;
    for child in children.children().filter(|n| n.is_element()) {
        attributes(child, &["name", "library", "uid", "kind"])?;
        if !child.has_tag_name("component") {
            return Err("non-component body metadata".into());
        }
        let library = child.attribute("library").unwrap_or_default();
        let name = child.attribute("name").unwrap_or_default();
        let kind = child.attribute("kind").unwrap_or_default();
        if name.len() > MAX_SYMBOL_BYTES || library.len() > MAX_SYMBOL_BYTES {
            return Err("scalar symbol byte limit exceeded".into());
        }
        if kind == "19" {
            return Err(
                "nested calls require unsupported typed/eager invocation boundaries".into(),
            );
        }
        let allowed = match (library, kind) {
            ("core", "2" | "3" | "4" | "6" | "7") => true,
            ("core" | "lang", "5") => {
                !matches!(
                    name,
                    "sleep"
                        | "create-guid"
                        | "random"
                        | "now"
                        | "mfd-filepath"
                        | "main-mfd-filepath"
                        | "resolve-filepath"
                        | "set-xsi-nil"
                ) && (function::map_name(name).is_some()
                    || matches!(name, "set-empty" | "not-exists"))
            }
            _ => false,
        };
        if !allowed {
            return Err(format!(
                "unknown/effectful/structured/origin-sensitive body `{name}` ({library}, kind {kind})"
            ));
        }
        if child.children().filter(|n| n.is_element()).any(|n| {
            !matches!(
                n.tag_name().name(),
                "sources" | "targets" | "data" | "properties"
            )
        }) {
            return Err("unsupported body metadata".into());
        }
        for tag in ["sources", "targets", "data", "properties"] {
            if child.children().filter(|n| n.has_tag_name(tag)).count() > 1 {
                return Err("duplicate scalar metadata container".into());
            }
        }
        for properties in child.children().filter(|n| n.has_tag_name("properties")) {
            attributes(properties, &[])?;
            if properties.children().any(|n| n.is_element()) {
                return Err("unsupported scalar properties".into());
            }
        }
        for ports in child
            .children()
            .filter(|n| n.has_tag_name("sources") || n.has_tag_name("targets"))
        {
            attributes(ports, &[])?;
            let input = ports.has_tag_name("sources");
            let mut positions = BTreeSet::new();
            for (index, pin) in ports.children().filter(|n| n.is_element()).enumerate() {
                attributes(pin, &["key", "pos"])?;
                if !pin.has_tag_name("datapoint") || pin.children().any(|n| n.is_element()) {
                    return Err("unsupported scalar pin".into());
                }
                let position = pin
                    .attribute("pos")
                    .map_or(Some(index), |s| parse_u32(Some(s)).map(|n| n as usize))
                    .ok_or("invalid pin position")?;
                if position >= 64 || !positions.insert(position) {
                    return Err(
                        "scalar positions must be unique and below 64; no index fallback".into(),
                    );
                }
                if !input && kind != "3" && (position != 0 || index != 0)
                    || kind == "3" && position >= 2
                    || matches!(kind, "6" | "7") && (position != 0 || index != 0)
                {
                    return Err("unconsumed scalar output/default/control slot".into());
                }
                if input && kind == "2" && pin.attribute("key").is_some() {
                    return Err("constant context/trigger input is unsupported".into());
                }
                if let Some(key) = pin.attribute("key") {
                    let key = parse_u32(Some(key)).ok_or("invalid function-owned port key")?;
                    if !all_pins.insert(key) {
                        return Err("colliding function-owned port key".into());
                    }
                    if input {
                        input_pins.insert(key);
                    } else {
                        output_pins.insert(key);
                    }
                }
            }
        }
        let data = child.children().find(|n| n.has_tag_name("data"));
        let mut typed = 0;
        let mut parameters = 0;
        if let Some(data) = data {
            attributes(data, &[])?;
            for node in data.children().filter(|n| n.is_element()) {
                if node.children().any(|n| n.is_element()) {
                    return Err("nested scalar metadata is unsupported".into());
                }
                match (kind, node.tag_name().name()) {
                    ("2", "constant") => {
                        attributes(node, &["value", "datatype"])?;
                        typed += 1;
                        let value = node.attribute("value").ok_or("missing scalar literal")?;
                        let ty = node.attribute("datatype").unwrap_or_default();
                        if value.len() > 1024
                            || !matches!(
                                ty,
                                "string"
                                    | "anySimpleType"
                                    | "boolean"
                                    | "integer"
                                    | "int"
                                    | "long"
                                    | "decimal"
                                    | "double"
                                    | "float"
                            )
                            || matches!(function::parse_constant(value, ty), ir::Value::Null)
                        {
                            return Err("unsupported/invalid or oversized typed literal".into());
                        }
                    }
                    ("6", "input") | ("7", "output") => {
                        attributes(node, &["datatype"])?;
                        typed += 1;
                        if !matches!(
                            node.attribute("datatype"),
                            Some(
                                "string"
                                    | "boolean"
                                    | "integer"
                                    | "int"
                                    | "long"
                                    | "decimal"
                                    | "double"
                                    | "float"
                            )
                        ) {
                            return Err("missing/unsupported scalar parameter type".into());
                        }
                    }
                    ("6" | "7", "parameter") => {
                        attributes(node, &["name", "usageKind", "optional"])?;
                        parameters += 1;
                        if node.attribute("name") != Some(name)
                            || node.attribute("usageKind")
                                != Some(if kind == "6" { "input" } else { "output" })
                            || node.attribute("optional").is_some_and(|s| s != "0")
                        {
                            return Err(
                                "optional/defaulted/conflicting parameter signature is unsupported"
                                    .into(),
                            );
                        }
                    }
                    _ => return Err("unknown scalar data/signature metadata".into()),
                }
            }
        }
        if matches!(kind, "2" | "6" | "7") && typed != 1 || parameters > 1 {
            return Err("missing/duplicate typed scalar declaration".into());
        }
        if matches!(kind, "6" | "7") && (name.trim().is_empty() || !names.insert((kind, name))) {
            return Err("missing/duplicate parameter name".into());
        }
        if kind == "7" {
            output_count += 1;
        }
    }
    if output_count != 1 {
        return Err("imported function requires one regular scalar output".into());
    }
    // Unlike a permissive graph read, require direction-owned ports and exact
    // nesting. An input->input edge must not masquerade as a producer.
    let graph = structure
        .children()
        .find(|n| n.has_tag_name("graph"))
        .ok_or("missing graph")?;
    attributes(graph, &["directed"])?;
    if graph.attribute("directed").is_some_and(|s| s != "1") {
        return Err("unsupported graph direction".into());
    }
    for node in graph
        .descendants()
        .filter(|n| n.is_element() && *n != graph)
    {
        let (parent, keys, expected) = match node.tag_name().name() {
            "vertices" => ("graph", &[][..], None),
            "vertex" => ("vertices", &["vertexkey"][..], Some(&output_pins)),
            "edges" => ("vertex", &[][..], None),
            "edge" => ("edges", &["vertexkey"][..], Some(&input_pins)),
            _ => return Err("unsupported graph vocabulary".into()),
        };
        attributes(node, keys)?;
        if node.parent().is_none_or(|n| !n.has_tag_name(parent)) {
            return Err("unsupported graph nesting".into());
        }
        if let Some(expected) = expected
            && parse_u32(node.attribute("vertexkey")).is_none_or(|key| !expected.contains(&key))
        {
            return Err("graph references a port outside its owned direction".into());
        }
    }
    let feeds = crate::import::graph::read_edges(structure, Some(&component));
    for input in children
        .children()
        .filter(|n| n.attribute("kind") == Some("6"))
    {
        if default_port(input)?.is_some_and(|port| feeds.contains_key(&port)) {
            return Err("connected typed default requires unsupported inline adaptation".into());
        }
    }
    Ok(())
}

fn default_port(input: roxmltree::Node<'_, '_>) -> Result<Option<u32>, String> {
    let Some(sources) = input.children().find(|n| n.has_tag_name("sources")) else {
        return Ok(None);
    };
    let mut pins = sources.children().filter(|n| n.is_element());
    let Some(pin) = pins.next() else {
        return Ok(None);
    };
    if pins.next().is_some()
        || !pin.has_tag_name("datapoint")
        || pin.attribute("pos").is_some_and(|p| p != "0")
    {
        return Err("default supports one position-0 slot".into());
    }
    pin.attribute("key")
        .map(|s| parse_u32(Some(s)).ok_or_else(|| "invalid default key".into()))
        .transpose()
}

pub(in crate::import::udf) fn required_parameters(
    component: roxmltree::Node<'_, '_>,
) -> Result<BTreeSet<u32>, String> {
    // Validation has already refused optional or connected-default signatures.
    component
        .children()
        .find(|n| n.has_tag_name("structure"))
        .and_then(|n| n.children().find(|n| n.has_tag_name("children")))
        .into_iter()
        .flat_map(|n| n.children())
        .filter(|n| n.has_tag_name("component") && n.attribute("kind") == Some("6"))
        .map(|n| {
            parse_u32(n.attribute("uid"))
                .ok_or_else(|| "invalid required parameter componentid".into())
        })
        .collect()
}

pub(in crate::import::udf) fn validate_call(
    component: &roxmltree::Node<'_, '_>,
    budget: &limits::ConstructionBudget,
) -> Result<(), String> {
    let result = (|| {
        attributes(*component, &["name", "library", "kind", "uid"])?;
        if component.attribute("kind") != Some("19") {
            return Err("only saved kind-19 calls are supported".into());
        }
        let structure = component
            .ancestors()
            .find(|n| n.has_tag_name("structure"))
            .ok_or("no owning graph")?;
        let owner = structure.parent().ok_or("no owning mapping")?;
        if owner.attribute("library").is_some_and(|l| !l.is_empty())
            || owner.parent().is_none_or(|n| !n.has_tag_name("mapping"))
        {
            return Err("nested typed/eager regular-call boundaries are unsupported".into());
        }
        budget.claim(
            512usize.saturating_add(structure.descendants().count().saturating_mul(8)),
            limits::decoded_bytes(*component).saturating_mul(8),
            0,
        )?;
        let mut data_count = 0;
        let mut roots = BTreeSet::new();
        let mut pins = BTreeSet::new();
        for node in component
            .descendants()
            .filter(|n| n.is_element() && *n != *component)
        {
            match node.tag_name().name() {
                "data" if node.parent() == Some(*component) => {
                    attributes(node, &[])?;
                    data_count += 1;
                }
                "properties"
                    if node.parent() == Some(*component)
                        && !node.children().any(|n| n.is_element()) =>
                {
                    attributes(node, &[])?;
                }
                "root" if node.parent().is_some_and(|n| n.has_tag_name("data")) => {
                    attributes(node, &["rootindex"])?;
                    let index = node.attribute("rootindex").unwrap_or("0");
                    if !matches!(index, "0" | "1") || !roots.insert(index) {
                        return Err("duplicate/unknown call root direction".into());
                    }
                }
                "entry"
                    if node.parent().is_some_and(|n| n.has_tag_name("root"))
                        && !node.children().any(|n| n.is_element()) =>
                {
                    attributes(node, &["name", "componentid", "inpkey", "outkey"])?;
                    if node
                        .attribute("name")
                        .is_none_or(|s| s.len() > MAX_SYMBOL_BYTES)
                        || parse_u32(node.attribute("componentid")).is_none()
                    {
                        return Err("missing/oversized call port identity".into());
                    }
                    let input = node
                        .parent()
                        .and_then(|n| n.attribute("rootindex"))
                        .unwrap_or("0")
                        == "0";
                    if node
                        .attribute(if input { "outkey" } else { "inpkey" })
                        .is_some()
                    {
                        return Err("call port has wrong root direction".into());
                    }
                    let key = parse_u32(node.attribute(if input { "inpkey" } else { "outkey" }))
                        .ok_or("missing/invalid call port key")?;
                    if !pins.insert(key) {
                        return Err("colliding call pin key".into());
                    }
                }
                _ => return Err("structured/unknown call-site context metadata".into()),
            }
        }
        if data_count != 1 {
            return Err("missing/duplicate call data".into());
        }
        let children = structure
            .children()
            .find(|n| n.has_tag_name("children"))
            .ok_or("no component list")?;
        for key in pins {
            let declarations = children
                .descendants()
                .filter(|node| {
                    ["inpkey", "outkey"]
                        .into_iter()
                        .any(|attribute| parse_u32(node.attribute(attribute)) == Some(key))
                        || node.has_tag_name("datapoint")
                            && parse_u32(node.attribute("key")) == Some(key)
                })
                .count();
            if declarations != 1 {
                return Err("call pin collides with another owner or direction".into());
            }
        }
        if children
            .children()
            .filter(|n| n.has_tag_name("component"))
            .any(|n| {
                n.attribute("library") == Some("core")
                    && matches!(n.attribute("kind"), Some("6" | "7"))
                    || n.descendants().any(|p| {
                        p.has_tag_name("properties") && p.attribute("UsageKind").is_some()
                            || p.has_tag_name("parameter") && p.attribute("usageKind").is_some()
                    })
            })
        {
            return Err("RootFunctionABI/variable parameter consumption is unsupported".into());
        }
        let feeds = crate::import::graph::read_edges(&structure, Some(&owner));
        consumers::validate(component, children, &feeds)
            .map_err(|reason| reason.message().to_string())
    })();
    result.map_err(|reason: String| format!("imported call consumption refused: {reason}"))
}
