//! Opt-in static, instant-period XBRL table byte companions. No taxonomy is resolved.
use crate::{composition::ClassPart, literal::string};
use codegen::{ArtifactSet, Program};
use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{XbrlBoundaryMode, XbrlBoundaryOptions};

/// Options are bound by exact declaration name and order, never guessed from XML or paths.
pub struct Policy<'a> {
    pub source: &'a XbrlBoundaryOptions,
    pub target: &'a XbrlBoundaryOptions,
    pub extra_sources: &'a [(&'a str, &'a XbrlBoundaryOptions)],
    pub extra_targets: &'a [(&'a str, &'a XbrlBoundaryOptions)],
}

const I: &str = "http://www.xbrl.org/2003/instance";
fn descriptor(
    schema: &SchemaNode,
    options: &XbrlBoundaryOptions,
    read: bool,
) -> Result<String, String> {
    let bad = || "unsupported instant-period XBRL table boundary".to_string();
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(bad());
    };
    let [rows] = children.as_slice() else {
        return Err(bad());
    };
    let SchemaKind::Group { children, .. } = &rows.kind else {
        return Err(bad());
    };
    let [_, _, fact] = children.as_slice() else {
        return Err(bad());
    };
    let SchemaKind::Scalar { ty } = fact.kind else {
        return Err(bad());
    };
    if !matches!(ty, ScalarType::Int | ScalarType::String)
        || !name(&rows.name)
        || !name(&fact.name)
        || matches!(
            fact.name.as_str(),
            "identifier" | "period" | "instant" | "forever" | "startDate" | "endDate"
        )
    {
        return Err(bad());
    }
    let text = |n: &str| SchemaNode::scalar(n, ScalarType::String);
    let shape = SchemaNode::group(
        &schema.name,
        vec![
            SchemaNode::group(
                &rows.name,
                vec![
                    SchemaNode::group(
                        "identifier",
                        vec![text("scheme").attribute(), text(ir::XML_TEXT_FIELD).text()],
                    ),
                    SchemaNode::group("period", vec![text("instant")]),
                    SchemaNode::scalar(&fact.name, ty),
                ],
            )
            .repeating(),
        ],
    );
    if &shape != schema
        || options.mode()
            != if read {
                XbrlBoundaryMode::ExternalSource
            } else {
                XbrlBoundaryMode::ExternalTarget
            }
        || options.presentation().is_some()
        || !options.fact_bindings().is_empty()
        || options.namespace_bindings().len() != 4
    {
        return Err(bad());
    }
    let lookup = |tail: &[&str]| {
        options
            .namespace_bindings()
            .iter()
            .find(|b| {
                b.path()
                    .iter()
                    .map(String::as_str)
                    .eq(std::iter::once(rows.name.as_str()).chain(tail.iter().copied()))
            })
            .map(|b| b.namespace())
    };
    if lookup(&["identifier"]) != Some(I)
        || lookup(&["period"]) != Some(I)
        || lookup(&["period", "instant"]) != Some(I)
    {
        return Err(bad());
    }
    let ns = lookup(&[&fact.name]).ok_or_else(bad)?;
    if !safe_literal(ns)
        || !safe_literal(options.taxonomy())
        || matches!(
            ns,
            I | "view"
                | "http://www.altova.com/mapforce"
                | "http://www.xbrl.org/2003/linkbase"
                | "http://xbrl.org/2006/xbrldi"
                | "http://www.w3.org/1999/xlink"
                | "http://www.xbrl.org/2003/iso4217"
                | "http://www.w3.org/XML/1998/namespace"
                | "http://www.w3.org/2000/xmlns/"
        )
    {
        return Err(bad());
    }
    Ok(format!(
        "new({}, {}, {}, {}, {})",
        string(&rows.name),
        string(&fact.name),
        string(ns),
        string(&options.taxonomy().replace('\\', "/")),
        ty == ScalarType::Int
    ))
}
fn name(n: &str) -> bool {
    n.len() <= 128
        && n.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && n.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn safe_literal(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 4096
        && n.bytes()
            .all(|b| b.is_ascii_graphic() && !b"<&>\"'".contains(&b))
}

/// Emit a standalone library for static primary and named XBRL sources and targets.
pub fn emit(program: &Program, policy: &Policy<'_>) -> Result<ArtifactSet, String> {
    if program.xml_boundary.is_some()
        || program.extra_sources.len() > 15
        || program.extra_targets.len() > 15
        || program.extra_sources.iter().any(|s| s.dynamic.is_some())
        || !program
            .extra_sources
            .iter()
            .map(|s| &*s.name)
            .eq(policy.extra_sources.iter().map(|p| p.0))
        || !program
            .extra_targets
            .iter()
            .map(|t| &*t.name)
            .eq(policy.extra_targets.iter().map(|p| p.0))
    {
        return Err("unsupported static XBRL endpoint set".into());
    }
    let mut sources = vec![descriptor(&program.source, policy.source, true)?];
    let mut targets = vec![descriptor(&program.target, policy.target, false)?];
    for (s, (_, p)) in program.extra_sources.iter().zip(policy.extra_sources) {
        sources.push(descriptor(&s.source, p, true)?)
    }
    for (t, (_, p)) in program.extra_targets.iter().zip(policy.extra_targets) {
        targets.push(descriptor(&t.target, p, false)?)
    }
    let names = policy
        .extra_sources
        .iter()
        .map(|(n, _)| string(n))
        .collect::<Vec<_>>()
        .join(",");
    let code = format!(
        "private static readonly global::Ferrule.Runtime.XbrlTable[] XbrlSources = [{}], XbrlTargets = [{}];\nprivate static readonly string[] XbrlNames = [{}];\n{}",
        sources.join(","),
        targets.join(","),
        names,
        include_str!("xbrl_members.cs.in")
    );
    let mut lib = crate::library(program).map_err(|e| e.to_string())?;
    lib.add_companion("GeneratedMapping.Xbrl.cs", ClassPart::members(code));
    lib.add_runtime(&[(
        "Runtime/XbrlTable.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/XbrlTable.cs"),
    )]);
    lib.into_artifacts().map_err(|e| e.to_string())
}
