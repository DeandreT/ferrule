use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use ir::{SchemaKind, SchemaNode, XML_TEXT_FIELD};
use mapping::{
    FormatOptions, XBRL_UNIT_FIELD_PREFIX, XbrlBoundaryMode, XbrlBoundaryOptions, XbrlFactType,
};

use crate::MfdError;

use super::concatenation::TargetBranches;
use super::schema::{GeneratedSibling, PortTree, RenderedSchemaComponent, Side, xml_escape};

pub(super) struct RenderArgs<'a> {
    pub(super) schema: &'a SchemaNode,
    pub(super) ports: &'a PortTree,
    pub(super) side: Side,
    pub(super) instance_path: Option<&'a str>,
    pub(super) options: &'a FormatOptions,
    pub(super) mfd_path: &'a Path,
    pub(super) target_branches: Option<&'a TargetBranches>,
    pub(super) component_name: &'a str,
    pub(super) component_uid: u32,
    pub(super) default_output: bool,
    pub(super) used_ports: &'a BTreeSet<u32>,
}

pub(super) fn validate_side(
    schema: &SchemaNode,
    options: &FormatOptions,
    expected_mode: XbrlBoundaryMode,
    side_name: &str,
) -> Result<(), MfdError> {
    let Some(boundary) = options.xbrl.as_ref() else {
        return Ok(());
    };
    if boundary.mode() != expected_mode {
        return Err(unsupported(format!(
            "the {side_name} XBRL boundary has mode {:?}, expected {:?}",
            boundary.mode(),
            expected_mode
        )));
    }
    if schema.name != "xbrl" || !matches!(schema.kind, SchemaKind::Group { .. }) || schema.repeating
    {
        return Err(unsupported(format!(
            "the {side_name} XBRL boundary requires one non-repeating group schema named `xbrl`"
        )));
    }
    if has_conflicting_options(options) {
        return Err(unsupported(format!(
            "the {side_name} XBRL boundary cannot combine XBRL metadata with another format's options"
        )));
    }
    if !boundary.fact_bindings().is_empty() && boundary.presentation().is_none() {
        return Err(unsupported(format!(
            "the {side_name} XBRL boundary has numeric fact metadata but no presentation path"
        )));
    }
    for binding in boundary.namespace_bindings() {
        if !schema_has_emitted_path(schema, binding.path()) {
            return Err(unsupported(format!(
                "the {side_name} XBRL namespace path `{}` is missing from its schema",
                binding.path().join("/")
            )));
        }
    }
    Ok(())
}

pub(super) fn explicit_text_ports(
    schema: &SchemaNode,
    options: &FormatOptions,
) -> BTreeSet<Vec<String>> {
    if options.xbrl.is_none() {
        return BTreeSet::new();
    }
    fn collect(node: &SchemaNode, path: &mut Vec<String>, result: &mut BTreeSet<Vec<String>>) {
        let SchemaKind::Group { children, .. } = &node.kind else {
            return;
        };
        if children.iter().any(|child| child.text)
            && children.iter().any(|child| !child.text && !child.attribute)
        {
            path.push(XML_TEXT_FIELD.to_string());
            result.insert(path.clone());
            path.pop();
        }
        for child in children {
            path.push(child.name.clone());
            collect(child, path, result);
            path.pop();
        }
    }
    let mut result = BTreeSet::new();
    collect(schema, &mut Vec::new(), &mut result);
    result
}

pub(super) fn render(args: RenderArgs<'_>) -> Result<RenderedSchemaComponent, MfdError> {
    let boundary = args.options.xbrl.as_ref().ok_or_else(|| {
        unsupported("an XBRL component has no retained boundary metadata".to_string())
    })?;
    let expected_mode = match args.side {
        Side::Source => XbrlBoundaryMode::ExternalSource,
        Side::Target => XbrlBoundaryMode::ExternalTarget,
    };
    validate_side(
        args.schema,
        args.options,
        expected_mode,
        side_name(args.side),
    )?;

    let attr = match args.side {
        Side::Source => "outkey",
        Side::Target => "inpkey",
    };
    let namespace_slots = namespace_slots(boundary);
    let entries = render_payload_entries(
        args.schema,
        args.ports,
        attr,
        boundary,
        &namespace_slots,
        args.target_branches,
        args.used_ports,
    )?;
    let root_key = args.ports.required_key_for_abs(&[], "XBRL document root")?;
    let root_port = if args.used_ports.contains(&root_key) {
        format!(" {attr}=\"{root_key}\"")
    } else {
        String::new()
    };
    if root_port.is_empty() && entries.is_empty() {
        return Err(unsupported(format!(
            "the {} XBRL boundary has no connected ports to export",
            side_name(args.side)
        )));
    }

    let header = match args.side {
        Side::Source => String::new(),
        Side::Target if args.default_output => {
            "<properties XSLTDefaultOutput=\"1\"/>\n\t\t\t\t\t".to_string()
        }
        Side::Target => String::new(),
    };
    let view = match args.side {
        Side::Source => "<view rbx=\"300\" rby=\"400\"/>",
        Side::Target => "<view ltx=\"700\" rbx=\"1000\" rby=\"400\"/>",
    };
    let mut namespace_header = String::from("<namespace/>");
    for namespace in namespace_slots.keys() {
        let _ = write!(
            namespace_header,
            "<namespace uid=\"{}\"/>",
            xml_escape(namespace)
        );
    }
    let mut metadata = format!(" schema=\"{}\"", xml_escape(boundary.taxonomy()));
    if let Some(presentation) = boundary.presentation() {
        let _ = write!(metadata, " sps=\"{}\"", xml_escape(presentation));
    }
    if let Some(instance) = args.instance_path {
        let instance_attr = match args.side {
            Side::Source => "inputinstance",
            Side::Target => "outputinstance",
        };
        let _ = write!(metadata, " {instance_attr}=\"{}\"", xml_escape(instance));
    }

    let mut out = String::new();
    let _ = write!(
        out,
        "\t\t\t\t<component name=\"{}\" library=\"xbrl\" uid=\"{}\" kind=\"27\">\n\
         \t\t\t\t\t{header}{view}\n\
         \t\t\t\t\t<data>\n\
         \t\t\t\t\t\t<root>\n\
         \t\t\t\t\t\t\t<header><namespaces>{namespace_header}</namespaces></header>\n\
         \t\t\t\t\t\t\t<entry name=\"FileInstance\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t<entry name=\"document\" expanded=\"1\">\n\
         \t\t\t\t\t\t\t\t\t<entry name=\"xbrl\"{root_port} expanded=\"1\">\n\
         {entries}\
         \t\t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t\t</entry>\n\
         \t\t\t\t\t\t</root>\n\
         \t\t\t\t\t\t<xbrl{metadata}/>\n\
         \t\t\t\t\t</data>\n\
         \t\t\t\t</component>\n",
        xml_escape(args.component_name),
        args.component_uid,
    );

    let siblings = presentation_sibling(args.mfd_path, boundary)?
        .into_iter()
        .collect();
    Ok(RenderedSchemaComponent { xml: out, siblings })
}

fn render_payload_entries(
    schema: &SchemaNode,
    ports: &PortTree,
    attr: &str,
    boundary: &XbrlBoundaryOptions,
    namespace_slots: &BTreeMap<String, usize>,
    target_branches: Option<&TargetBranches>,
    used_ports: &BTreeSet<u32>,
) -> Result<String, MfdError> {
    let mut out = String::new();
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Ok(out);
    };
    for child in children {
        out.push_str(&render_node(RenderNodeArgs {
            node: child,
            schema_path: vec![child.name.clone()],
            emitted_path: vec![emitted_name(&child.name).to_string()],
            indent: 10,
            attr,
            boundary,
            namespace_slots,
            ports,
            target_branches,
            active_branch: None,
            used_ports,
        })?);
    }
    Ok(out)
}

struct RenderNodeArgs<'a> {
    node: &'a SchemaNode,
    schema_path: Vec<String>,
    emitted_path: Vec<String>,
    indent: usize,
    attr: &'a str,
    boundary: &'a XbrlBoundaryOptions,
    namespace_slots: &'a BTreeMap<String, usize>,
    ports: &'a PortTree,
    target_branches: Option<&'a TargetBranches>,
    active_branch: Option<(Vec<String>, usize)>,
    used_ports: &'a BTreeSet<u32>,
}

fn render_node(args: RenderNodeArgs<'_>) -> Result<String, MfdError> {
    let count = args
        .active_branch
        .is_none()
        .then(|| {
            args.target_branches
                .and_then(|branches| branches.count(&args.schema_path))
        })
        .flatten()
        .unwrap_or(1);
    let mut out = String::new();
    for index in 0..count {
        let branch = args
            .active_branch
            .clone()
            .or_else(|| (count > 1).then(|| (args.schema_path.clone(), index)));
        out.push_str(&render_node_variant(&args, branch, index > 0)?);
    }
    Ok(out)
}

fn render_node_variant(
    args: &RenderNodeArgs<'_>,
    active_branch: Option<(Vec<String>, usize)>,
    cloned: bool,
) -> Result<String, MfdError> {
    let key_for = |path: &[String]| {
        active_branch
            .as_ref()
            .and_then(|(root, index)| {
                args.target_branches
                    .and_then(|branches| branches.key_for(args.ports, root, *index, path))
            })
            .or_else(|| args.ports.key_for_abs(path))
    };
    let own_key = key_for(&args.schema_path).ok_or_else(|| {
        unsupported(format!(
            "internal XBRL port `{}` was not allocated",
            args.schema_path.join("/")
        ))
    })?;
    let own_used = args.used_ports.contains(&own_key);
    let pad = "\t".repeat(args.indent);
    let name = emitted_name(&args.node.name);
    let type_attr = if args.node.attribute {
        " type=\"attribute\""
    } else {
        ""
    };
    let clone_attr = if cloned { " clone=\"1\"" } else { "" };
    let namespace_attr = args
        .boundary
        .namespace_bindings()
        .iter()
        .find(|binding| binding.path() == args.emitted_path)
        .and_then(|binding| args.namespace_slots.get(binding.namespace()))
        .map(|slot| format!(" ns=\"{slot}\""))
        .unwrap_or_default();

    let SchemaKind::Group { children, .. } = &args.node.kind else {
        if !own_used {
            return Ok(String::new());
        }
        return Ok(format!(
            "{pad}<entry name=\"{}\"{type_attr}{namespace_attr} {}=\"{own_key}\" expanded=\"1\"{clone_attr}/>\n",
            xml_escape(name),
            args.attr
        ));
    };

    let text = children.iter().find(|child| child.text);
    let mixed = text.is_some() && children.iter().any(|child| !child.text && !child.attribute);
    let mut child_xml = String::new();
    for child in children.iter().filter(|child| !child.text) {
        let mut schema_path = args.schema_path.clone();
        schema_path.push(child.name.clone());
        let mut emitted_path = args.emitted_path.clone();
        emitted_path.push(emitted_name(&child.name).to_string());
        child_xml.push_str(&render_node(RenderNodeArgs {
            node: child,
            schema_path,
            emitted_path,
            indent: args.indent + 1,
            attr: args.attr,
            boundary: args.boundary,
            namespace_slots: args.namespace_slots,
            ports: args.ports,
            target_branches: args.target_branches,
            active_branch: active_branch.clone(),
            used_ports: args.used_ports,
        })?);
    }

    let mut out = String::new();
    if mixed {
        let mut text_path = args.schema_path.clone();
        text_path.push(XML_TEXT_FIELD.to_string());
        let text_key = key_for(&text_path).ok_or_else(|| {
            unsupported(format!(
                "internal XBRL text port `{}` was not allocated",
                text_path.join("/")
            ))
        })?;
        if args.used_ports.contains(&text_key) {
            let _ = writeln!(
                out,
                "{pad}<entry name=\"{}\"{namespace_attr} {}=\"{text_key}\" expanded=\"1\"{clone_attr}/>",
                xml_escape(name),
                args.attr
            );
        }
    }

    let value_key = if text.is_some() && !mixed {
        let mut text_path = args.schema_path.clone();
        text_path.push(XML_TEXT_FIELD.to_string());
        key_for(&text_path).unwrap_or(own_key)
    } else {
        own_key
    };
    let value_used = args.used_ports.contains(&value_key);
    if text.is_some() && !mixed && own_key != value_key && own_used && value_used {
        return Err(unsupported(format!(
            "XBRL simple-content path `{}` requires distinct structural and scalar ports",
            args.emitted_path.join("/")
        )));
    }
    let port_key = if value_used {
        Some(value_key)
    } else if own_used {
        Some(own_key)
    } else {
        None
    };
    if port_key.is_none() && child_xml.is_empty() {
        return Ok(out);
    }
    let port_attr = port_key
        .map(|key| format!(" {}=\"{key}\"", args.attr))
        .unwrap_or_default();
    let _ = writeln!(
        out,
        "{pad}<entry name=\"{}\"{type_attr}{namespace_attr}{port_attr} expanded=\"1\"{clone_attr}>",
        xml_escape(name)
    );
    out.push_str(&child_xml);
    let _ = writeln!(out, "{pad}</entry>");
    Ok(out)
}

fn namespace_slots(boundary: &XbrlBoundaryOptions) -> BTreeMap<String, usize> {
    boundary
        .namespace_bindings()
        .iter()
        .map(|binding| binding.namespace().to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .enumerate()
        .map(|(index, namespace)| (namespace, index + 1))
        .collect()
}

fn presentation_sibling(
    mfd_path: &Path,
    boundary: &XbrlBoundaryOptions,
) -> Result<Option<GeneratedSibling>, MfdError> {
    let Some(declared) = boundary.presentation() else {
        return Ok(None);
    };
    let path = resolve_sibling(mfd_path, declared)?;
    if path == mfd_path {
        return Err(unsupported(
            "the XBRL presentation path conflicts with the exported design path".to_string(),
        ));
    }
    let contents = render_sps(boundary)?;
    match std::fs::read_to_string(&path) {
        Ok(existing) if existing == contents => Ok(None),
        Ok(_) => Err(unsupported(format!(
            "XBRL presentation `{}` already exists with different content",
            path.display()
        ))),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            Ok(Some(GeneratedSibling { path, contents }))
        }
        Err(error) => Err(MfdError::Io(error)),
    }
}

fn resolve_sibling(mfd_path: &Path, declared: &str) -> Result<PathBuf, MfdError> {
    let portable = declared.replace('\\', "/");
    let relative = Path::new(&portable);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(unsupported(format!(
            "XBRL presentation path `{declared}` is not a bounded relative path"
        )));
    }
    Ok(mfd_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(relative))
}

fn render_sps(boundary: &XbrlBoundaryOptions) -> Result<String, MfdError> {
    let namespaces = boundary
        .namespace_bindings()
        .iter()
        .map(|binding| (binding.path(), binding.namespace()))
        .collect::<BTreeMap<_, _>>();
    let mut concepts = BTreeMap::new();
    for binding in boundary.fact_bindings() {
        let namespace = namespaces.get(binding.path()).ok_or_else(|| {
            unsupported(format!(
                "XBRL fact path `{}` has no namespace binding",
                binding.path().join("/")
            ))
        })?;
        let local = binding
            .path()
            .last()
            .ok_or_else(|| unsupported("an XBRL fact binding has an empty path".to_string()))?;
        if let Some(existing) = concepts.insert(
            ((*namespace).to_string(), local.clone()),
            binding.fact_type(),
        ) && existing != binding.fact_type()
        {
            return Err(unsupported(format!(
                "XBRL concept `{{{namespace}}}{local}` has conflicting numeric item types"
            )));
        }
    }
    let used_namespaces = concepts
        .keys()
        .map(|(namespace, _)| namespace.clone())
        .collect::<BTreeSet<_>>();
    let prefixes = used_namespaces
        .into_iter()
        .enumerate()
        .map(|(index, namespace)| (namespace, format!("ns{}", index + 1)))
        .collect::<BTreeMap<_, _>>();
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<structure>\n\t<schemasources><namespaces>\n",
    );
    for (namespace, prefix) in &prefixes {
        let _ = writeln!(
            out,
            "\t\t<nspair prefix=\"{}\" uri=\"{}\"/>",
            xml_escape(prefix),
            xml_escape(namespace)
        );
    }
    out.push_str("\t</namespaces></schemasources>\n");
    for ((namespace, local), fact_type) in concepts {
        let prefix = &prefixes[&namespace];
        let _ = writeln!(
            out,
            "\t<template subtype=\"xbrl-concept-aspect\" match=\"{}:{}\"><children><calltemplate subtype=\"named\" match=\"{}\"/></children></template>",
            xml_escape(prefix),
            xml_escape(&local),
            fact_type_name(fact_type)
        );
    }
    out.push_str("</structure>\n");
    Ok(out)
}

fn schema_has_emitted_path(schema: &SchemaNode, path: &[String]) -> bool {
    let mut candidates = vec![schema];
    for segment in path {
        candidates = candidates
            .into_iter()
            .flat_map(|candidate| match &candidate.kind {
                SchemaKind::Group { children, .. } => children
                    .iter()
                    .filter(|child| {
                        child.name == *segment
                            || segment == "unit" && child.name.starts_with(XBRL_UNIT_FIELD_PREFIX)
                    })
                    .collect::<Vec<_>>(),
                SchemaKind::Scalar { .. } => Vec::new(),
            })
            .collect();
        if candidates.is_empty() {
            return false;
        }
    }
    true
}

fn emitted_name(name: &str) -> &str {
    if name.starts_with(XBRL_UNIT_FIELD_PREFIX) {
        "unit"
    } else {
        name
    }
}

fn fact_type_name(fact_type: XbrlFactType) -> &'static str {
    match fact_type {
        XbrlFactType::Monetary => "monetaryItemType",
        XbrlFactType::Numeric => "numericItemType",
        XbrlFactType::Shares => "sharesItemType",
        XbrlFactType::PerShare => "perShareItemType",
    }
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Source => "source",
        Side::Target => "target",
    }
}

fn has_conflicting_options(options: &FormatOptions) -> bool {
    options.lenient_segments
        || options.idoc.is_some()
        || options.swift_mt.is_some()
        || options.delimiter.is_some()
        || options.has_header_row.is_some()
        || options.fixed_width.is_some()
        || options.flextext.is_some()
        || options.pdf.is_some()
        || options.http_get.is_some()
        || options.external_source.is_some()
        || options.json_lines
        || options.protobuf.is_some()
        || options.xlsx_sheet.is_some()
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || options.xlsx_update_existing
        || !options.xlsx_rows.is_empty()
        || options.xlsx_composite.is_some()
        || options.xlsx_grid.is_some()
        || options.xlsx_hierarchical.is_some()
}

fn unsupported(message: String) -> MfdError {
    MfdError::Unsupported(message)
}
