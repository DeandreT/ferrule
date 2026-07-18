use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{ScalarType, SchemaKind};

use super::graph::GraphBuilder;
use super::schema::{ComponentFormat, SchemaComponent, normalize_xml_entry_name, schema_node_at};
use super::scope::ScopeBuilder;
use super::source_node_function::{Definitions, Expr, instantiate_target};

const MAX_SCHEMA_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SCHEMA_DEPTH: usize = 64;
const MAX_FRACTION_DIGITS: u32 = 308;

#[derive(Clone, Copy)]
enum TypeFilter {
    Any,
    String,
    Numeric,
    Int,
    Float,
    Bool,
}

impl TypeFilter {
    fn accepts(self, ty: ScalarType) -> bool {
        match self {
            Self::Any => true,
            Self::String => ty == ScalarType::String,
            Self::Numeric => matches!(ty, ScalarType::Int | ScalarType::Float),
            Self::Int => ty == ScalarType::Int,
            Self::Float => ty == ScalarType::Float,
            Self::Bool => ty == ScalarType::Bool,
        }
    }
}

#[derive(Clone)]
struct Rule {
    filter: TypeFilter,
    expression: Expr,
}

struct AppliedRule {
    path: Vec<String>,
    rule: Rule,
}

pub(super) fn install(
    mapping: &roxmltree::Node<'_, '_>,
    target: &SchemaComponent,
    structure: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) {
    let definitions =
        super::source_node_function::read_target_definitions(mapping, builder.udf_registry);
    if definitions.is_empty() {
        return;
    }
    let Some(component) = structure
        .descendants()
        .filter(|node| node.has_tag_name("component"))
        .find(|component| owns_target_port(*component, target))
    else {
        return;
    };
    let Some(root) = component
        .children()
        .find(|node| node.has_tag_name("data"))
        .and_then(|data| data.children().find(|node| node.has_tag_name("root")))
    else {
        return;
    };

    let mut rules = Vec::new();
    for entry in root.children().filter(|node| node.has_tag_name("entry")) {
        collect(entry, target, &definitions, &[], &[], &mut rules);
    }
    let paths = rules
        .iter()
        .map(|rule| rule.path.clone())
        .collect::<BTreeSet<_>>();
    let fraction_digits = read_fraction_digits(component, mfd_path, target, &paths);
    for applied in rules {
        install_one(applied, target, &fraction_digits, builder, scopes);
    }
}

fn owns_target_port(component: roxmltree::Node<'_, '_>, target: &SchemaComponent) -> bool {
    component
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|entry| super::schema::parse_u32(entry.attribute("inpkey")))
        .any(|key| target.input_keys.contains(&key) || target.ports.contains_key(&key))
}

fn collect(
    entry: roxmltree::Node<'_, '_>,
    target: &SchemaComponent,
    definitions: &Definitions,
    parent: &[String],
    inherited: &[Rule],
    output: &mut Vec<AppliedRule>,
) {
    let (name, _) = normalize_xml_entry_name(entry.attribute("name").unwrap_or_default());
    let wrapper = matches!(name, "FileInstance" | "document")
        || parent.is_empty()
            && (name == target.schema.name
                || matches!(target.format, ComponentFormat::Csv | ComponentFormat::Xlsx)
                    && entry.children().any(|child| child.has_tag_name("entry")));
    let mut path = parent.to_vec();
    if !wrapper && !name.is_empty() {
        path.push(name.to_string());
    }

    let functions = entry
        .children()
        .find(|node| node.has_tag_name("inputnodefunctions"));
    let active = if functions.and_then(|node| node.attribute("inherit")) == Some("block") {
        Vec::new()
    } else {
        inherited.to_vec()
    };
    let direct = functions
        .into_iter()
        .flat_map(|node| node.children().filter(|child| child.has_tag_name("rule")))
        .filter_map(|rule| read_rule(rule, definitions))
        .collect::<Vec<_>>();

    let mut current = active.clone();
    current.extend(
        direct
            .iter()
            .filter(|(apply_to, _)| apply_to == "self")
            .map(|(_, rule)| rule.clone()),
    );
    if matches!(
        schema_node_at(&target.schema, &path).map(|node| &node.kind),
        Some(SchemaKind::Scalar { .. })
    ) {
        output.extend(current.into_iter().map(|rule| AppliedRule {
            path: path.clone(),
            rule,
        }));
    }

    let mut descendants = active;
    descendants.extend(
        direct
            .into_iter()
            .filter(|(apply_to, _)| apply_to == "descendants")
            .map(|(_, rule)| rule),
    );
    for child in entry.children().filter(|node| node.has_tag_name("entry")) {
        collect(child, target, definitions, &path, &descendants, output);
    }
}

fn read_rule(rule: roxmltree::Node<'_, '_>, definitions: &Definitions) -> Option<(String, Rule)> {
    let function = rule.children().find(|node| node.has_tag_name("function"))?;
    let expression = definitions.get(function.attribute("name")?)?.clone();
    let filter = match rule
        .children()
        .find(|node| node.has_tag_name("filter"))
        .and_then(|filter| filter.attribute("datatype"))
    {
        Some(datatype) => type_filter(datatype)?,
        None => TypeFilter::Any,
    };
    Some((
        rule.attribute("applyto").unwrap_or("self").to_string(),
        Rule { filter, expression },
    ))
}

fn type_filter(value: &str) -> Option<TypeFilter> {
    match value {
        "anySimpleType" => Some(TypeFilter::Any),
        "string" => Some(TypeFilter::String),
        "numeric" | "number" => Some(TypeFilter::Numeric),
        "integer" | "int" | "long" => Some(TypeFilter::Int),
        "decimal" | "double" | "float" => Some(TypeFilter::Float),
        "boolean" => Some(TypeFilter::Bool),
        _ => None,
    }
}

fn install_one(
    applied: AppliedRule,
    target: &SchemaComponent,
    fraction_digits: &BTreeMap<Vec<String>, u32>,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) {
    let Some(SchemaKind::Scalar { ty }) =
        schema_node_at(&target.schema, &applied.path).map(|node| &node.kind)
    else {
        return;
    };
    if !applied.rule.filter.accepts(*ty) {
        return;
    }
    let Some((field, chain)) = applied.path.split_last() else {
        return;
    };
    let Some(scope) = existing_scope(&mut scopes.root, chain) else {
        return;
    };
    let Some(binding) = scope
        .bindings
        .iter_mut()
        .find(|binding| binding.target_field == *field)
    else {
        return;
    };
    binding.node = instantiate_target(
        &applied.rule.expression,
        binding.node,
        *ty,
        fraction_digits.get(&applied.path).copied(),
        builder,
    );
}

fn existing_scope<'a>(
    mut scope: &'a mut mapping::Scope,
    chain: &[String],
) -> Option<&'a mut mapping::Scope> {
    for field in chain {
        let index = scope
            .children
            .iter()
            .position(|child| child.target_field == *field)?;
        scope = &mut scope.children[index];
    }
    Some(scope)
}

fn read_fraction_digits(
    component: roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    target: &SchemaComponent,
    paths: &BTreeSet<Vec<String>>,
) -> BTreeMap<Vec<String>, u32> {
    let schema_reference = component
        .descendants()
        .find(|node| node.has_tag_name("document"))
        .and_then(|document| document.attribute("schema"));
    let Some(schema_path) = schema_reference.and_then(|reference| {
        super::schema::resolve_xml_schema_reference(mfd_path, reference).ok()
    }) else {
        return BTreeMap::new();
    };
    if std::fs::metadata(&schema_path)
        .ok()
        .is_none_or(|metadata| metadata.len() > MAX_SCHEMA_BYTES)
    {
        return BTreeMap::new();
    }
    let Some(text) = std::fs::read_to_string(schema_path).ok() else {
        return BTreeMap::new();
    };
    let Some(document) = roxmltree::Document::parse(&text).ok() else {
        return BTreeMap::new();
    };
    let schema = document.root_element();
    paths
        .iter()
        .filter_map(|path| {
            fraction_digits_for_path(schema, &target.schema.name, path)
                .map(|digits| (path.clone(), digits))
        })
        .collect()
}

fn fraction_digits_for_path(
    schema: roxmltree::Node<'_, '_>,
    root_name: &str,
    path: &[String],
) -> Option<u32> {
    let mut element = top_level_element(schema, root_name)?;
    for segment in path {
        element = child_element(schema, element, segment, 0)?;
    }
    element_fraction_digits(schema, element, &mut BTreeSet::new(), 0)
}

pub(super) fn path_requires_datetime(
    schema: roxmltree::Node<'_, '_>,
    root_name: &str,
    path: &[String],
) -> bool {
    let Some(mut element) = top_level_element(schema, root_name) else {
        return false;
    };
    for segment in path {
        let Some(child) = child_element(schema, element, segment, 0) else {
            return false;
        };
        element = child;
    }
    element_requires_datetime(schema, element, &mut BTreeSet::new(), 0)
}

fn element_requires_datetime(
    schema: roxmltree::Node<'_, '_>,
    element: roxmltree::Node<'_, '_>,
    active: &mut BTreeSet<String>,
    depth: usize,
) -> bool {
    if depth >= MAX_SCHEMA_DEPTH {
        return false;
    }
    let Some(element) = resolve_element(schema, element) else {
        return false;
    };
    if let Some(simple) = element
        .children()
        .find(|node| node.has_tag_name("simpleType"))
    {
        return simple_requires_datetime(schema, simple, active, depth + 1);
    }
    let Some(name) = element.attribute("type").map(local_name) else {
        return false;
    };
    let Some(simple) = named_type(schema, "simpleType", name) else {
        return name == "dateTime";
    };
    if !active.insert(name.to_string()) {
        return false;
    }
    let result = simple_requires_datetime(schema, simple, active, depth + 1);
    active.remove(name);
    result
}

fn simple_requires_datetime(
    schema: roxmltree::Node<'_, '_>,
    simple: roxmltree::Node<'_, '_>,
    active: &mut BTreeSet<String>,
    depth: usize,
) -> bool {
    if depth >= MAX_SCHEMA_DEPTH {
        return false;
    }
    let Some(base) = simple
        .children()
        .find(|node| node.has_tag_name("restriction"))
        .and_then(|restriction| restriction.attribute("base"))
        .map(local_name)
    else {
        return false;
    };
    let Some(parent) = named_type(schema, "simpleType", base) else {
        return base == "dateTime";
    };
    if !active.insert(base.to_string()) {
        return false;
    }
    let result = simple_requires_datetime(schema, parent, active, depth + 1);
    active.remove(base);
    result
}

fn top_level_element<'a, 'input>(
    schema: roxmltree::Node<'a, 'input>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    schema.children().find(|node| {
        node.has_tag_name("element")
            && node
                .attribute("name")
                .is_some_and(|candidate| local_name(candidate) == local_name(name))
    })
}

fn resolve_element<'a, 'input>(
    schema: roxmltree::Node<'a, 'input>,
    element: roxmltree::Node<'a, 'input>,
) -> Option<roxmltree::Node<'a, 'input>> {
    element
        .attribute("ref")
        .and_then(|reference| top_level_element(schema, local_name(reference)))
        .or(Some(element))
}

fn child_element<'a, 'input>(
    schema: roxmltree::Node<'a, 'input>,
    element: roxmltree::Node<'a, 'input>,
    name: &str,
    depth: usize,
) -> Option<roxmltree::Node<'a, 'input>> {
    if depth >= MAX_SCHEMA_DEPTH {
        return None;
    }
    let element = resolve_element(schema, element)?;
    let complex = element
        .children()
        .find(|node| node.has_tag_name("complexType"))
        .or_else(|| {
            let ty = local_name(element.attribute("type")?);
            named_type(schema, "complexType", ty)
        })?;
    find_complex_child(schema, complex, name, depth + 1)
}

fn find_complex_child<'a, 'input>(
    schema: roxmltree::Node<'a, 'input>,
    complex: roxmltree::Node<'a, 'input>,
    name: &str,
    depth: usize,
) -> Option<roxmltree::Node<'a, 'input>> {
    if depth >= MAX_SCHEMA_DEPTH {
        return None;
    }
    if let Some(extension) = complex
        .children()
        .find(|node| node.has_tag_name("complexContent"))
        .and_then(|content| {
            content
                .children()
                .find(|node| node.has_tag_name("extension"))
        })
    {
        if let Some(child) = find_particle_child(extension, name) {
            return Some(child);
        }
        let base = local_name(extension.attribute("base")?);
        return find_complex_child(
            schema,
            named_type(schema, "complexType", base)?,
            name,
            depth + 1,
        );
    }
    find_particle_child(complex, name)
}

fn find_particle_child<'a, 'input>(
    container: roxmltree::Node<'a, 'input>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    for child in container.children().filter(roxmltree::Node::is_element) {
        if child.has_tag_name("element") {
            let declared = child
                .attribute("name")
                .or_else(|| child.attribute("ref"))
                .map(local_name);
            if declared == Some(local_name(name)) {
                return Some(child);
            }
        } else if matches!(child.tag_name().name(), "sequence" | "choice" | "all")
            && let Some(found) = find_particle_child(child, name)
        {
            return Some(found);
        }
    }
    None
}

fn element_fraction_digits<'a, 'input>(
    schema: roxmltree::Node<'a, 'input>,
    element: roxmltree::Node<'a, 'input>,
    active: &mut BTreeSet<String>,
    depth: usize,
) -> Option<u32> {
    if depth >= MAX_SCHEMA_DEPTH {
        return None;
    }
    let element = resolve_element(schema, element)?;
    if let Some(simple) = element
        .children()
        .find(|node| node.has_tag_name("simpleType"))
    {
        return simple_fraction_digits(schema, simple, active, depth + 1);
    }
    let name = local_name(element.attribute("type")?).to_string();
    let simple = named_type(schema, "simpleType", &name)?;
    if !active.insert(name.clone()) {
        return None;
    }
    let result = simple_fraction_digits(schema, simple, active, depth + 1);
    active.remove(&name);
    result
}

fn simple_fraction_digits<'a, 'input>(
    schema: roxmltree::Node<'a, 'input>,
    simple: roxmltree::Node<'a, 'input>,
    active: &mut BTreeSet<String>,
    depth: usize,
) -> Option<u32> {
    if depth >= MAX_SCHEMA_DEPTH {
        return None;
    }
    let restriction = simple
        .children()
        .find(|node| node.has_tag_name("restriction"))?;
    if let Some(digits) = restriction
        .children()
        .find(|node| node.has_tag_name("fractionDigits"))
        .and_then(|facet| facet.attribute("value"))
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|digits| *digits <= MAX_FRACTION_DIGITS)
    {
        return Some(digits);
    }
    let base = local_name(restriction.attribute("base")?).to_string();
    let parent = named_type(schema, "simpleType", &base)?;
    if !active.insert(base.clone()) {
        return None;
    }
    let result = simple_fraction_digits(schema, parent, active, depth + 1);
    active.remove(&base);
    result
}

fn named_type<'a, 'input>(
    schema: roxmltree::Node<'a, 'input>,
    tag: &str,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    schema.children().find(|node| {
        node.has_tag_name(tag)
            && node
                .attribute("name")
                .is_some_and(|candidate| local_name(candidate) == local_name(name))
    })
}

fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_inherited_named_fraction_digits_by_path() {
        let document = roxmltree::Document::parse(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
              <xs:simpleType name="Money"><xs:restriction base="xs:decimal"><xs:fractionDigits value="2"/></xs:restriction></xs:simpleType>
              <xs:complexType name="Base"><xs:sequence><xs:element name="Amount" type="Money"/></xs:sequence></xs:complexType>
              <xs:complexType name="Record"><xs:complexContent><xs:extension base="Base"><xs:sequence><xs:element name="Label" type="xs:string"/></xs:sequence></xs:extension></xs:complexContent></xs:complexType>
              <xs:element name="Root" type="Record"/>
            </xs:schema>"#,
        )
        .unwrap();
        assert_eq!(
            fraction_digits_for_path(document.root_element(), "Root", &["Amount".to_string()]),
            Some(2)
        );
        assert_eq!(
            fraction_digits_for_path(document.root_element(), "Root", &["Label".to_string()]),
            None
        );
    }

    #[test]
    fn resolves_builtin_and_restricted_datetime_leaves_by_path() {
        let document = roxmltree::Document::parse(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
              <xs:simpleType name="Timestamp"><xs:restriction base="xs:dateTime"/></xs:simpleType>
              <xs:element name="Root"><xs:complexType><xs:sequence>
                <xs:element name="Direct" type="xs:dateTime"/>
                <xs:element name="Derived" type="Timestamp"/>
                <xs:element name="Date" type="xs:date"/>
              </xs:sequence></xs:complexType></xs:element>
            </xs:schema>"#,
        )
        .unwrap();
        let schema = document.root_element();
        assert!(path_requires_datetime(
            schema,
            "Root",
            &["Direct".to_string()]
        ));
        assert!(path_requires_datetime(
            schema,
            "Root",
            &["Derived".to_string()]
        ));
        assert!(!path_requires_datetime(
            schema,
            "Root",
            &["Date".to_string()]
        ));
    }
}
