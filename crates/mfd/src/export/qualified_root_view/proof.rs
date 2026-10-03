//! Complete ownership proof for a closed flat String root profile.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::PathBuf;

use ir::{GroupAlternativeMode, SchemaKind, SchemaNode, XmlAlternativeKind};
use roxmltree::{Document, Node};
use serde::Serialize;

#[path = "proof/schema.rs"]
mod schema;
pub(crate) use schema::owned_string_schema;
use schema::{derived_from_root, normalize_proven_boundary_namespaces, utf8_schema_declaration};

const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
const XS: &str = "http://www.w3.org/2001/XMLSchema";
const PROTOCOL: &str = "http://www.altova.com/mapforce";

#[derive(Debug, Serialize)]
pub(crate) struct Rejection {
    pub code: &'static str,
    pub detail: String,
}
pub(super) type Result<T> = std::result::Result<T, Rejection>;

pub(super) struct SchemaResource {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}
fn reject<T>(code: &'static str, detail: impl Into<String>) -> Result<T> {
    Err(Rejection {
        code,
        detail: detail.into(),
    })
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Identity {
    pub start: usize,
    pub end: usize,
}
fn id(node: Node<'_, '_>) -> Identity {
    Identity {
        start: node.range().start,
        end: node.range().end,
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Pin {
    pub raw: String,
    pub key: u32,
    pub kind: String,
    pub entry: Identity,
    pub component_uid: u32,
    pub view_ordinal: usize,
    pub expanded_field: Option<String>,
    pub path: Vec<String>,
    pub attribute: bool,
    pub connected: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct Boundary {
    pub raw_uid: String,
    pub uid: u32,
    pub component: Identity,
    pub document: Identity,
    pub role: String,
    pub instance_path: String,
    pub declared_root: String,
    pub base_root: Identity,
    pub conditioned_root: Identity,
    pub raw_type_literal: String,
    pub canonical_type: String,
    pub schema_path: PathBuf,
    pub schema_owned_bytes: Vec<u8>,
    pub schema: SchemaNode,
    pub pins: Vec<Pin>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Feed {
    pub raw_source: String,
    pub raw_target: String,
    pub source: Pin,
    pub target: Pin,
    pub vertex: Identity,
    pub edge: Identity,
}

#[derive(Debug, Serialize)]
pub(crate) struct AccountedEmptyVertex {
    pub vertex: Identity,
    pub source: Pin,
}

#[derive(Debug, Serialize)]
pub(crate) struct VerifiedRootViewPlan {
    pub certificate_scope: &'static str,
    pub raw_mapping_owned: String,
    pub structure: Identity,
    pub graph: Identity,
    pub source: Boundary,
    pub target: Boundary,
    pub feeds: Vec<Feed>,
    pub empty_vertices: Vec<AccountedEmptyVertex>,
    pub required_primitives: [&'static str; 2],
}

/// A complete profile proof owns the exact inspected mapping and schema bytes.
/// No deserialization or mutable access may fabricate/rebuild this proof owner.
#[derive(Debug, Serialize)]
pub(crate) struct QualifiedRootViewPlan {
    plan: VerifiedRootViewPlan,
}
impl QualifiedRootViewPlan {
    pub(crate) fn as_plan(&self) -> &VerifiedRootViewPlan {
        &self.plan
    }
}
pub(super) fn prove_qualified(
    text: &str,
    load_schema: &dyn Fn(&str) -> Result<SchemaResource>,
) -> Result<QualifiedRootViewPlan> {
    if text.len() > 1_048_576 {
        return reject("budget", "mapping bytes");
    }
    let document = Document::parse(text).map_err(|e| Rejection {
        code: "xml",
        detail: e.to_string(),
    })?;
    utf8_schema_declaration(text).map_err(|error| Rejection {
        code: "mapping_encoding",
        detail: error.detail,
    })?;
    let plan = prove_profile(text, load_schema, true)?;
    for boundary in [&plan.source, &plan.target] {
        for (identity, wanted) in [
            (&boundary.base_root, Some("all")),
            (&boundary.conditioned_root, None),
        ] {
            let view = document
                .descendants()
                .find(|node| {
                    node.is_element()
                        && node.range().start == identity.start
                        && node.range().end == identity.end
                })
                .ok_or_else(|| Rejection {
                    code: "display_mode",
                    detail: "exact proved view identity missing".into(),
                })?;
            if view.attribute("displayselectionmode") != wanted {
                return reject(
                    "display_mode",
                    "base view requires all; selected view requires absent mode",
                );
            }
        }
    }
    Ok(QualifiedRootViewPlan { plan })
}

fn elements<'a, 'i>(node: Node<'a, 'i>) -> Vec<Node<'a, 'i>> {
    node.children().filter(Node::is_element).collect()
}
fn one<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Result<Node<'a, 'i>> {
    let children = elements(node);
    let mut matching = children.iter().copied().filter(|n| n.has_tag_name(name));
    let Some(value) = matching.next() else {
        return reject("shape", format!("missing {name}"));
    };
    if matching.next().is_some() {
        return reject("shape", format!("duplicate {name}"));
    }
    Ok(value)
}
fn attrs(node: Node<'_, '_>, allowed: &[&str]) -> Result<()> {
    if node
        .attributes()
        .any(|a| a.namespace().is_some() || !allowed.contains(&a.name()))
    {
        return reject(
            "attributes",
            format!("unrepresented attributes at {}", node.range().start),
        );
    }
    Ok(())
}
fn tags(node: Node<'_, '_>, allowed: &[&str]) -> Result<()> {
    if node
        .children()
        .any(|n| n.is_text() && n.text().is_some_and(|t| !t.trim().is_empty()))
    {
        return reject("shape", "unrepresented structural text");
    }
    if elements(node)
        .iter()
        .any(|n| n.tag_name().namespace().is_some() || !allowed.contains(&n.tag_name().name()))
    {
        return reject("shape", "unrepresented or namespaced structural child");
    }
    Ok(())
}
fn number(raw: Option<&str>) -> Result<u32> {
    let raw = raw.ok_or_else(|| Rejection {
        code: "numeric_identity",
        detail: "missing identity".into(),
    })?;
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return reject("numeric_identity", raw);
    }
    match raw.parse::<u32>() {
        Ok(n) if n > 0 => Ok(n),
        _ => reject("numeric_identity", raw),
    }
}
fn empty(node: Node<'_, '_>) -> Result<()> {
    tags(node, &[])
}
fn ncname(local: &str) -> bool {
    if local.is_empty() || local.len() > 4096 || local.contains(':') {
        return false;
    }
    let markup = format!("<{local}/>");
    Document::parse(&markup).is_ok_and(|d| {
        d.root_element().has_tag_name(local) && d.root_element().tag_name().namespace().is_none()
    })
}
fn canonical(raw: &str) -> Result<String> {
    let Some(tail) = raw.strip_prefix('{') else {
        return reject("qname", "expanded QName required");
    };
    let Some((namespace, local)) = tail.split_once('}') else {
        return reject("qname", raw);
    };
    if namespace.contains(['{', '}']) || !ncname(local) {
        return reject("qname", raw);
    }
    let identity = if namespace.is_empty() {
        local.to_string()
    } else {
        format!("{{{namespace}}}{local}")
    };
    if !ir::primary_root_type_identity_is_valid(&identity) {
        return reject("qname", "invalid or oversized resolved type identity");
    }
    Ok(identity)
}
pub(super) fn bounded_schema_bytes(reader: impl Read) -> Result<Vec<u8>> {
    const MAX_SCHEMA_BYTES: u64 = 1_048_576;
    let mut bytes = Vec::with_capacity(8192);
    reader
        .take(MAX_SCHEMA_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Rejection {
            code: "schema",
            detail: error.to_string(),
        })?;
    if bytes.len() > MAX_SCHEMA_BYTES as usize {
        return reject("budget", "schema bytes");
    }
    Ok(bytes)
}

fn expanded(name: &str, namespace: &str) -> String {
    format!("{{{namespace}}}{name}")
}
fn condition(root: Node<'_, '_>) -> Result<(String, String)> {
    let c = one(root, "condition")?;
    attrs(c, &[])?;
    tags(c, &["expression"])?;
    let expression = one(c, "expression")?;
    attrs(expression, &[])?;
    tags(expression, &["function"])?;
    let f = one(expression, "function")?;
    attrs(f, &["name", "library"])?;
    tags(f, &["expression"])?;
    if f.attribute("name") != Some("equal") || f.attribute("library") != Some("core") {
        return reject("condition", "exact core.equal required");
    }
    let operands = elements(f);
    if operands.len() != 2 {
        return reject("condition", "exactly two operands required");
    }
    let mut literal = None;
    let mut attribute = false;
    for operand in operands {
        attrs(operand, &[])?;
        tags(operand, &["attribute", "constant"])?;
        let parts = elements(operand);
        let [part] = parts.as_slice() else {
            return reject("condition", "exactly one operand child required");
        };
        empty(*part)?;
        match part.tag_name().name() {
            "attribute" => {
                attrs(*part, &["name", "ns"])?;
                if attribute
                    || part.attribute("name") != Some("type")
                    || part.attribute("ns") != Some(XSI)
                {
                    return reject("condition", "exact xsi:type required");
                }
                attribute = true;
            }
            "constant" => {
                attrs(*part, &["value", "datatype"])?;
                if literal.is_some() || part.attribute("datatype") != Some("QName") {
                    return reject("condition", "one QName constant required");
                }
                literal = part.attribute("value");
            }
            _ => return reject("condition", "unsupported operand"),
        }
        if part.tag_name().namespace().is_some() {
            return reject("condition", "namespaced operand");
        }
    }
    let Some(raw) = literal.filter(|_| attribute) else {
        return reject("condition", "missing operands");
    };
    Ok((raw.to_string(), canonical(raw)?))
}

fn namespace<'a>(entry: Node<'_, '_>, slots: &'a [String]) -> Result<&'a str> {
    let raw = entry.attribute("ns").unwrap_or("0");
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return reject("namespace", raw);
    }
    let slot: usize = raw.parse().map_err(|_| Rejection {
        code: "namespace",
        detail: raw.into(),
    })?;
    slots
        .get(slot)
        .map(String::as_str)
        .ok_or_else(|| Rejection {
            code: "namespace",
            detail: raw.into(),
        })
}
fn boundary(
    component: Node<'_, '_>,
    load_schema: &dyn Fn(&str) -> Result<SchemaResource>,
    qualified_saved_views: bool,
) -> Result<Boundary> {
    attrs(component, &["name", "uid", "library", "kind"])?;
    tags(component, &["properties", "view", "data"])?;
    if component.attribute("library") != Some("xml")
        || component.attribute("kind") != Some("14")
        || component
            .descendants()
            .any(|n| n.attribute("PassThrough") == Some("1") || n.attribute("usageKind").is_some())
    {
        return reject("component", "only static nonvariable XML14 boundaries");
    }
    let uid = number(component.attribute("uid"))?;
    let properties = component
        .children()
        .filter(|n| n.has_tag_name("properties"))
        .collect::<Vec<_>>();
    if properties.len() > 1 {
        return reject("component", "duplicate properties");
    }
    for p in properties {
        attrs(
            p,
            &["XSLTTargetEncoding", "XSLTDefaultOutput", "PassThrough"],
        )?;
        empty(p)?;
        if !elements(p).is_empty()
            || p.attribute("XSLTTargetEncoding")
                .is_some_and(|s| s != "UTF-8")
            || p.attribute("XSLTDefaultOutput").is_some_and(|s| s != "1")
            || p.attribute("PassThrough").is_some_and(|s| s != "0")
        {
            return reject("component", "unrepresented property behavior");
        }
    }
    let views = component
        .children()
        .filter(|n| n.has_tag_name("view"))
        .collect::<Vec<_>>();
    if views.len() > 1 {
        return reject("component", "duplicate view");
    }
    for v in views {
        attrs(v, &["ltx", "lty", "rbx", "rby"])?;
        empty(v)?;
        if !elements(v).is_empty() {
            return reject("component", "nested view control");
        }
    }
    let data = one(component, "data")?;
    attrs(data, &[])?;
    tags(data, &["root", "document"])?;
    let document = one(data, "document")?;
    attrs(
        document,
        &["schema", "instanceroot", "inputinstance", "outputinstance"],
    )?;
    empty(document)?;
    let input = document
        .attribute("inputinstance")
        .filter(|s| !s.is_empty());
    let output = document
        .attribute("outputinstance")
        .filter(|s| !s.is_empty());
    let (role, path) = match (input, output) {
        (Some(p), None) => ("source", p),
        (None, Some(p)) => ("target", p),
        _ => return reject("document", "ambiguous/missing document role"),
    };
    if path.len() > 4096
        || path.chars().any(char::is_control)
        || path.contains("://")
        || path.contains(['*', '?'])
    {
        return reject("document", "dynamic/set/remote path unsupported");
    }
    let root = one(data, "root")?;
    attrs(root, &[])?;
    tags(root, &["header", "entry"])?;
    let header = one(root, "header")?;
    attrs(header, &[])?;
    tags(header, &["namespaces"])?;
    let ns = one(header, "namespaces")?;
    attrs(ns, &[])?;
    tags(ns, &["namespace"])?;
    let mut slots = Vec::new();
    for n in elements(ns) {
        attrs(n, &["uid"])?;
        empty(n)?;
        if !elements(n).is_empty() {
            return reject("namespace", "nested namespace");
        }
        slots.push(n.attribute("uid").unwrap_or("").to_string());
    }
    let file = one(root, "entry")?;
    let wrapper = one(file, "entry")?;
    for (entry, name) in [(file, "FileInstance"), (wrapper, "document")] {
        attrs(entry, &["name", "ns", "expanded", "casttotargettypemode"])?;
        if entry.attribute("name") != Some(name)
            || namespace(entry, &slots)? != PROTOCOL
            || entry
                .attribute("casttotargettypemode")
                .is_some_and(|s| s != "cast-in-subtree")
        {
            return reject("wrapper", "exact protocol wrapper required");
        }
        tags(entry, &["entry"])?;
    }
    if elements(file).len() != 1 {
        return reject("wrapper", "multiple document wrappers");
    }
    let views = elements(wrapper);
    let [base_view, selected_view] = views.as_slice() else {
        return reject(
            "root_view",
            "exactly base and one conditional sibling required",
        );
    };
    for view in views.iter().copied() {
        attrs(
            view,
            &[
                "name",
                "ns",
                "expanded",
                "displayselectionmode",
                "outkey",
                "inpkey",
            ],
        )?;
        tags(view, &["entry", "condition"])?;
    }
    if base_view.children().any(|n| n.has_tag_name("condition")) {
        return reject("root_view", "base condition unsupported");
    }
    let (raw_type_literal, canonical_type) = condition(*selected_view)?;
    let name = selected_view.attribute("name").ok_or_else(|| Rejection {
        code: "root_view",
        detail: "root name absent".into(),
    })?;
    let declared_root = document
        .attribute("instanceroot")
        .ok_or_else(|| Rejection {
            code: "root_view",
            detail: "instance root absent".into(),
        })?;
    if declared_root != expanded(name, namespace(*selected_view, &slots)?)
        || declared_root
            != expanded(
                base_view.attribute("name").unwrap_or(""),
                namespace(*base_view, &slots)?,
            )
    {
        return reject("root_view", "root identity mismatch/nested root");
    }
    let declared_schema = document.attribute("schema").ok_or_else(|| Rejection {
        code: "schema",
        detail: "schema absent".into(),
    })?;
    if declared_schema.contains("://") || declared_schema.len() > 4096 {
        return reject("schema", "nonlocal schema");
    }
    let resource = load_schema(declared_schema)?;
    if resource.bytes.len() > 1_048_576 {
        return reject("budget", "owned schema bytes");
    }
    let schema_path = resource.path;
    let schema_owned_bytes = resource.bytes;
    let schema_text = std::str::from_utf8(&schema_owned_bytes).map_err(|e| Rejection {
        code: "schema",
        detail: e.to_string(),
    })?;
    derived_from_root(schema_text, name, &canonical_type)?;
    let mut schema = owned_string_schema(schema_text, name, declared_root, &canonical_type)?;
    let SchemaKind::Group {
        children,
        alternatives,
        dynamic,
        ..
    } = &schema.kind
    else {
        return reject("schema", "group root required");
    };
    if schema.repeating
        || dynamic.is_some()
        || !schema.xml_type_alternatives
        || schema.xml_alternative_kind != XmlAlternativeKind::XsiType
        || schema.alternative_mode() != GroupAlternativeMode::Exclusive
        || !schema.xml_repeating_sequences.is_empty()
        || !schema.xml_repeating_choices.is_empty()
        || schema.recursive_ref.is_some()
        || schema.xml_default_type.as_ref().is_none()
    {
        return reject("schema", "unsupported root schema");
    }
    if children.is_empty()
        || children.len() > 32
        || children.iter().any(|c| {
            !c.attribute
                || c.repeating
                || c.text
                || !matches!(c.kind, SchemaKind::Scalar { .. })
                || !c.xml_name_alternatives.is_empty()
        })
        || alternatives.iter().any(|a| !a.constraints.is_empty())
    {
        return reject("schema", "flat closed scalar attributes required");
    }
    let selected = alternatives
        .iter()
        .find(|a| a.name == canonical_type)
        .ok_or_else(|| Rejection {
            code: "schema_type",
            detail: "selected type not in root domain".into(),
        })?;
    let base = alternatives
        .iter()
        .find(|a| Some(&a.name) == schema.xml_default_type.as_ref())
        .ok_or_else(|| Rejection {
            code: "schema_type",
            detail: "base type missing".into(),
        })?;
    let mut fields = BTreeMap::new();
    for c in children {
        if !ncname(&c.name) || fields.insert(c.name.as_str(), c).is_some() {
            return reject("schema_field", "ambiguous local schema field");
        }
    }
    let mut pins = Vec::new();
    let expected_kind = if role == "source" { "outkey" } else { "inpkey" };
    for (ordinal, view) in views.iter().copied().enumerate() {
        let members = if ordinal == 0 {
            &base.members
        } else {
            &selected.members
        };
        let mut exposed = BTreeSet::new();
        let mut entries = vec![(view, None)];
        for leaf in view.children().filter(|n| n.has_tag_name("entry")) {
            attrs(
                leaf,
                &["name", "ns", "type", "expanded", "outkey", "inpkey"],
            )?;
            empty(leaf)?;
            if !elements(leaf).is_empty() || leaf.attribute("type") != Some("attribute") {
                return reject("schema_field", "direct scalar attribute leaf required");
            }
            let field = leaf.attribute("name").unwrap_or("");
            let Some(child) = fields.get(field) else {
                return reject("schema_field", "unresolved exposed field");
            };
            let identity = expanded(field, namespace(leaf, &slots)?);
            if identity
                != expanded(
                    &child.name,
                    child
                        .xml_namespace
                        .as_ref()
                        .and_then(ir::XmlNamespace::uri)
                        .unwrap_or(""),
                )
                || !members.iter().any(|m| m == field)
                || !exposed.insert(identity.clone())
            {
                return reject("schema_field", "ambiguous/nonmember field identity");
            }
            entries.push((leaf, Some((identity, field.to_string()))));
        }
        // The original static proof still requires full explicit membership.
        // In the measured saved profile, an empty all-mode base view is an
        // implicit schema display, not a source of fabricated leaf pins. A
        // selected view may retain only actually exposed exact member entries;
        // graph proof below still requires every used endpoint to own a raw pin.
        let complete = exposed.len() == members.len();
        let qualified_subset = qualified_saved_views
            && view.attribute("displayselectionmode")
                == if ordinal == 0 { Some("all") } else { None }
            && (ordinal == 1 || exposed.is_empty());
        if !complete && !qualified_subset {
            return reject(
                "schema_field",
                "complete explicit membership or exact qualified saved display required",
            );
        }
        for (entry, field) in entries {
            for kind in ["outkey", "inpkey"] {
                if let Some(raw) = entry.attribute(kind) {
                    if kind != expected_kind {
                        return reject("pin_role", "pin conflicts with document role");
                    }
                    pins.push(Pin {
                        raw: raw.into(),
                        key: number(Some(raw))?,
                        kind: kind.into(),
                        entry: id(entry),
                        component_uid: uid,
                        view_ordinal: ordinal,
                        expanded_field: field.as_ref().map(|x| x.0.clone()),
                        path: field
                            .as_ref()
                            .map(|x| vec![x.1.clone()])
                            .unwrap_or_default(),
                        attribute: field.is_some(),
                        connected: false,
                    });
                }
            }
        }
    }
    // No raw pin/control outside this exact payload inventory is tolerated.
    let actual = component
        .descendants()
        .filter(Node::is_element)
        .flat_map(|n| {
            ["outkey", "inpkey"]
                .into_iter()
                .filter_map(move |k| n.attribute(k).map(|_| (n.range().start, k)))
        })
        .collect::<Vec<_>>();
    if actual.len() != pins.len() {
        return reject("pin_owner", "unrepresented raw pin");
    }
    normalize_proven_boundary_namespaces(schema_text, declared_root, &canonical_type, &mut schema)?;
    Ok(Boundary {
        raw_uid: component.attribute("uid").unwrap().into(),
        uid,
        component: id(component),
        document: id(document),
        role: role.into(),
        instance_path: path.into(),
        declared_root: declared_root.into(),
        base_root: id(*base_view),
        conditioned_root: id(*selected_view),
        raw_type_literal,
        canonical_type,
        schema_path,
        schema_owned_bytes,
        schema,
        pins,
    })
}

fn account_empty_vertex(
    vertex: Node<'_, '_>,
    key: u32,
    owners: &BTreeMap<u32, Pin>,
    source_uid: u32,
) -> Result<AccountedEmptyVertex> {
    let Some(source) = owners.get(&key) else {
        return reject("endpoint", "dangling empty source");
    };
    if source.component_uid != source_uid
        || source.kind != "outkey"
        || source.view_ordinal != 1
        || !source.attribute
        || source.path.len() != 1
        || source.expanded_field.is_none()
    {
        return reject(
            "endpoint",
            "only exact selected source attribute empty vertices",
        );
    }
    Ok(AccountedEmptyVertex {
        vertex: id(vertex),
        source: source.clone(),
    })
}

fn prove_profile(
    text: &str,
    load_schema: &dyn Fn(&str) -> Result<SchemaResource>,
    qualified_saved_views: bool,
) -> Result<VerifiedRootViewPlan> {
    if text.len() > 1_048_576 {
        return reject("budget", "mapping bytes");
    }
    let doc = Document::parse(text).map_err(|e| Rejection {
        code: "xml",
        detail: e.to_string(),
    })?;
    if doc.descendants().count() > 4096 {
        return reject("budget", "mapping nodes");
    }
    let root = doc.root_element();
    if !root.has_tag_name("mapping") {
        return reject("shape", "mapping required");
    }
    attrs(root, &["version", "ferrule-primary-source"])?;
    tags(root, &["resources", "component"])?;
    if !matches!(root.attribute("version"), Some("22" | "26")) {
        return reject("version", "unsupported root profile version");
    }
    for resources in root.children().filter(|n| n.has_tag_name("resources")) {
        attrs(resources, &[])?;
        empty(resources)?;
        if !elements(resources).is_empty() {
            return reject("resources", "external resources unsupported");
        }
    }
    if root
        .children()
        .filter(|n| n.has_tag_name("resources"))
        .count()
        > 1
    {
        return reject("resources", "duplicate resources");
    }
    let owner = one(root, "component")?;
    attrs(owner, &["name", "uid", "editable", "blackbox"])?;
    number(owner.attribute("uid"))?;
    tags(owner, &["properties", "structure"])?;
    if owner.attribute("blackbox").is_some_and(|s| s != "0") {
        return reject("component", "blackbox mapping");
    }
    let properties = owner
        .children()
        .filter(|n| n.has_tag_name("properties"))
        .collect::<Vec<_>>();
    if properties.len() > 1 {
        return reject("component", "duplicate mapping properties");
    }
    for p in properties {
        attrs(p, &["SelectedLanguage"])?;
        empty(p)?;
        if !elements(p).is_empty()
            || p.attribute("SelectedLanguage")
                .is_some_and(|s| s != "builtin")
        {
            return reject("component", "nonbuiltin mapping properties");
        }
    }
    let structure = one(owner, "structure")?;
    attrs(structure, &[])?;
    tags(structure, &["children", "graph"])?;
    let children = one(structure, "children")?;
    attrs(children, &[])?;
    tags(children, &["component"])?;
    let components = elements(children);
    if components.len() != 2 {
        return reject("component", "exactly two components required");
    }
    let mut boundaries = components
        .into_iter()
        .map(|c| boundary(c, load_schema, qualified_saved_views))
        .collect::<Result<Vec<_>>>()?;
    if boundaries[0].uid == boundaries[1].uid {
        return reject("component_uid", "duplicate numeric component UID");
    }
    if boundaries
        .iter()
        .any(|b| Some(b.uid) == number(owner.attribute("uid")).ok())
    {
        return reject("component_uid", "mapping/component UID collision");
    }
    if boundaries[0].role == boundaries[1].role {
        return reject("document", "one source and one target required");
    }
    boundaries.sort_by_key(|b| b.role != "source");
    let mut target = boundaries.pop().unwrap();
    let mut source = boundaries.pop().unwrap();
    if source.canonical_type != target.canonical_type
        || source.declared_root != target.declared_root
    {
        return reject("type_pair", "one exact matching root/type pair required");
    }
    if root
        .attribute("ferrule-primary-source")
        .is_some_and(|r| number(Some(r)).ok() != Some(source.uid))
    {
        return reject("primary_owner", "primary hint mismatch");
    }
    let graph = one(structure, "graph")?;
    attrs(graph, &["directed"])?;
    tags(graph, &["edges", "vertices"])?;
    if graph.attribute("directed") != Some("1") {
        return reject("graph", "explicit directed orientation required");
    }
    let graph_edges = elements(graph)
        .into_iter()
        .filter(|n| n.has_tag_name("edges"))
        .collect::<Vec<_>>();
    if graph_edges.len() > 1
        || graph_edges
            .iter()
            .any(|n| !elements(*n).is_empty() || n.attributes().len() != 0)
    {
        return reject("graph", "unsupported top edge controls");
    }
    for edge_container in &graph_edges {
        attrs(*edge_container, &[])?;
        empty(*edge_container)?;
    }
    let vertices = one(graph, "vertices")?;
    attrs(vertices, &[])?;
    tags(vertices, &["vertex"])?;
    let mut owners = BTreeMap::new();
    for pin in source.pins.iter().chain(&target.pins) {
        if owners.insert(pin.key, pin.clone()).is_some() {
            return reject("pin_owner", "duplicate numeric raw pin key");
        }
    }
    let mut seen_vertices = BTreeSet::new();
    let mut seen_targets = BTreeSet::new();
    let mut feeds = Vec::new();
    let mut empty_vertices = Vec::new();
    let mut connected = BTreeSet::new();
    for vertex in elements(vertices) {
        attrs(vertex, &["vertexkey"])?;
        tags(vertex, &["edges"])?;
        let from = number(vertex.attribute("vertexkey"))?;
        if !seen_vertices.insert(from) {
            return reject("graph", "duplicate numeric vertex");
        }
        let edges = elements(vertex);
        if edges.len() > 1 {
            return reject("graph", "duplicate vertex edges");
        }
        if edges.is_empty() {
            empty_vertices.push(account_empty_vertex(vertex, from, &owners, source.uid)?);
            continue;
        }
        attrs(edges[0], &[])?;
        tags(edges[0], &["edge"])?;
        if elements(edges[0]).is_empty() {
            empty_vertices.push(account_empty_vertex(vertex, from, &owners, source.uid)?);
            continue;
        }
        for edge in elements(edges[0]) {
            attrs(edge, &["vertexkey"])?;
            empty(edge)?;
            if !elements(edge).is_empty() {
                return reject("graph", "edge controls unsupported");
            }
            let to = number(edge.attribute("vertexkey"))?;
            if !seen_targets.insert(to) {
                return reject("graph", "duplicate/multiple producer target");
            }
            let Some(a) = owners.get(&from) else {
                return reject("endpoint", "dangling source");
            };
            let Some(b) = owners.get(&to) else {
                return reject("endpoint", "dangling target");
            };
            if a.component_uid != source.uid
                || b.component_uid != target.uid
                || a.kind != "outkey"
                || b.kind != "inpkey"
                || a.view_ordinal != 1
                || b.view_ordinal != 1
                || !a.attribute
                || !b.attribute
            {
                return reject(
                    "endpoint",
                    "only exact conditional leaf source-to-target feeds",
                );
            }
            let sa = source.schema.child(&a.path[0]).unwrap();
            let tb = target.schema.child(&b.path[0]).unwrap();
            if sa.kind != tb.kind {
                return reject("field_domain", "scalar adaptation outside initial proof");
            }
            let mut a = a.clone();
            a.connected = true;
            let mut b = b.clone();
            b.connected = true;
            connected.extend([from, to]);
            feeds.push(Feed {
                raw_source: vertex.attribute("vertexkey").unwrap().into(),
                raw_target: edge.attribute("vertexkey").unwrap().into(),
                source: a,
                target: b,
                vertex: id(vertex),
                edge: id(edge),
            });
            if feeds.len() > 256 {
                return reject("budget", "feeds");
            }
        }
    }
    if feeds.is_empty() {
        return reject("graph", "no connected view");
    }
    for pin in source.pins.iter_mut().chain(&mut target.pins) {
        pin.connected = connected.contains(&pin.key);
    }
    Ok(VerifiedRootViewPlan {
        certificate_scope: "static ownership only; no behavior/admission/export certificate",
        raw_mapping_owned: text.into(),
        structure: id(structure),
        graph: id(graph),
        source,
        target,
        feeds,
        empty_vertices,
        required_primitives: ["SourceRootXmlTypeEquals", "SourceRootField"],
    })
}

#[cfg(test)]
mod hardening_tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn normalized_type_identity_uses_the_mapping_grammar() {
        for (raw, expected) in [("{}Derived", "Derived"), ("{urn:t}Å", "{urn:t}Å")] {
            assert_eq!(canonical(raw).unwrap(), expected);
        }
        for raw in [
            "{a b}Derived",
            "{a\nb}Derived",
            "{a\u{85}b}Derived",
            "{a}p:Derived",
        ] {
            assert_eq!(canonical(raw).unwrap_err().code, "qname");
        }
    }

    struct CountingRead<'a> {
        remaining: usize,
        consumed: &'a Cell<usize>,
    }
    impl Read for CountingRead<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            let count = bytes.len().min(self.remaining);
            bytes[..count].fill(b'x');
            self.remaining -= count;
            self.consumed.set(self.consumed.get() + count);
            Ok(count)
        }
    }

    #[test]
    fn oversized_schema_reads_stop_at_the_limit_plus_one() {
        let consumed = Cell::new(0);
        let result = bounded_schema_bytes(CountingRead {
            remaining: 16 * 1_048_576,
            consumed: &consumed,
        });
        assert_eq!(result.unwrap_err().code, "budget");
        assert_eq!(consumed.get(), 1_048_577);
    }

    #[test]
    fn exact_schema_byte_limit_and_empty_input_are_retained() {
        let consumed = Cell::new(0);
        let bytes = bounded_schema_bytes(CountingRead {
            remaining: 1_048_576,
            consumed: &consumed,
        })
        .unwrap();
        assert_eq!(bytes.len(), 1_048_576);
        assert_eq!(consumed.get(), 1_048_576);
        assert!(bounded_schema_bytes(&b""[..]).unwrap().is_empty());
    }
}
