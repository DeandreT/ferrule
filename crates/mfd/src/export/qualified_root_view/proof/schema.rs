use super::*;

fn schema_qname(node: Node<'_, '_>, raw: &str) -> Result<String> {
    let (prefix, local) = raw
        .split_once(':')
        .map_or((None, raw), |(p, l)| (Some(p), l));
    if !ncname(local) {
        return reject("schema_type", raw);
    }
    let ns = node.lookup_namespace_uri(prefix).unwrap_or("");
    if prefix.is_some() && ns.is_empty() {
        return reject("schema_type", raw);
    }
    Ok(if ns.is_empty() {
        local.to_string()
    } else {
        expanded(local, ns)
    })
}
const OWNED_ALTERNATIVES: &str = "urn:ferrule:xsd:group-alternatives";

fn xs_children<'a, 'i>(node: Node<'a, 'i>, allowed: &[&str]) -> Result<Vec<Node<'a, 'i>>> {
    if node.children().any(|n| {
        n.is_pi()
            || (n.is_text()
                && n.text().is_some_and(|text| {
                    !text
                        .bytes()
                        .all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
                }))
    }) {
        return reject("schema_shape", "unrepresented physical schema content");
    }
    let children = elements(node);
    if children
        .iter()
        .any(|n| n.tag_name().namespace() != Some(XS) || !allowed.contains(&n.tag_name().name()))
    {
        return reject("schema_shape", "unsupported physical schema child");
    }
    Ok(children)
}

fn physical_string_attributes(
    parent: Node<'_, '_>,
    target_namespace: &str,
    form_default: &str,
    all_names: &mut BTreeSet<String>,
) -> Result<Vec<SchemaNode>> {
    let nodes = xs_children(parent, &["sequence", "attribute"])?;
    let mut sequence = false;
    let mut fields = Vec::new();
    for (ordinal, node) in nodes.into_iter().enumerate() {
        if node.has_tag_name((XS, "sequence")) {
            if sequence || ordinal != 0 {
                return reject(
                    "schema_shape",
                    "one optional initial empty sequence required",
                );
            }
            sequence = true;
            attrs(node, &[])?;
            xs_children(node, &[])?;
            continue;
        }
        attrs(node, &["name", "type", "use", "form"])?;
        xs_children(node, &[])?;
        let name = node.attribute("name").ok_or_else(|| Rejection {
            code: "schema_field",
            detail: "named direct scalar attribute required".into(),
        })?;
        if !ir::primary_root_ncname_is_valid(name) || !all_names.insert(name.into()) {
            return reject(
                "schema_field",
                "invalid or duplicate physical attribute identity",
            );
        }
        if all_names.len() > 32 {
            return reject("budget", "physical attribute count");
        }
        let scalar_type = node.attribute("type").ok_or_else(|| Rejection {
            code: "schema_scalar",
            detail: "explicit physical string type required".into(),
        })?;
        if schema_qname(node, scalar_type)? != expanded("string", XS) {
            return reject(
                "schema_scalar",
                "only unrestricted XML Schema string is qualified",
            );
        }
        let form = node.attribute("form").unwrap_or(form_default);
        let namespace = match form {
            "unqualified" => ir::XmlNamespace::Unqualified,
            "qualified" if !target_namespace.is_empty() => {
                ir::XmlNamespace::qualified(target_namespace).ok_or_else(|| Rejection {
                    code: "schema_namespace",
                    detail: "physical attribute namespace is invalid".into(),
                })?
            }
            _ => return reject("schema_namespace", "invalid physical attribute form"),
        };
        if name == "xmlns" && namespace == ir::XmlNamespace::Unqualified {
            return reject(
                "schema_field",
                "unqualified xmlns is namespace syntax, not scalar data",
            );
        }
        let required = match node.attribute("use") {
            None | Some("optional") => false,
            Some("required") => true,
            _ => return reject("schema_field", "unsupported physical attribute use"),
        };
        let mut field = SchemaNode::scalar(name, ir::ScalarType::String).attribute();
        field.xml_namespace = Some(namespace);
        field.xml_attribute_required = required;
        if !field.metadata_is_valid() {
            return reject("schema_field", "invalid constructed string metadata");
        }
        fields.push(field);
    }
    Ok(fields)
}

/// XML declaration policy for this proved byte lane only. roxmltree accepts
/// UTF8 `&str` independently of the declared file encoding, so check it explicitly.
pub(super) fn utf8_schema_declaration(text: &str) -> Result<()> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = text.strip_prefix("<?xml") else {
        return Ok(());
    };
    if !rest.starts_with([' ', '\t', '\r', '\n']) {
        return Ok(());
    }
    // Document::parse already proved the declaration grammar before this call.
    let declaration = rest
        .split_once("?>")
        .ok_or_else(|| Rejection {
            code: "schema_encoding",
            detail: "unterminated XML declaration".into(),
        })?
        .0;
    let Some(position) = declaration.find("encoding") else {
        return Ok(());
    };
    let suffix =
        declaration[position + "encoding".len()..].trim_start_matches([' ', '\t', '\r', '\n']);
    let value = suffix
        .strip_prefix('=')
        .ok_or_else(|| Rejection {
            code: "schema_encoding",
            detail: "invalid XML declaration encoding".into(),
        })?
        .trim_start_matches([' ', '\t', '\r', '\n']);
    let quote = value
        .chars()
        .next()
        .filter(|ch| matches!(ch, '\'' | '"'))
        .ok_or_else(|| Rejection {
            code: "schema_encoding",
            detail: "quoted XML encoding required".into(),
        })?;
    let encoding = value[quote.len_utf8()..]
        .split_once(quote)
        .ok_or_else(|| Rejection {
            code: "schema_encoding",
            detail: "closed XML encoding required".into(),
        })?
        .0;
    if !encoding.eq_ignore_ascii_case("UTF-8") {
        return reject(
            "schema_encoding",
            "only absent or UTF-8 XML declarations are proved",
        );
    }
    Ok(())
}

/// A deliberately closed physical schema adapter. All semantic data comes from
/// this captured text; paths and the general path-based XSD importer are unused.
/// This is a string-only declaration proof, not a general XSD validator.
pub(crate) fn owned_string_schema(
    bytes: &str,
    root_name: &str,
    declared_root: &str,
    selected: &str,
) -> Result<SchemaNode> {
    if bytes.len() > 1_048_576 {
        return reject("budget", "owned physical schema bytes");
    }
    let document = Document::parse(bytes).map_err(|e| Rejection {
        code: "schema",
        detail: e.to_string(),
    })?;
    utf8_schema_declaration(bytes)?;
    if document.root().children().any(|node| node.is_pi()) {
        return reject(
            "schema_shape",
            "document processing instructions are outside the closed proof",
        );
    }
    let physical = document.root_element();
    if !physical.has_tag_name((XS, "schema")) {
        return reject("schema_shape", "XML Schema document required");
    }
    attrs(
        physical,
        &[
            "targetNamespace",
            "elementFormDefault",
            "attributeFormDefault",
        ],
    )?;
    let target_namespace = physical.attribute("targetNamespace").unwrap_or("");
    if [
        XS,
        XSI,
        "http://www.w3.org/XML/1998/namespace",
        "http://www.w3.org/2000/xmlns/",
    ]
    .contains(&target_namespace)
    {
        return reject(
            "schema_namespace",
            "reserved namespace cannot own this native string lane",
        );
    }
    if physical.attribute("targetNamespace") == Some("")
        || !ir::primary_root_ncname_is_valid(root_name)
        || !ir::primary_root_type_identity_is_valid(&if target_namespace.is_empty() {
            root_name.into()
        } else {
            expanded(root_name, target_namespace)
        })
        || declared_root != expanded(root_name, target_namespace)
        || !ir::primary_root_type_identity_is_valid(selected)
    {
        return reject(
            "schema_namespace",
            "physical root/type identity contradicts protocol",
        );
    }
    for form in ["elementFormDefault", "attributeFormDefault"] {
        if physical
            .attribute(form)
            .is_some_and(|v| !matches!(v, "qualified" | "unqualified"))
        {
            return reject("schema_namespace", "invalid physical form default");
        }
    }
    for node in physical.descendants().filter(Node::is_element) {
        if node.namespaces().any(|n| {
            ![
                XS,
                OWNED_ALTERNATIVES,
                target_namespace,
                "http://www.w3.org/XML/1998/namespace",
            ]
            .contains(&n.uri())
        }) {
            return reject(
                "schema_namespace",
                "unrepresented physical namespace binding",
            );
        }
    }
    let mut types = BTreeMap::new();
    let mut root = None;
    for node in xs_children(physical, &["complexType", "element"])? {
        if node.has_tag_name((XS, "element")) {
            if root.replace(node).is_some() {
                return reject(
                    "schema_root",
                    "one complete physical root declaration required",
                );
            }
            continue;
        }
        attrs(node, &["name"])?;
        let name = node.attribute("name").ok_or_else(|| Rejection {
            code: "schema_type",
            detail: "named complex type required".into(),
        })?;
        if !ir::primary_root_ncname_is_valid(name) {
            return reject("schema_type", "invalid physical type name");
        }
        let identity = if target_namespace.is_empty() {
            name.into()
        } else {
            expanded(name, target_namespace)
        };
        if !ir::primary_root_type_identity_is_valid(&identity)
            || types.insert(identity, node).is_some()
        {
            return reject("schema_type", "invalid or duplicate physical type identity");
        }
    }
    let root = root.ok_or_else(|| Rejection {
        code: "schema_root",
        detail: "named root declaration required".into(),
    })?;
    attrs(root, &["name", "type"])?;
    if root.attribute("name") != Some(root_name) {
        return reject("schema_root", "physical root name contradicts protocol");
    }
    let base = schema_qname(
        root,
        root.attribute("type").ok_or_else(|| Rejection {
            code: "schema_type",
            detail: "explicit named root default required".into(),
        })?,
    )?;
    if base == selected
        || types.len() != 2
        || !types.contains_key(&base)
        || !types.contains_key(selected)
    {
        return reject(
            "schema_type",
            "exact concrete default and proper selected two-type domain required",
        );
    }
    let mut names = BTreeSet::new();
    let form_default = physical
        .attribute("attributeFormDefault")
        .unwrap_or("unqualified");
    let mut fields =
        physical_string_attributes(types[&base], target_namespace, form_default, &mut names)?;
    let base_members = fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
    let content = xs_children(types[selected], &["complexContent"])?;
    let [content] = content.as_slice() else {
        return reject("schema_type", "one selected complexContent required");
    };
    attrs(*content, &[])?;
    let extension = xs_children(*content, &["extension"])?;
    let [extension] = extension.as_slice() else {
        return reject("schema_type", "one proper direct extension required");
    };
    attrs(*extension, &["base"])?;
    if schema_qname(
        *extension,
        extension.attribute("base").ok_or_else(|| Rejection {
            code: "schema_type",
            detail: "extension base is absent".into(),
        })?,
    )? != base
    {
        return reject(
            "schema_type",
            "selected extension does not own the exact default base",
        );
    }
    fields.extend(physical_string_attributes(
        *extension,
        target_namespace,
        form_default,
        &mut names,
    )?);
    if fields.is_empty() {
        return reject(
            "schema_field",
            "nonempty complete physical string field domain required",
        );
    }
    let selected_members = fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
    let mut alternative_order = vec![base.clone(), selected.into()];
    let annotations = xs_children(root, &["annotation"])?;
    if !annotations.is_empty() {
        let [annotation] = annotations.as_slice() else {
            return reject(
                "schema_annotation",
                "one owned alternative annotation required",
            );
        };
        attrs(*annotation, &[])?;
        let infos = xs_children(*annotation, &["appinfo"])?;
        let [info] = infos.as_slice() else {
            return reject(
                "schema_annotation",
                "one owned alternative appinfo required",
            );
        };
        attrs(*info, &["source"])?;
        if info.attribute("source") != Some(OWNED_ALTERNATIVES) {
            return reject(
                "schema_annotation",
                "unrepresented physical annotation source",
            );
        }
        if info.children().any(|n| {
            n.is_pi()
                || (n.is_text()
                    && n.text().is_some_and(|t| {
                        !t.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
                    }))
        }) {
            return reject("schema_annotation", "unrepresented annotation content");
        }
        let identities = elements(*info);
        if identities.len() != 2 {
            return reject("schema_annotation", "complete two-type annotation required");
        }
        let mut ordered = Vec::new();
        for identity in identities {
            if !identity.has_tag_name((OWNED_ALTERNATIVES, "type")) {
                return reject(
                    "schema_annotation",
                    "unrepresented physical annotation child",
                );
            }
            attrs(identity, &["name"])?;
            xs_children(identity, &[])?;
            let name = identity.attribute("name").ok_or_else(|| Rejection {
                code: "schema_annotation",
                detail: "owned annotation identity required".into(),
            })?;
            if (name != base && name != selected) || ordered.iter().any(|old| old == name) {
                return reject(
                    "schema_annotation",
                    "unresolved or duplicate annotation identity",
                );
            }
            ordered.push(name.into());
        }
        alternative_order = ordered;
    }
    let mut schema = SchemaNode::group(root_name, fields);
    if let SchemaKind::Group { alternatives, .. } = &mut schema.kind {
        *alternatives = alternative_order
            .into_iter()
            .map(|name| ir::GroupAlternative {
                members: if name == base {
                    base_members.clone()
                } else {
                    selected_members.clone()
                },
                name,
                required: vec![],
                constraints: vec![],
            })
            .collect();
    }
    schema.xml_type_alternatives = true;
    schema.xml_default_type = Some(base);
    schema.xml_namespace = Some(if target_namespace.is_empty() {
        ir::XmlNamespace::Unqualified
    } else {
        ir::XmlNamespace::qualified(target_namespace).ok_or_else(|| Rejection {
            code: "schema_namespace",
            detail: "invalid constructed root namespace".into(),
        })?
    });
    if !schema.metadata_is_valid() || !ir::primary_root_schema_is_supported(&schema) {
        return reject("schema", "invalid constructed closed string schema");
    }
    Ok(schema)
}

pub(super) fn derived_from_root(bytes: &str, root_name: &str, selected: &str) -> Result<()> {
    let document = Document::parse(bytes).map_err(|e| Rejection {
        code: "schema",
        detail: e.to_string(),
    })?;
    let schema = document.root_element();
    if !schema.has_tag_name((XS, "schema"))
        || schema.descendants().any(|n| {
            n.has_tag_name((XS, "include"))
                || n.has_tag_name((XS, "import"))
                || n.has_tag_name((XS, "redefine"))
        })
    {
        return reject("schema", "closed physical XSD required");
    }
    let declarations = schema
        .children()
        .filter(|n| n.has_tag_name((XS, "element")) && n.attribute("name") == Some(root_name))
        .collect::<Vec<_>>();
    let [declaration] = declarations.as_slice() else {
        return reject("schema_root", "unique named root declaration required");
    };
    let base = schema_qname(
        *declaration,
        declaration.attribute("type").ok_or_else(|| Rejection {
            code: "schema_type",
            detail: "named base type required".into(),
        })?,
    )?;
    let target_ns = schema.attribute("targetNamespace").unwrap_or("");
    let mut types = BTreeMap::new();
    for t in schema
        .children()
        .filter(|n| n.has_tag_name((XS, "complexType")))
    {
        let name = t.attribute("name").ok_or_else(|| Rejection {
            code: "schema_type",
            detail: "unnamed type".into(),
        })?;
        let key = if target_ns.is_empty() {
            name.to_string()
        } else {
            expanded(name, target_ns)
        };
        if types.insert(key, t).is_some() {
            return reject("schema_type", "duplicate type identity");
        }
    }
    if selected == base {
        return reject(
            "schema_type",
            "proper derived view required for this profile",
        );
    }
    let mut current = selected.to_string();
    let mut seen = BTreeSet::new();
    for _ in 0..8 {
        if current == base {
            return Ok(());
        }
        if !seen.insert(current.clone()) {
            return reject("schema_type", "cyclic derivation");
        }
        let Some(t) = types.get(&current) else {
            return reject("schema_type", "unrelated or unresolved type");
        };
        if t.attribute("abstract") == Some("true") {
            return reject("schema_type", "abstract selected chain unsupported");
        }
        let cs = t
            .children()
            .filter(|n| n.has_tag_name((XS, "complexContent")))
            .collect::<Vec<_>>();
        let [content] = cs.as_slice() else {
            return reject("schema_type", "supported complexContent extension required");
        };
        let parts = elements(*content);
        let [extension] = parts.as_slice() else {
            return reject("schema_type", "one extension required");
        };
        if !extension.has_tag_name((XS, "extension")) {
            return reject("schema_type", "restriction or unsupported derivation");
        }
        current = schema_qname(
            *extension,
            extension.attribute("base").ok_or_else(|| Rejection {
                code: "schema_type",
                detail: "missing base".into(),
            })?,
        )?;
    }
    reject("schema_type", "derivation budget exceeded")
}

/// This proof retains physical boundary policy independently of the importer legacy
/// representation. It never changes global format-xml None semantics. All raw
/// schema and complete display/port checks have succeeded before this is called.
pub(super) fn normalize_proven_boundary_namespaces(
    bytes: &str,
    declared_root: &str,
    selected: &str,
    schema: &mut SchemaNode,
) -> Result<()> {
    let document = Document::parse(bytes).map_err(|e| Rejection {
        code: "schema",
        detail: e.to_string(),
    })?;
    let physical = document.root_element();
    let target_ns = physical.attribute("targetNamespace").unwrap_or("");
    if physical.attribute("targetNamespace") == Some("")
        || !ir::primary_root_type_identity_is_valid(&if target_ns.is_empty() {
            schema.name.clone()
        } else {
            expanded(&schema.name, target_ns)
        })
        || declared_root != expanded(&schema.name, target_ns)
    {
        return reject(
            "schema_namespace",
            "physical root namespace contradicts protocol root",
        );
    }
    for attribute in ["elementFormDefault", "attributeFormDefault"] {
        if physical
            .attribute(attribute)
            .is_some_and(|value| !matches!(value, "qualified" | "unqualified"))
        {
            return reject(
                "schema_namespace",
                "invalid physical namespace form default",
            );
        }
    }
    // A conservative direct-declaration subset makes every normalized field's
    // origin provable. Imported unresolved refs and legacy marker fallbacks are
    // deliberately insufficient evidence for this closed profile.
    if physical.descendants().any(|n| {
        n.attributes()
            .any(|a| a.namespace() == Some("urn:ferrule:xsd:legacy-name"))
            || n.has_tag_name((XS, "attributeGroup"))
            || n.has_tag_name((XS, "anyAttribute"))
            || (n.has_tag_name((XS, "attribute"))
                && (n.attribute("ref").is_some() || n.parent() == Some(physical)))
    }) {
        return reject(
            "schema_namespace",
            "indirect or legacy physical attribute policy is unproved",
        );
    }
    for node in physical.descendants().filter(Node::is_element) {
        for name in ["type", "base"] {
            if let Some(value) = node.attribute(name) {
                schema_qname(node, value)?;
            }
        }
    }
    let form_default = physical
        .attribute("attributeFormDefault")
        .unwrap_or("unqualified");
    let mut declarations = BTreeMap::<String, String>::new();
    for attribute in physical
        .descendants()
        .filter(|n| n.has_tag_name((XS, "attribute")))
    {
        let name = attribute.attribute("name").ok_or_else(|| Rejection {
            code: "schema_namespace",
            detail: "named local attribute required".into(),
        })?;
        if !ir::primary_root_ncname_is_valid(name) {
            return reject("schema_namespace", "invalid physical attribute name");
        }
        let form = attribute.attribute("form").unwrap_or(form_default);
        let namespace = match form {
            "unqualified" => "",
            "qualified" if !target_ns.is_empty() => target_ns,
            _ => {
                return reject(
                    "schema_namespace",
                    "unproved or invalid physical attribute form",
                );
            }
        };
        if declarations
            .insert(name.into(), namespace.into())
            .is_some_and(|old| old != namespace)
        {
            return reject("schema_namespace", "ambiguous physical attribute namespace");
        }
    }
    let SchemaKind::Group {
        children,
        alternatives,
        ..
    } = &schema.kind
    else {
        return reject("schema_namespace", "physical group required");
    };
    for identity in alternatives
        .iter()
        .map(|a| a.name.as_str())
        .chain(schema.xml_default_type.as_deref())
        .chain(std::iter::once(selected))
    {
        if !ir::primary_root_type_identity_is_valid(identity) {
            return reject(
                "schema_namespace",
                "invalid canonical physical type identity",
            );
        }
        let namespace = identity
            .strip_prefix('{')
            .and_then(|s| s.split_once('}').map(|(ns, _)| ns))
            .unwrap_or("");
        if namespace != target_ns {
            return reject(
                "schema_namespace",
                "physical type namespace contradicts root namespace",
            );
        }
    }
    let matches_physical = |node: &SchemaNode, expected: &str| match &node.xml_namespace {
        None => expected.is_empty(),
        Some(ir::XmlNamespace::Unqualified) => expected.is_empty(),
        Some(ir::XmlNamespace::Qualified(uri)) => uri.as_str() == expected,
    };
    if !matches_physical(schema, target_ns) {
        return reject(
            "schema_namespace",
            "imported root namespace contradicts physical declaration",
        );
    }
    for child in children {
        let Some(namespace) = declarations.get(&child.name) else {
            return reject(
                "schema_namespace",
                "returned attribute has no direct physical declaration",
            );
        };
        if !matches_physical(child, namespace) {
            return reject(
                "schema_namespace",
                "imported attribute namespace contradicts physical declaration",
            );
        }
    }
    // Mutation starts only after the full raw/returned namespace inventory was
    // validated. Qualified nodes remain exact; only physically empty names gain
    // the explicit unqualified policy that the legacy importer had omitted.
    if target_ns.is_empty() {
        schema.xml_namespace = Some(ir::XmlNamespace::Unqualified);
    }
    if let SchemaKind::Group { children, .. } = &mut schema.kind {
        for child in children {
            if declarations[&child.name].is_empty() {
                child.xml_namespace = Some(ir::XmlNamespace::Unqualified);
            }
        }
    }
    Ok(())
}
