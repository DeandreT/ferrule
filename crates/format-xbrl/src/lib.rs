//! Bounded projection of XBRL facts into a mapping table schema.
//!
//! This crate intentionally does not evaluate taxonomy formulae or render
//! presentation linkbases. It projects one imported table by grouping facts
//! on `contextRef`, which is sufficient for mappings whose connected entry
//! tree names the period fields and concrete concepts it consumes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value, XML_TEXT_FIELD};
use mapping::XbrlFactType;

pub use error::XbrlFormatError;
use render::{Prefixes, expanded_qname, is_structural_namespace, render_target};

mod error;
mod render;

const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_CONTEXTS: usize = 100_000;
const MAX_FACTS: usize = 1_000_000;
const MAX_SCHEMA_DEPTH: usize = 256;

const XBRLI: &str = "http://www.xbrl.org/2003/instance";
const XBRLDI: &str = "http://xbrl.org/2006/xbrldi";
const LINK: &str = "http://www.xbrl.org/2003/linkbase";
const MAPFORCE_VIEW: &str = "http://www.altova.com/mapforce";

/// Writes a table-shaped target instance as an XBRL instance document.
pub fn write(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &mapping::XbrlBoundaryOptions,
) -> Result<(), XbrlFormatError> {
    std::fs::write(path, to_string(schema, instance, options)?)?;
    Ok(())
}

/// In-memory form of [`write`].
pub fn to_string(
    schema: &SchemaNode,
    instance: &Instance,
    options: &mapping::XbrlBoundaryOptions,
) -> Result<String, XbrlFormatError> {
    let namespaces = namespace_map(options);
    let fact_types = options
        .fact_bindings()
        .iter()
        .map(|binding| (binding.path().to_vec(), binding.fact_type()))
        .collect::<BTreeMap<_, _>>();
    let defaults = target_defaults(schema, instance, &namespaces, &fact_types)?;
    let context = TargetContext {
        namespaces: &namespaces,
        fact_types: &fact_types,
        defaults: &defaults,
    };
    let mut rows = Vec::new();
    collect_target_rows(schema, instance, &mut Vec::new(), &context, &mut rows, 0)?;
    if rows.is_empty() {
        return Err(XbrlFormatError::InvalidTableSchema);
    }

    let mut prefix_uris = BTreeSet::new();
    for row in &rows {
        for fact in &row.facts {
            prefix_uris.insert(fact.namespace.clone());
        }
        for dimension in &row.dimensions {
            prefix_uris.insert(dimension.namespace.clone());
            if let Some((namespace, _)) = expanded_qname(&dimension.member) {
                prefix_uris.insert(namespace.to_string());
            }
        }
    }
    for unit in &defaults.units {
        for measure in unit.measures() {
            if let Some((namespace, _)) = expanded_qname(measure) {
                prefix_uris.insert(namespace.to_string());
            }
        }
    }
    let prefixes = Prefixes::new(prefix_uris);
    render_target(options.taxonomy(), &rows, &defaults.units, &prefixes)
}

fn namespace_map(options: &mapping::XbrlBoundaryOptions) -> BTreeMap<Vec<String>, String> {
    options
        .namespace_bindings()
        .iter()
        .map(|binding| (binding.path().to_vec(), binding.namespace().to_string()))
        .collect()
}

#[derive(Default)]
struct TargetDefaults {
    identifier: Option<EntityIdentifier>,
    monetary: FactDefaults,
    numeric: FactDefaults,
    shares: FactDefaults,
    per_share: FactDefaults,
    units: Vec<Unit>,
}

#[derive(Default)]
struct FactDefaults {
    unit_ref: Option<String>,
    decimals: Option<String>,
}

impl TargetDefaults {
    fn fact(&self, fact_type: XbrlFactType) -> &FactDefaults {
        match fact_type {
            XbrlFactType::Monetary => &self.monetary,
            XbrlFactType::Numeric => &self.numeric,
            XbrlFactType::Shares => &self.shares,
            XbrlFactType::PerShare => &self.per_share,
        }
    }

    fn fact_mut(&mut self, fact_type: XbrlFactType) -> &mut FactDefaults {
        match fact_type {
            XbrlFactType::Monetary => &mut self.monetary,
            XbrlFactType::Numeric => &mut self.numeric,
            XbrlFactType::Shares => &mut self.shares,
            XbrlFactType::PerShare => &mut self.per_share,
        }
    }
}

struct TargetContext<'a> {
    namespaces: &'a BTreeMap<Vec<String>, String>,
    fact_types: &'a BTreeMap<Vec<String>, XbrlFactType>,
    defaults: &'a TargetDefaults,
}

#[derive(Clone)]
struct EntityIdentifier {
    value: String,
    scheme: String,
}

#[derive(Default)]
struct Period {
    start: Option<String>,
    end: Option<String>,
    instant: Option<String>,
    forever: bool,
}

struct Dimension {
    namespace: String,
    name: String,
    member: String,
}

struct Fact {
    namespace: String,
    name: String,
    value: String,
    unit_ref: Option<String>,
    decimals: Option<String>,
}

struct TargetRow {
    path: String,
    identifier: EntityIdentifier,
    period: Period,
    dimensions: Vec<Dimension>,
    facts: Vec<Fact>,
}

#[derive(Default)]
struct Unit {
    id: String,
    measure: Option<String>,
    numerator: Option<String>,
    denominator: Option<String>,
}

impl Unit {
    fn measures(&self) -> impl Iterator<Item = &str> {
        self.measure
            .iter()
            .chain(self.numerator.iter())
            .chain(self.denominator.iter())
            .map(String::as_str)
    }
}

fn target_defaults(
    schema: &SchemaNode,
    instance: &Instance,
    namespaces: &BTreeMap<Vec<String>, String>,
    fact_types: &BTreeMap<Vec<String>, XbrlFactType>,
) -> Result<TargetDefaults, XbrlFormatError> {
    let mut defaults = TargetDefaults::default();
    visit_target(
        schema,
        instance,
        &mut Vec::new(),
        0,
        &mut |schema, instance, path| {
            if schema.name == "identifier"
                && namespace_at(namespaces, path) == Some(XBRLI)
                && defaults.identifier.is_none()
            {
                defaults.identifier = entity_identifier(schema, instance);
            }
            if let Some(fact_type) = default_fact_type(&schema.name) {
                let fact = defaults.fact_mut(fact_type);
                fact.unit_ref = scalar_descendant(schema, instance, "unitRef");
                fact.decimals = scalar_descendant(schema, instance, "decimals");
            }
            if (schema.name == "unit" && namespace_at(namespaces, path) == Some(XBRLI))
                || schema.name.starts_with(mapping::XBRL_UNIT_FIELD_PREFIX)
            {
                defaults.units.push(unit_value(schema, instance));
            }
            Ok(())
        },
    )?;
    normalize_units(&mut defaults.units)?;
    if fact_types
        .values()
        .any(|fact_type| *fact_type == XbrlFactType::PerShare)
        && !defaults
            .units
            .iter()
            .any(|unit| unit_fact_type(unit) == Some(XbrlFactType::PerShare))
    {
        let numerators = defaults
            .units
            .iter()
            .filter(|unit| unit_fact_type(unit) == Some(XbrlFactType::Monetary))
            .filter_map(|unit| unit.measure.clone())
            .collect::<Vec<_>>();
        match numerators.as_slice() {
            [numerator] => defaults.units.push(Unit {
                numerator: Some(numerator.clone()),
                denominator: Some(format!("{{{XBRLI}}}shares")),
                ..Unit::default()
            }),
            [] => {}
            _ => {
                return Err(XbrlFormatError::AmbiguousFactUnit {
                    fact_type: XbrlFactType::PerShare,
                });
            }
        }
        normalize_units(&mut defaults.units)?;
    }
    for fact_type in [
        XbrlFactType::Monetary,
        XbrlFactType::Numeric,
        XbrlFactType::Shares,
        XbrlFactType::PerShare,
    ] {
        if defaults.fact(fact_type).unit_ref.is_some()
            || !fact_types.values().any(|candidate| *candidate == fact_type)
        {
            continue;
        }
        let candidates = defaults
            .units
            .iter()
            .filter(|unit| unit_fact_type(unit) == Some(fact_type))
            .map(|unit| unit.id.clone())
            .collect::<Vec<_>>();
        match candidates.as_slice() {
            [unit_ref] => defaults.fact_mut(fact_type).unit_ref = Some(unit_ref.clone()),
            [] => {}
            _ => return Err(XbrlFormatError::AmbiguousFactUnit { fact_type }),
        }
    }
    Ok(defaults)
}

fn default_fact_type(name: &str) -> Option<XbrlFactType> {
    match name {
        "monetaryItemType" | "monetaryItemTypeNegative" => Some(XbrlFactType::Monetary),
        "numericItemType" => Some(XbrlFactType::Numeric),
        "sharesItemType" => Some(XbrlFactType::Shares),
        "perShareItemType" => Some(XbrlFactType::PerShare),
        _ => None,
    }
}

fn collect_target_rows(
    schema: &SchemaNode,
    instance: &Instance,
    path: &mut Vec<String>,
    context: &TargetContext<'_>,
    rows: &mut Vec<TargetRow>,
    depth: usize,
) -> Result<(), XbrlFormatError> {
    check_depth(depth)?;
    if schema.repeating {
        let Instance::Repeated(items) = instance else {
            return Err(shape(path));
        };
        for item in items {
            let mut row = TargetRow {
                path: path.join("/"),
                identifier: context
                    .defaults
                    .identifier
                    .clone()
                    .unwrap_or(EntityIdentifier {
                        value: String::new(),
                        scheme: String::new(),
                    }),
                period: Period::default(),
                dimensions: Vec::new(),
                facts: Vec::new(),
            };
            collect_row_values(schema, item, path, context, &mut row, depth + 1)?;
            if row.facts.is_empty() {
                continue;
            }
            if row.identifier.value.is_empty()
                || row.identifier.scheme.is_empty()
                || !valid_period(&row.period)
            {
                return Err(XbrlFormatError::MissingContext {
                    path: row.path.clone(),
                });
            }
            rows.push(row);
        }
        return Ok(());
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Ok(());
    };
    let Instance::Group(fields) = instance else {
        return Err(shape(path));
    };
    for (index, child) in children.iter().enumerate() {
        let Some(child_instance) = instance_child(fields, children, index) else {
            continue;
        };
        path.push(child.name.clone());
        collect_target_rows(child, child_instance, path, context, rows, depth + 1)?;
        path.pop();
    }
    Ok(())
}

fn collect_row_values(
    schema: &SchemaNode,
    instance: &Instance,
    path: &mut Vec<String>,
    context: &TargetContext<'_>,
    row: &mut TargetRow,
    depth: usize,
) -> Result<(), XbrlFormatError> {
    check_depth(depth)?;
    match &schema.kind {
        SchemaKind::Scalar { .. } => {
            if schema.attribute || schema.text {
                return Ok(());
            }
            let Some(value) = scalar_value(instance) else {
                return Ok(());
            };
            let namespace = namespace_at(context.namespaces, path).ok_or_else(|| {
                XbrlFormatError::MissingFactNamespace {
                    path: path.join("/"),
                }
            })?;
            if !is_structural_namespace(namespace) {
                let (unit_ref, decimals) =
                    if let Some(fact_type) = context.fact_types.get(path).copied() {
                        let fact_defaults = context.defaults.fact(fact_type);
                        let unit_ref = fact_defaults.unit_ref.clone().ok_or_else(|| {
                            XbrlFormatError::MissingFactUnit {
                                path: path.join("/"),
                                fact_type,
                            }
                        })?;
                        if !context
                            .defaults
                            .units
                            .iter()
                            .any(|unit| unit.id == unit_ref)
                        {
                            return Err(XbrlFormatError::MissingFactUnit {
                                path: path.join("/"),
                                fact_type,
                            });
                        }
                        (Some(unit_ref), fact_defaults.decimals.clone())
                    } else {
                        (None, None)
                    };
                row.facts.push(Fact {
                    namespace: namespace.to_string(),
                    name: local_name(&schema.name).to_string(),
                    value,
                    unit_ref,
                    decimals,
                });
            }
        }
        SchemaKind::Group { children, .. } => {
            let Instance::Group(fields) = instance else {
                return Err(shape(path));
            };
            let namespace = namespace_at(context.namespaces, path);
            if schema.name == "identifier"
                && namespace == Some(XBRLI)
                && let Some(identifier) = entity_identifier(schema, instance)
            {
                row.identifier = identifier;
                return Ok(());
            }
            if schema.name == "period" && namespace == Some(XBRLI) && is_period_group(schema) {
                row.period = period_value(schema, instance);
                return Ok(());
            }
            if let Some(member) = direct_scalar_child(schema, instance, "explicitMember")
                && let Some(namespace) = namespace
            {
                row.dimensions.push(Dimension {
                    namespace: namespace.to_string(),
                    name: schema.name.clone(),
                    member,
                });
            }
            for (index, child) in children.iter().enumerate() {
                let Some(child_instance) = instance_child(fields, children, index) else {
                    continue;
                };
                path.push(child.name.clone());
                collect_row_values(child, child_instance, path, context, row, depth + 1)?;
                path.pop();
            }
        }
    }
    Ok(())
}

fn visit_target(
    schema: &SchemaNode,
    instance: &Instance,
    path: &mut Vec<String>,
    depth: usize,
    visitor: &mut impl FnMut(&SchemaNode, &Instance, &[String]) -> Result<(), XbrlFormatError>,
) -> Result<(), XbrlFormatError> {
    check_depth(depth)?;
    visitor(schema, instance, path)?;
    let (SchemaKind::Group { children, .. }, Instance::Group(fields)) = (&schema.kind, instance)
    else {
        return Ok(());
    };
    for (index, child) in children.iter().enumerate() {
        if child.repeating {
            continue;
        }
        let Some(child_instance) = instance_child(fields, children, index) else {
            continue;
        };
        path.push(child.name.clone());
        visit_target(child, child_instance, path, depth + 1, visitor)?;
        path.pop();
    }
    Ok(())
}

fn instance_child<'a>(
    fields: &'a [(String, Instance)],
    schemas: &[SchemaNode],
    schema_index: usize,
) -> Option<&'a Instance> {
    let schema = &schemas[schema_index];
    let occurrence = schemas[..schema_index]
        .iter()
        .filter(|other| other.name == schema.name)
        .count();
    fields
        .iter()
        .filter(|(name, _)| name == &schema.name)
        .nth(occurrence)
        .map(|(_, value)| value)
}

fn entity_identifier(schema: &SchemaNode, instance: &Instance) -> Option<EntityIdentifier> {
    let value = direct_scalar_child(schema, instance, XML_TEXT_FIELD)?;
    let scheme = direct_scalar_child(schema, instance, "scheme")?;
    Some(EntityIdentifier { value, scheme })
}

fn is_period_group(schema: &SchemaNode) -> bool {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return false;
    };
    children.iter().any(|child| {
        matches!(
            child.name.as_str(),
            "startDate" | "endDate" | "instant" | "forever"
        )
    })
}

fn period_value(schema: &SchemaNode, instance: &Instance) -> Period {
    Period {
        start: scalar_descendant(schema, instance, "startDate"),
        end: scalar_descendant(schema, instance, "endDate"),
        instant: scalar_descendant(schema, instance, "instant"),
        forever: scalar_descendant(schema, instance, "forever").is_some(),
    }
}

fn valid_period(period: &Period) -> bool {
    (period.start.is_some() && period.end.is_some())
        || (period.start.is_none() && period.end.is_none() && period.instant.is_some())
        || (period.start.is_none()
            && period.end.is_none()
            && period.instant.is_none()
            && period.forever)
}

fn unit_value(schema: &SchemaNode, instance: &Instance) -> Unit {
    Unit {
        id: direct_scalar_child(schema, instance, "id").unwrap_or_default(),
        measure: direct_scalar_child(schema, instance, "measure"),
        numerator: child_scalar_path(schema, instance, &["divide", "unitNumerator", "measure"]),
        denominator: child_scalar_path(schema, instance, &["divide", "unitDenominator", "measure"]),
    }
}

fn child_scalar_path(
    mut schema: &SchemaNode,
    mut instance: &Instance,
    path: &[&str],
) -> Option<String> {
    for segment in path {
        let SchemaKind::Group { children, .. } = &schema.kind else {
            return None;
        };
        let Instance::Group(fields) = instance else {
            return None;
        };
        let index = children.iter().position(|child| child.name == *segment)?;
        schema = &children[index];
        instance = instance_child(fields, children, index)?;
    }
    scalar_value(instance)
}

fn scalar_descendant(schema: &SchemaNode, instance: &Instance, name: &str) -> Option<String> {
    if schema.name == name && matches!(schema.kind, SchemaKind::Scalar { .. }) {
        return scalar_value(instance);
    }
    let (SchemaKind::Group { children, .. }, Instance::Group(fields)) = (&schema.kind, instance)
    else {
        return None;
    };
    children.iter().enumerate().find_map(|(index, child)| {
        let value = instance_child(fields, children, index)?;
        scalar_descendant(child, value, name)
    })
}

fn direct_scalar_child(schema: &SchemaNode, instance: &Instance, name: &str) -> Option<String> {
    let (SchemaKind::Group { children, .. }, Instance::Group(fields)) = (&schema.kind, instance)
    else {
        return None;
    };
    children.iter().enumerate().find_map(|(index, child)| {
        if child.name != name {
            return None;
        }
        let value = instance_child(fields, children, index)?;
        scalar_descendant(child, value, name)
            .or_else(|| scalar_descendant(child, value, XML_TEXT_FIELD))
    })
}

fn scalar_value(instance: &Instance) -> Option<String> {
    match instance.as_scalar()? {
        Value::Null | Value::XmlNil(_) => None,
        Value::String(value) => Some(value.clone()),
        Value::Int(value) => Some(value.to_string()),
        Value::Float(value) if value.is_finite() => Some(value.to_string()),
        Value::Float(_) => None,
        Value::Bool(value) => Some(value.to_string()),
    }
}

fn unit_id(measure: &str) -> Option<String> {
    let lexical = expanded_qname(measure).map_or(measure, |(_, lexical)| lexical);
    let id = lexical
        .rsplit_once(':')
        .map_or(lexical, |(_, local)| local)
        .trim();
    (!id.is_empty()).then(|| id.to_string())
}

fn normalize_units(units: &mut [Unit]) -> Result<(), XbrlFormatError> {
    let mut ids = BTreeSet::new();
    for unit in units {
        let direct = unit.measure.is_some();
        let divided = unit.numerator.is_some() && unit.denominator.is_some();
        if direct == divided {
            return Err(XbrlFormatError::InvalidUnit {
                id: unit.id.clone(),
            });
        }
        if unit.id.is_empty() {
            unit.id = if let Some(measure) = &unit.measure {
                unit_id(measure)
            } else {
                unit.numerator
                    .as_deref()
                    .and_then(unit_id)
                    .zip(unit.denominator.as_deref().and_then(unit_id))
                    .map(|(numerator, denominator)| format!("{numerator}_per_{denominator}"))
            }
            .ok_or_else(|| XbrlFormatError::InvalidUnit { id: String::new() })?;
        }
        if !ids.insert(unit.id.clone()) {
            return Err(XbrlFormatError::DuplicateUnit {
                id: unit.id.clone(),
            });
        }
    }
    Ok(())
}

fn unit_fact_type(unit: &Unit) -> Option<XbrlFactType> {
    if let Some(measure) = &unit.measure {
        if expanded_measure_is(measure, "http://www.xbrl.org/2003/iso4217", None) {
            return Some(XbrlFactType::Monetary);
        }
        if expanded_measure_is(measure, XBRLI, Some("pure")) {
            return Some(XbrlFactType::Numeric);
        }
        if expanded_measure_is(measure, XBRLI, Some("shares")) {
            return Some(XbrlFactType::Shares);
        }
        return None;
    }
    match (&unit.numerator, &unit.denominator) {
        (Some(numerator), Some(denominator))
            if expanded_measure_is(numerator, "http://www.xbrl.org/2003/iso4217", None)
                && expanded_measure_is(denominator, XBRLI, Some("shares")) =>
        {
            Some(XbrlFactType::PerShare)
        }
        _ => None,
    }
}

fn expanded_measure_is(value: &str, namespace: &str, local: Option<&str>) -> bool {
    expanded_qname(value).is_some_and(|(actual_namespace, lexical)| {
        actual_namespace == namespace
            && local.is_none_or(|expected| local_name(lexical) == expected)
    })
}

fn namespace_at<'a>(
    namespaces: &'a BTreeMap<Vec<String>, String>,
    path: &[String],
) -> Option<&'a str> {
    namespaces.get(path).map(String::as_str)
}

fn check_depth(depth: usize) -> Result<(), XbrlFormatError> {
    if depth > MAX_SCHEMA_DEPTH {
        Err(XbrlFormatError::SchemaDepth {
            limit: MAX_SCHEMA_DEPTH,
        })
    } else {
        Ok(())
    }
}

fn shape(path: &[String]) -> XbrlFormatError {
    XbrlFormatError::TargetShape {
        path: path.join("/"),
    }
}

/// Reads an XBRL instance and projects its connected facts into the single
/// repeating table declared by `schema`.
pub fn read(path: &Path, schema: &SchemaNode) -> Result<Instance, XbrlFormatError> {
    read_with_namespaces(path, schema, None)
}

/// Reads an XBRL instance using the namespace identities retained by an
/// imported boundary.
pub fn read_with_options(
    path: &Path,
    schema: &SchemaNode,
    options: &mapping::XbrlBoundaryOptions,
) -> Result<Instance, XbrlFormatError> {
    let namespaces = namespace_map(options);
    read_with_namespaces(path, schema, Some(&namespaces))
}

fn read_with_namespaces(
    path: &Path,
    schema: &SchemaNode,
    namespaces: Option<&BTreeMap<Vec<String>, String>>,
) -> Result<Instance, XbrlFormatError> {
    let bytes = std::fs::read(path)?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(XbrlFormatError::InputLimit {
            limit: MAX_INPUT_BYTES / (1024 * 1024),
        });
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        XbrlFormatError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "XBRL input is not UTF-8",
        ))
    })?;
    from_str_with_namespaces(text, schema, namespaces)
}

/// In-memory form of [`read`].
pub fn from_str(text: &str, schema: &SchemaNode) -> Result<Instance, XbrlFormatError> {
    from_str_with_namespaces(text, schema, None)
}

/// In-memory form of [`read_with_options`].
pub fn from_str_with_options(
    text: &str,
    schema: &SchemaNode,
    options: &mapping::XbrlBoundaryOptions,
) -> Result<Instance, XbrlFormatError> {
    let namespaces = namespace_map(options);
    from_str_with_namespaces(text, schema, Some(&namespaces))
}

fn from_str_with_namespaces(
    text: &str,
    schema: &SchemaNode,
    namespaces: Option<&BTreeMap<Vec<String>, String>>,
) -> Result<Instance, XbrlFormatError> {
    if text.len() > MAX_INPUT_BYTES {
        return Err(XbrlFormatError::InputLimit {
            limit: MAX_INPUT_BYTES / (1024 * 1024),
        });
    }
    let document = roxmltree::Document::parse(text)?;
    let root = document.root_element();
    if root.tag_name().name() != "xbrl" || root.tag_name().namespace() != Some(XBRLI) {
        let found = root.tag_name().namespace().map_or_else(
            || root.tag_name().name().to_string(),
            |namespace| format!("{{{namespace}}}{}", root.tag_name().name()),
        );
        return Err(XbrlFormatError::UnexpectedRoot { found });
    }

    let row_path = table_row_path(schema)?;
    let row_schema =
        schema_at_path(schema, &row_path).ok_or(XbrlFormatError::InvalidTableSchema)?;
    let mut concept_path = row_path.clone();
    let concepts = concrete_concepts(row_schema, &mut concept_path, namespaces, 0)?;
    let contexts = read_contexts(&root)?;
    let facts = read_facts(&root, &contexts, &concepts)?;
    materialize(
        schema, &row_path, &row_path, &contexts, &facts, namespaces, 0,
    )
}

#[derive(Clone, Copy)]
struct Context<'a, 'input> {
    id: &'a str,
    node: roxmltree::Node<'a, 'input>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ConceptKey {
    namespace: Option<String>,
    local_name: String,
}

type FactMap<'a, 'input> = BTreeMap<(usize, ConceptKey), roxmltree::Node<'a, 'input>>;

struct RowBuildContext<'a, 'input, 'options> {
    context: Context<'a, 'input>,
    context_index: usize,
    facts: &'a FactMap<'a, 'input>,
    namespaces: Option<&'options BTreeMap<Vec<String>, String>>,
}

fn table_row_path(schema: &SchemaNode) -> Result<Vec<String>, XbrlFormatError> {
    fn collect(
        node: &SchemaNode,
        path: &mut Vec<String>,
        found: &mut Vec<Vec<String>>,
        depth: usize,
    ) -> Result<(), XbrlFormatError> {
        if depth > MAX_SCHEMA_DEPTH {
            return Err(XbrlFormatError::SchemaDepth {
                limit: MAX_SCHEMA_DEPTH,
            });
        }
        if node.repeating {
            if !matches!(node.kind, SchemaKind::Group { .. }) {
                return Err(XbrlFormatError::InvalidTableSchema);
            }
            found.push(path.clone());
        }
        let SchemaKind::Group { children, .. } = &node.kind else {
            return Ok(());
        };
        for child in children {
            path.push(child.name.clone());
            collect(child, path, found, depth + 1)?;
            path.pop();
        }
        Ok(())
    }

    let mut found = Vec::new();
    collect(schema, &mut Vec::new(), &mut found, 0)?;
    match found.as_slice() {
        [path] if !path.is_empty() => Ok(path.clone()),
        _ => Err(XbrlFormatError::InvalidTableSchema),
    }
}

fn schema_at_path<'a>(mut schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    for segment in path {
        schema = schema.child(segment)?;
    }
    Some(schema)
}

fn concrete_concepts(
    schema: &SchemaNode,
    path: &mut Vec<String>,
    namespaces: Option<&BTreeMap<Vec<String>, String>>,
    depth: usize,
) -> Result<BTreeSet<ConceptKey>, XbrlFormatError> {
    if depth > MAX_SCHEMA_DEPTH {
        return Err(XbrlFormatError::SchemaDepth {
            limit: MAX_SCHEMA_DEPTH,
        });
    }
    let mut result = BTreeSet::new();
    match &schema.kind {
        SchemaKind::Scalar { .. } => {
            if !schema.attribute && !schema.text && !is_context_schema(schema, path, namespaces) {
                result.insert(concept_key(schema, path, namespaces)?);
            }
        }
        SchemaKind::Group { children, .. } => {
            if children.iter().any(|child| child.text)
                && !is_context_schema(schema, path, namespaces)
            {
                result.insert(concept_key(schema, path, namespaces)?);
            }
            for child in children {
                path.push(child.name.clone());
                result.extend(concrete_concepts(child, path, namespaces, depth + 1)?);
                path.pop();
            }
        }
    }
    Ok(result)
}

fn concept_key(
    schema: &SchemaNode,
    path: &[String],
    namespaces: Option<&BTreeMap<Vec<String>, String>>,
) -> Result<ConceptKey, XbrlFormatError> {
    let namespace = namespaces
        .map(|namespaces| {
            namespaces
                .get(path)
                .cloned()
                .ok_or_else(|| XbrlFormatError::MissingFactNamespace {
                    path: path.join("/"),
                })
        })
        .transpose()?;
    Ok(ConceptKey {
        namespace,
        local_name: local_name(&schema.name).to_string(),
    })
}

fn is_context_schema(
    schema: &SchemaNode,
    path: &[String],
    namespaces: Option<&BTreeMap<Vec<String>, String>>,
) -> bool {
    if !is_context_field(local_name(&schema.name)) {
        return false;
    }
    namespaces.is_none_or(|namespaces| namespace_at(namespaces, path) == Some(XBRLI))
}

fn local_name(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

fn read_contexts<'a, 'input>(
    root: &roxmltree::Node<'a, 'input>,
) -> Result<Vec<Context<'a, 'input>>, XbrlFormatError> {
    let mut contexts = Vec::new();
    let mut ids = BTreeSet::new();
    for node in root.children().filter(|node| {
        node.is_element()
            && node.tag_name().name() == "context"
            && node.tag_name().namespace() == Some("http://www.xbrl.org/2003/instance")
    }) {
        if contexts.len() == MAX_CONTEXTS {
            return Err(XbrlFormatError::ContextLimit {
                limit: MAX_CONTEXTS,
            });
        }
        let id = node.attribute("id").unwrap_or_default();
        if id.is_empty() || !ids.insert(id) {
            return Err(XbrlFormatError::InvalidContextId { id: id.to_string() });
        }
        contexts.push(Context { id, node });
    }
    Ok(contexts)
}

fn read_facts<'a, 'input>(
    root: &roxmltree::Node<'a, 'input>,
    contexts: &[Context<'a, 'input>],
    concepts: &BTreeSet<ConceptKey>,
) -> Result<FactMap<'a, 'input>, XbrlFormatError> {
    let context_indexes = contexts
        .iter()
        .enumerate()
        .map(|(index, context)| (context.id, index))
        .collect::<BTreeMap<_, _>>();
    let mut facts = BTreeMap::new();
    let mut count = 0usize;
    for node in root.children().filter(|node| node.is_element()) {
        let exact = ConceptKey {
            namespace: node.tag_name().namespace().map(str::to_string),
            local_name: node.tag_name().name().to_string(),
        };
        let wildcard = ConceptKey {
            namespace: None,
            local_name: node.tag_name().name().to_string(),
        };
        let concept = if concepts.contains(&exact) {
            exact
        } else if concepts.contains(&wildcard) {
            wildcard
        } else {
            continue;
        };
        count += 1;
        if count > MAX_FACTS {
            return Err(XbrlFormatError::FactLimit { limit: MAX_FACTS });
        }
        let Some(context_id) = node.attribute("contextRef") else {
            continue;
        };
        let Some(index) = context_indexes.get(context_id).copied() else {
            continue;
        };
        if facts.insert((index, concept.clone()), node).is_some() {
            return Err(XbrlFormatError::DuplicateFact {
                context: context_id.to_string(),
                concept: concept.local_name,
            });
        }
    }
    Ok(facts)
}

fn materialize(
    schema: &SchemaNode,
    row_path: &[String],
    full_row_path: &[String],
    contexts: &[Context<'_, '_>],
    facts: &FactMap<'_, '_>,
    namespaces: Option<&BTreeMap<Vec<String>, String>>,
    depth: usize,
) -> Result<Instance, XbrlFormatError> {
    if depth > MAX_SCHEMA_DEPTH {
        return Err(XbrlFormatError::SchemaDepth {
            limit: MAX_SCHEMA_DEPTH,
        });
    }
    if row_path.is_empty() {
        let rows = contexts
            .iter()
            .enumerate()
            .filter(|(index, _)| facts.keys().any(|(fact_index, _)| fact_index == index))
            .map(|(index, context)| {
                let source = RowBuildContext {
                    context: *context,
                    context_index: index,
                    facts,
                    namespaces,
                };
                build_row(
                    schema,
                    &source,
                    None,
                    &mut full_row_path.to_vec(),
                    depth + 1,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Instance::Repeated(rows));
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(XbrlFormatError::InvalidTableSchema);
    };
    let mut fields = Vec::with_capacity(children.len());
    for child in children {
        let value = if child.name == row_path[0] {
            materialize(
                child,
                &row_path[1..],
                full_row_path,
                contexts,
                facts,
                namespaces,
                depth + 1,
            )?
        } else {
            empty_instance(child, depth + 1)?
        };
        fields.push((child.name.clone(), value));
    }
    Ok(Instance::Group(fields))
}

fn build_row(
    schema: &SchemaNode,
    source: &RowBuildContext<'_, '_, '_>,
    active: Option<roxmltree::Node<'_, '_>>,
    path: &mut Vec<String>,
    depth: usize,
) -> Result<Instance, XbrlFormatError> {
    if depth > MAX_SCHEMA_DEPTH {
        return Err(XbrlFormatError::SchemaDepth {
            limit: MAX_SCHEMA_DEPTH,
        });
    }
    match &schema.kind {
        SchemaKind::Scalar { ty } => {
            let text = scalar_text(
                schema,
                source.context,
                source.context_index,
                source.facts,
                active,
                path,
                source.namespaces,
            );
            Ok(Instance::Scalar(parse_scalar(&schema.name, *ty, text)?))
        }
        SchemaKind::Group { children, .. } => {
            let active = fact_at(
                source.facts,
                source.context_index,
                schema,
                path,
                source.namespaces,
            )
            .or_else(|| {
                is_context_schema(schema, path, source.namespaces)
                    .then(|| context_element(source.context.node, local_name(&schema.name)))
                    .flatten()
            })
            .or(active);
            let fields = children
                .iter()
                .map(|child| {
                    path.push(child.name.clone());
                    let value = build_row(child, source, active, path, depth + 1);
                    path.pop();
                    value.map(|value| (child.name.clone(), value))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Instance::Group(fields))
        }
    }
}

fn scalar_text<'a>(
    schema: &SchemaNode,
    context: Context<'a, '_>,
    context_index: usize,
    facts: &'a FactMap<'a, '_>,
    active: Option<roxmltree::Node<'a, '_>>,
    path: &[String],
    namespaces: Option<&BTreeMap<Vec<String>, String>>,
) -> Option<&'a str> {
    if schema.attribute {
        return active.and_then(|node| node.attribute(schema.name.as_str()));
    }
    if schema.text || schema.name == XML_TEXT_FIELD {
        return active.and_then(|node| node.text());
    }
    if is_context_schema(schema, path, namespaces) {
        return context_element(context.node, local_name(&schema.name))
            .and_then(|node| node.text());
    }
    fact_at(facts, context_index, schema, path, namespaces).and_then(|node| node.text())
}

fn fact_at<'a, 'input>(
    facts: &'a FactMap<'a, 'input>,
    context_index: usize,
    schema: &SchemaNode,
    path: &[String],
    namespaces: Option<&BTreeMap<Vec<String>, String>>,
) -> Option<roxmltree::Node<'a, 'input>> {
    let namespace = match namespaces {
        Some(namespaces) => Some(namespace_at(namespaces, path)?.to_string()),
        None => None,
    };
    facts
        .get(&(
            context_index,
            ConceptKey {
                namespace,
                local_name: local_name(&schema.name).to_string(),
            },
        ))
        .copied()
}

fn context_element<'a, 'input>(
    context: roxmltree::Node<'a, 'input>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    context.descendants().find(|node| {
        node.is_element()
            && node.tag_name().name() == name
            && node.tag_name().namespace() == Some(XBRLI)
    })
}

fn is_context_field(name: &str) -> bool {
    matches!(
        name,
        "period" | "startDate" | "endDate" | "instant" | "identifier" | "forever"
    )
}

fn parse_scalar(name: &str, ty: ScalarType, text: Option<&str>) -> Result<Value, XbrlFormatError> {
    let Some(text) = text else {
        return Ok(Value::Null);
    };
    let invalid = || XbrlFormatError::ScalarParse {
        name: name.to_string(),
        ty,
        value: text.to_string(),
    };
    match ty {
        ScalarType::String => Ok(Value::String(text.to_string())),
        ScalarType::Int => text.trim().parse().map(Value::Int).map_err(|_| invalid()),
        ScalarType::Float => {
            let value = text.trim().parse::<f64>().map_err(|_| invalid())?;
            if value.is_finite() {
                Ok(Value::Float(value))
            } else {
                Err(invalid())
            }
        }
        ScalarType::Bool => text.trim().parse().map(Value::Bool).map_err(|_| invalid()),
    }
}

fn empty_instance(schema: &SchemaNode, depth: usize) -> Result<Instance, XbrlFormatError> {
    if depth > MAX_SCHEMA_DEPTH {
        return Err(XbrlFormatError::SchemaDepth {
            limit: MAX_SCHEMA_DEPTH,
        });
    }
    if schema.repeating {
        return Ok(Instance::Repeated(Vec::new()));
    }
    match &schema.kind {
        SchemaKind::Scalar { .. } => Ok(Instance::Scalar(Value::Null)),
        SchemaKind::Group { children, .. } => Ok(Instance::Group(
            children
                .iter()
                .map(|child| {
                    empty_instance(child, depth + 1).map(|value| (child.name.clone(), value))
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
    }
}

#[cfg(test)]
mod tests;
