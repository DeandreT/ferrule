//! Canonical export for MapForce's runtime-named JSON property ports.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{FormatOptions, Graph, Node, NodeId, Scope, ScopeConstruction, ScopeIteration};

use crate::MfdError;

mod component;

use super::schema::{KeyAlloc, PortTree, RenderedSchemaComponent, SideFormat, xml_escape};
use super::source::SourceExports;
use component::{JsonComponentArgs, JsonSide, render_json_component};

#[derive(Default)]
pub(super) struct SourcePlan {
    sites: Vec<SourceSite>,
    patterns: Vec<DynamicBooleanPattern>,
    owned_nodes: BTreeSet<NodeId>,
}

struct SourceSite {
    source_index: usize,
    owner: Vec<String>,
    name_port: u32,
    value_port: u32,
}

struct DynamicBooleanPattern {
    output: NodeId,
    key: NodeId,
    site: usize,
}

pub(super) struct TargetPlan {
    kind: TargetKind,
    static_root: Option<Scope>,
}

enum TargetKind {
    Root(RootTarget),
    Nested(Vec<NestedTarget>),
}

struct RootTarget {
    outer_source: Vec<String>,
    group_by: Option<NodeId>,
    property_input: u32,
    property_key: NodeId,
    property_key_input: u32,
    item_source: Vec<String>,
    item_input: u32,
    fields: Vec<DynamicTargetField>,
    value_type: ScalarType,
}

struct NestedTarget {
    owner: Vec<String>,
    driver: DynamicDriver,
    property_input: u32,
    field: DynamicTargetField,
    value_type: ScalarType,
}

enum DynamicDriver {
    Source(Vec<String>),
    Sequence(NodeId),
}

struct DynamicTargetField {
    key: NodeId,
    key_input: u32,
    value: NodeId,
    value_input: u32,
}

pub(super) struct ConnectArgs<'a> {
    pub(super) sources: &'a SourceExports<'a>,
    pub(super) node_out_key: &'a BTreeMap<NodeId, u32>,
    pub(super) keys: &'a mut KeyAlloc,
    pub(super) uid: &'a mut u32,
    pub(super) components: &'a mut String,
    pub(super) edges: &'a mut Vec<(u32, u32)>,
}

pub(super) struct RenderTargetArgs<'a> {
    pub(super) plan: &'a TargetPlan,
    pub(super) schema: &'a SchemaNode,
    pub(super) ports: &'a PortTree,
    pub(super) instance_path: Option<&'a str>,
    pub(super) json_lines: bool,
    pub(super) mfd_path: &'a Path,
    pub(super) component_name: &'a str,
    pub(super) component_uid: u32,
    pub(super) sibling_suffix: &'a str,
    pub(super) default_output: bool,
}

pub(super) struct RenderSourceArgs<'a> {
    pub(super) plan: &'a SourcePlan,
    pub(super) source_index: usize,
    pub(super) schema: &'a SchemaNode,
    pub(super) ports: &'a PortTree,
    pub(super) instance_path: Option<&'a str>,
    pub(super) json_lines: bool,
    pub(super) mfd_path: &'a Path,
    pub(super) component_name: &'a str,
    pub(super) component_uid: u32,
    pub(super) sibling_suffix: &'a str,
}

pub(super) fn target_format_is_implicit(options: &FormatOptions) -> bool {
    !options.lenient_segments
        && options.edi_kind.is_none()
        && options.idoc.is_none()
        && options.swift_mt.is_none()
        && options.delimiter.is_none()
        && options.has_header_row.is_none()
        && options.fixed_width.is_none()
        && options.flextext.is_none()
        && options.pdf.is_none()
        && options.http_get.is_none()
        && options.external_source.is_none()
        && !options.xml_document
        && !options.local_xml_file_set
        && options.tabular_kind.is_none()
        && options.protobuf.is_none()
        && options.xbrl.is_none()
        && options.xlsx_sheet.is_none()
        && options.xlsx_start_row.is_none()
        && options.xlsx_columns.is_empty()
        && !options.xlsx_update_existing
        && options.xlsx_rows.is_empty()
        && options.xlsx_composite.is_none()
        && options.xlsx_grid.is_none()
        && options.xlsx_hierarchical.is_none()
}

impl SourcePlan {
    pub(super) fn build(
        project: &mapping::Project,
        sources: &SourceExports<'_>,
        keys: &mut KeyAlloc,
    ) -> Result<Self, MfdError> {
        let consumers = node_consumers(&project.graph);
        let mut plan = Self::default();
        for (&field_id, node) in &project.graph.nodes {
            let Node::DynamicSourceField { object, frame, key } = node else {
                continue;
            };
            let (output, exists) = exact_boolean_envelope(
                &project.graph,
                &consumers,
                field_id,
            )
            .ok_or_else(|| {
                MfdError::Unsupported(format!(
                    "dynamic source field node {field_id} is exportable only through an exact boolean existence envelope"
                ))
            })?;
            let mut absolute = frame.clone().unwrap_or_default();
            absolute.extend(object.iter().cloned());
            let (source_index, source, owner) = sources.owner_with_index(&absolute);
            if source.format != SideFormat::Json
                || source.options.external_source.is_some()
                || source.dynamic_path_node.is_some()
            {
                return Err(MfdError::Unsupported(format!(
                    "dynamic source field node {field_id} requires an ordinary JSON source component"
                )));
            }
            let owner_schema = schema_node_at(source.schema, owner).ok_or_else(|| {
                MfdError::Unsupported(format!(
                    "dynamic source field node {field_id} references missing open object `{}`",
                    absolute.join("/")
                ))
            })?;
            if !matches!(
                owner_schema.dynamic_fields(),
                Some(SchemaNode {
                    repeating: false,
                    kind: SchemaKind::Scalar {
                        ty: ScalarType::Bool
                    },
                    ..
                })
            ) {
                return Err(MfdError::Unsupported(format!(
                    "dynamic source field node {field_id} requires boolean additional properties"
                )));
            }
            let site = plan
                .sites
                .iter()
                .position(|site| site.source_index == source_index && site.owner == owner)
                .unwrap_or_else(|| {
                    let index = plan.sites.len();
                    plan.sites.push(SourceSite {
                        source_index,
                        owner: owner.to_vec(),
                        name_port: keys.next(),
                        value_port: keys.next(),
                    });
                    index
                });
            plan.patterns.push(DynamicBooleanPattern {
                output,
                key: *key,
                site,
            });
            plan.owned_nodes.extend([field_id, exists, output]);
        }
        Ok(plan)
    }

    pub(super) fn owned_nodes(&self) -> &BTreeSet<NodeId> {
        &self.owned_nodes
    }

    pub(super) fn render_nodes(
        &self,
        keys: &mut KeyAlloc,
        uid: &mut u32,
        node_out_key: &mut BTreeMap<NodeId, u32>,
        components: &mut String,
        edges: &mut Vec<(u32, u32)>,
    ) -> Result<(), MfdError> {
        for pattern in &self.patterns {
            let site = self.sites.get(pattern.site).ok_or_else(|| {
                MfdError::Unsupported("internal dynamic JSON source site is missing".into())
            })?;
            let key_output = node_out_key.get(&pattern.key).copied().ok_or_else(|| {
                MfdError::Unsupported(format!(
                    "dynamic source field key references unexported node {}",
                    pattern.key
                ))
            })?;
            let equal_name = keys.next();
            let equal_key = keys.next();
            let equal_output = keys.next();
            *uid += 1;
            let _ = write!(
                components,
                "\t\t\t\t<component name=\"equal\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                 \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{equal_name}\"/><datapoint pos=\"1\" key=\"{equal_key}\"/></sources>\n\
                 \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{equal_output}\"/></targets>\n\
                 \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                 \t\t\t\t</component>\n"
            );

            let and_predicate = keys.next();
            let and_value = keys.next();
            let and_output = keys.next();
            *uid += 1;
            let _ = write!(
                components,
                "\t\t\t\t<component name=\"logical-and\" library=\"core\" uid=\"{uid}\" kind=\"5\">\n\
                 \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{and_predicate}\"/><datapoint pos=\"1\" key=\"{and_value}\"/></sources>\n\
                 \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{and_output}\"/></targets>\n\
                 \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                 \t\t\t\t</component>\n"
            );
            node_out_key.insert(pattern.output, and_output);
            edges.extend([
                (site.name_port, equal_name),
                (key_output, equal_key),
                (equal_output, and_predicate),
                (site.value_port, and_value),
            ]);
        }
        Ok(())
    }

    fn sites_for(&self, source_index: usize) -> Vec<&SourceSite> {
        self.sites
            .iter()
            .filter(|site| site.source_index == source_index)
            .collect()
    }
}

impl TargetPlan {
    pub(super) fn build(
        schema: &SchemaNode,
        root: &Scope,
        sources: &SourceExports<'_>,
        graph: &Graph,
        keys: &mut KeyAlloc,
    ) -> Result<Option<Self>, MfdError> {
        if !scope_has_dynamic_mapping(root) {
            return Ok(None);
        }
        if let Some(root_target) = build_root_target(schema, root, sources, graph, keys)? {
            return Ok(Some(Self {
                kind: TargetKind::Root(root_target),
                static_root: None,
            }));
        }

        let mut nested = Vec::new();
        collect_nested_targets(
            schema,
            root,
            sources,
            graph,
            keys,
            &mut Vec::new(),
            &[],
            &mut nested,
        )?;
        if nested.is_empty() {
            return Err(MfdError::Unsupported(
                "computed JSON property mapping does not match a supported canonical shape".into(),
            ));
        }
        let mut static_root = root.clone();
        remove_dynamic_scopes(&mut static_root, &nested, &mut Vec::new());
        if scope_has_dynamic_mapping(&static_root) {
            return Err(MfdError::Unsupported(
                "computed JSON property mapping contains unsupported mixed dynamic content".into(),
            ));
        }
        Ok(Some(Self {
            kind: TargetKind::Nested(nested),
            static_root: Some(static_root),
        }))
    }

    pub(super) fn static_root(&self) -> Option<&Scope> {
        self.static_root.as_ref()
    }

    pub(super) fn connect(&self, mut args: ConnectArgs<'_>) -> Result<(), MfdError> {
        match &self.kind {
            TargetKind::Root(root) => root.connect(args),
            TargetKind::Nested(targets) => {
                for target in targets {
                    target.connect(&mut args)?;
                }
                Ok(())
            }
        }
    }
}

impl RootTarget {
    fn connect(&self, args: ConnectArgs<'_>) -> Result<(), MfdError> {
        let mut driver = args
            .sources
            .key_for_abs(&self.outer_source)
            .ok_or_else(|| {
                MfdError::Unsupported(format!(
                    "dynamic root property source `{}` has no exported port",
                    self.outer_source.join("/")
                ))
            })?;
        if let Some(group_by) = self.group_by {
            let group_key = required_node_output(args.node_out_key, group_by, "dynamic group key")?;
            let nodes_input = args.keys.next();
            let key_input = args.keys.next();
            let groups_output = args.keys.next();
            *args.uid += 1;
            let _ = write!(
                args.components,
                "\t\t\t\t<component name=\"group-by\" library=\"core\" uid=\"{}\" kind=\"5\">\n\
                 \t\t\t\t\t<sources><datapoint pos=\"0\" key=\"{nodes_input}\"/><datapoint pos=\"1\" key=\"{key_input}\"/></sources>\n\
                 \t\t\t\t\t<targets><datapoint pos=\"0\" key=\"{groups_output}\"/><datapoint/></targets>\n\
                 \t\t\t\t\t<view ltx=\"20\" lty=\"20\" rbx=\"120\" rby=\"60\"/>\n\
                 \t\t\t\t</component>\n",
                *args.uid
            );
            args.edges
                .extend([(driver, nodes_input), (group_key, key_input)]);
            driver = groups_output;
        }
        args.edges.push((driver, self.property_input));
        args.edges.push((
            required_node_output(args.node_out_key, self.property_key, "dynamic property key")?,
            self.property_key_input,
        ));
        args.edges.push((
            args.sources.key_for_abs(&self.item_source).ok_or_else(|| {
                MfdError::Unsupported(format!(
                    "dynamic root item source `{}` has no exported port",
                    self.item_source.join("/")
                ))
            })?,
            self.item_input,
        ));
        for field in &self.fields {
            connect_field(field, args.node_out_key, args.edges)?;
        }
        Ok(())
    }
}

impl NestedTarget {
    fn connect(&self, args: &mut ConnectArgs<'_>) -> Result<(), MfdError> {
        let driver = match &self.driver {
            DynamicDriver::Source(path) => args.sources.key_for_abs(path).ok_or_else(|| {
                MfdError::Unsupported(format!(
                    "dynamic property source `{}` has no exported port",
                    path.join("/")
                ))
            })?,
            DynamicDriver::Sequence(item) => {
                required_node_output(args.node_out_key, *item, "dynamic sequence item")?
            }
        };
        args.edges.push((driver, self.property_input));
        connect_field(&self.field, args.node_out_key, args.edges)
    }
}

pub(super) fn render_target(args: RenderTargetArgs<'_>) -> RenderedSchemaComponent {
    let entries = match &args.plan.kind {
        TargetKind::Root(root) => render_root_target_entries(root, 10),
        TargetKind::Nested(targets) => {
            render_json_entries(args.schema, args.ports, "inpkey", 10, &[], targets)
        }
    };
    render_json_component(JsonComponentArgs {
        schema: args.schema,
        entries,
        side: JsonSide::Target {
            default_output: args.default_output,
        },
        instance_path: args.instance_path,
        json_lines: args.json_lines,
        mfd_path: args.mfd_path,
        component_name: args.component_name,
        component_uid: args.component_uid,
        sibling_suffix: args.sibling_suffix,
    })
}

pub(super) fn render_source(args: RenderSourceArgs<'_>) -> Option<RenderedSchemaComponent> {
    let sites = args.plan.sites_for(args.source_index);
    if sites.is_empty() {
        return None;
    }
    let entries = render_source_entries(args.schema, args.ports, 10, &[], &sites);
    Some(render_json_component(JsonComponentArgs {
        schema: args.schema,
        entries,
        side: JsonSide::Source,
        instance_path: args.instance_path,
        json_lines: args.json_lines,
        mfd_path: args.mfd_path,
        component_name: args.component_name,
        component_uid: args.component_uid,
        sibling_suffix: args.sibling_suffix,
    }))
}

fn build_root_target(
    schema: &SchemaNode,
    root: &Scope,
    sources: &SourceExports<'_>,
    graph: &Graph,
    keys: &mut KeyAlloc,
) -> Result<Option<RootTarget>, MfdError> {
    if !root.merge_dynamic_fields || root.dynamic_children.is_empty() {
        return Ok(None);
    }
    let [child] = root.dynamic_children.as_slice() else {
        return Err(dynamic_target_error(
            "root mappings require exactly one computed child array",
        ));
    };
    if root.construction != ScopeConstruction::Constructed
        || root.source().is_none()
        || root.sequence().is_some()
        || root.join().is_some()
        || root.filter.is_some()
        || root.group_starting_with.is_some()
        || root.group_into_blocks.is_some()
        || root.sort_by.is_some()
        || root.take.is_some()
        || !root.bindings.is_empty()
        || !root.children.is_empty()
        || !root.dynamic_bindings.is_empty()
    {
        return Err(dynamic_target_error(
            "root mappings support only plain or grouped source iteration",
        ));
    }
    let item_scope = &child.scope;
    if item_scope.construction != ScopeConstruction::Constructed
        || item_scope.source().is_none()
        || item_scope.sequence().is_some()
        || item_scope.join().is_some()
        || item_scope.filter.is_some()
        || item_scope.group_by.is_some()
        || item_scope.group_starting_with.is_some()
        || item_scope.group_into_blocks.is_some()
        || item_scope.sort_by.is_some()
        || item_scope.take.is_some()
        || !item_scope.bindings.is_empty()
        || !item_scope.children.is_empty()
        || !item_scope.dynamic_children.is_empty()
        || item_scope.dynamic_bindings.is_empty()
        || item_scope.merge_dynamic_fields
    {
        return Err(dynamic_target_error(
            "root computed child arrays require one plain source iteration with scalar computed fields",
        ));
    }
    let Some(item_schema) = schema.dynamic_fields() else {
        return Err(dynamic_target_error(
            "root schema has no typed dynamic child values",
        ));
    };
    if !item_schema.repeating {
        return Err(dynamic_target_error(
            "root computed properties require repeating object values",
        ));
    }
    let Some(value_schema) = item_schema.dynamic_fields() else {
        return Err(dynamic_target_error(
            "root computed array items have no typed dynamic scalar values",
        ));
    };
    let SchemaKind::Scalar { ty: value_type } = value_schema.kind else {
        return Err(dynamic_target_error(
            "root computed array item values must be scalar",
        ));
    };
    if value_schema.repeating {
        return Err(dynamic_target_error(
            "root computed array item values cannot repeat",
        ));
    }
    let outer_source = root.source().unwrap_or_default().to_vec();
    if sources.schema_node_at(&outer_source).is_none() {
        return Err(dynamic_target_error("root source collection is missing"));
    }
    let mut item_source = if root.group_by.is_some() {
        outer_source[..outer_source.len().saturating_sub(1)].to_vec()
    } else {
        outer_source.clone()
    };
    item_source.extend(item_scope.source().unwrap_or_default().iter().cloned());
    if sources.schema_node_at(&item_source).is_none() {
        return Err(dynamic_target_error(
            "computed array item collection is missing",
        ));
    }
    reject_position_dependencies(
        graph,
        root.group_by.into_iter().chain([child.key]).chain(
            item_scope
                .dynamic_bindings
                .iter()
                .flat_map(|binding| [binding.key, binding.value]),
        ),
    )?;
    let fields = item_scope
        .dynamic_bindings
        .iter()
        .map(|binding| DynamicTargetField {
            key: binding.key,
            key_input: keys.next(),
            value: binding.value,
            value_input: keys.next(),
        })
        .collect();
    Ok(Some(RootTarget {
        outer_source,
        group_by: root.group_by,
        property_input: keys.next(),
        property_key: child.key,
        property_key_input: keys.next(),
        item_source,
        item_input: keys.next(),
        fields,
        value_type,
    }))
}

#[allow(clippy::too_many_arguments)]
fn collect_nested_targets(
    schema: &SchemaNode,
    scope: &Scope,
    sources: &SourceExports<'_>,
    graph: &Graph,
    keys: &mut KeyAlloc,
    path: &mut Vec<String>,
    anchor: &[String],
    targets: &mut Vec<NestedTarget>,
) -> Result<(), MfdError> {
    let current_anchor = if let Some(source) = scope.source() {
        if sources.is_named_extra_path(source) {
            source.to_vec()
        } else {
            let mut absolute = anchor.to_vec();
            absolute.extend(source.iter().cloned());
            absolute
        }
    } else {
        anchor.to_vec()
    };
    if scope.merge_dynamic_fields || !scope.dynamic_bindings.is_empty() {
        if path.is_empty()
            || scope.construction != ScopeConstruction::Constructed
            || !scope.merge_dynamic_fields
            || scope.dynamic_bindings.len() != 1
            || !scope.bindings.is_empty()
            || !scope.children.is_empty()
            || !scope.dynamic_children.is_empty()
            || scope.filter.is_some()
            || scope.group_by.is_some()
            || scope.group_starting_with.is_some()
            || scope.group_into_blocks.is_some()
            || scope.sort_by.is_some()
            || scope.take.is_some()
            || scope.join().is_some()
        {
            return Err(dynamic_target_error(
                "nested mappings require one plain iteration and one computed scalar property",
            ));
        }
        let driver = match &scope.iteration {
            ScopeIteration::Source(_) | ScopeIteration::DynamicDocuments { .. } => {
                if sources.schema_node_at(&current_anchor).is_none() {
                    return Err(dynamic_target_error(
                        "nested computed property source collection is missing",
                    ));
                }
                DynamicDriver::Source(current_anchor)
            }
            ScopeIteration::Sequence(sequence) => DynamicDriver::Sequence(sequence.item()),
            ScopeIteration::None
            | ScopeIteration::InnerJoin { .. }
            | ScopeIteration::Concatenate(_) => {
                return Err(dynamic_target_error(
                    "nested computed properties require source or generated-sequence iteration",
                ));
            }
        };
        let owner_schema = schema_node_at(schema, path).ok_or_else(|| {
            dynamic_target_error("nested computed property owner is missing from the target schema")
        })?;
        let Some(value_schema) = owner_schema.dynamic_fields() else {
            return Err(dynamic_target_error(
                "nested computed property owner has no typed dynamic fields",
            ));
        };
        let SchemaKind::Scalar { ty: value_type } = value_schema.kind else {
            return Err(dynamic_target_error(
                "nested computed property values must be scalar",
            ));
        };
        if value_schema.repeating {
            return Err(dynamic_target_error(
                "nested computed property values cannot repeat",
            ));
        }
        let binding = &scope.dynamic_bindings[0];
        reject_position_dependencies(graph, [binding.key, binding.value])?;
        targets.push(NestedTarget {
            owner: path.clone(),
            driver,
            property_input: keys.next(),
            field: DynamicTargetField {
                key: binding.key,
                key_input: keys.next(),
                value: binding.value,
                value_input: keys.next(),
            },
            value_type,
        });
        return Ok(());
    }
    if !scope.dynamic_children.is_empty() {
        return Err(dynamic_target_error(
            "nested computed object names are supported only by the grouped root shape",
        ));
    }
    for child in &scope.children {
        path.push(child.target_field.clone());
        collect_nested_targets(
            schema,
            child,
            sources,
            graph,
            keys,
            path,
            &current_anchor,
            targets,
        )?;
        path.pop();
    }
    Ok(())
}

fn remove_dynamic_scopes(scope: &mut Scope, sites: &[NestedTarget], path: &mut Vec<String>) {
    scope.children.retain_mut(|child| {
        path.push(child.target_field.clone());
        let remove = sites.iter().any(|site| site.owner == *path);
        if !remove {
            remove_dynamic_scopes(child, sites, path);
        }
        path.pop();
        !remove
    });
}

fn exact_boolean_envelope(
    graph: &Graph,
    consumers: &BTreeMap<NodeId, Vec<NodeId>>,
    field: NodeId,
) -> Option<(NodeId, NodeId)> {
    let field_consumers = consumers.get(&field)?;
    for (&output, node) in &graph.nodes {
        let Node::If {
            condition,
            then,
            else_,
        } = node
        else {
            continue;
        };
        if *then != field
            || !matches!(
                graph.nodes.get(else_),
                Some(Node::Const {
                    value: Value::Bool(false)
                })
            )
            || !matches!(
                graph.nodes.get(condition),
                Some(Node::Call { function, args })
                    if function == "exists" && args.as_slice() == [field]
            )
        {
            continue;
        }
        let condition_consumers = consumers
            .get(condition)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if condition_consumers != [output]
            || field_consumers.len() != 2
            || !field_consumers.contains(condition)
            || !field_consumers.contains(&output)
        {
            continue;
        }
        return Some((output, *condition));
    }
    None
}

fn node_consumers(graph: &Graph) -> BTreeMap<NodeId, Vec<NodeId>> {
    let mut consumers: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
    for (&id, node) in &graph.nodes {
        for dependency in node_dependencies(node) {
            consumers.entry(dependency).or_default().push(id);
        }
    }
    consumers
}

fn node_dependencies(node: &Node) -> Vec<NodeId> {
    match node {
        Node::SourceField { .. }
        | Node::SourceDocumentPath
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Const { .. }
        | Node::RuntimeValue { .. } => Vec::new(),
        Node::Call { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence
            .inputs()
            .into_iter()
            .chain([sequence.item(), *predicate])
            .collect(),
        Node::SequenceItemAt { sequence, index } => sequence
            .inputs()
            .into_iter()
            .chain([sequence.item(), *index])
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
    }
}

fn reject_position_dependencies(
    graph: &Graph,
    roots: impl IntoIterator<Item = NodeId>,
) -> Result<(), MfdError> {
    let mut pending = roots.into_iter().collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = graph.nodes.get(&id) else {
            continue;
        };
        if matches!(node, Node::Position { .. } | Node::JoinPosition { .. }) {
            return Err(dynamic_target_error(
                "computed property expressions using position are not yet exportable",
            ));
        }
        pending.extend(node_dependencies(node));
    }
    Ok(())
}

fn connect_field(
    field: &DynamicTargetField,
    node_out_key: &BTreeMap<NodeId, u32>,
    edges: &mut Vec<(u32, u32)>,
) -> Result<(), MfdError> {
    edges.extend([
        (
            required_node_output(node_out_key, field.key, "dynamic property key")?,
            field.key_input,
        ),
        (
            required_node_output(node_out_key, field.value, "dynamic property value")?,
            field.value_input,
        ),
    ]);
    Ok(())
}

fn required_node_output(
    outputs: &BTreeMap<NodeId, u32>,
    node: NodeId,
    label: &str,
) -> Result<u32, MfdError> {
    outputs
        .get(&node)
        .copied()
        .ok_or_else(|| MfdError::Unsupported(format!("{label} references unexported node {node}")))
}

fn scope_has_dynamic_mapping(scope: &Scope) -> bool {
    scope.merge_dynamic_fields
        || !scope.dynamic_bindings.is_empty()
        || !scope.dynamic_children.is_empty()
        || scope.children.iter().any(scope_has_dynamic_mapping)
        || scope
            .concatenated()
            .is_some_and(|segments| segments.iter().any(scope_has_dynamic_mapping))
}

fn dynamic_target_error(reason: &str) -> MfdError {
    MfdError::Unsupported(format!(
        "computed JSON property mapping is not exportable: {reason}"
    ))
}

fn schema_node_at<'a>(schema: &'a SchemaNode, path: &[String]) -> Option<&'a SchemaNode> {
    let mut current = schema;
    for segment in path {
        current = current.child(segment)?;
    }
    Some(current)
}

fn render_root_target_entries(root: &RootTarget, indent: usize) -> String {
    let pad = "\t".repeat(indent);
    let mut out = String::new();
    let _ = writeln!(out, "{pad}<entry name=\"object\" expanded=\"1\">");
    let _ = writeln!(
        out,
        "{pad}\t<entry name=\"property\" type=\"json-property\" inpkey=\"{}\" expanded=\"1\">",
        root.property_input
    );
    let _ = writeln!(
        out,
        "{pad}\t\t<entry name=\"name\" type=\"json-propertyname\" inpkey=\"{}\"/>",
        root.property_key_input
    );
    let _ = writeln!(out, "{pad}\t\t<entry name=\"array\" expanded=\"1\">");
    let _ = writeln!(
        out,
        "{pad}\t\t\t<entry name=\"item\" type=\"json-item\" inpkey=\"{}\" expanded=\"1\">",
        root.item_input
    );
    let _ = writeln!(out, "{pad}\t\t\t\t<entry name=\"object\" expanded=\"1\">");
    for field in &root.fields {
        let _ = writeln!(
            out,
            "{pad}\t\t\t\t\t<entry name=\"property\" type=\"json-property\" expanded=\"1\">"
        );
        let _ = writeln!(
            out,
            "{pad}\t\t\t\t\t\t<entry name=\"name\" type=\"json-propertyname\" inpkey=\"{}\"/>",
            field.key_input
        );
        let _ = writeln!(
            out,
            "{pad}\t\t\t\t\t\t<entry name=\"{}\" inpkey=\"{}\"/>",
            json_type_name(root.value_type),
            field.value_input
        );
        let _ = writeln!(out, "{pad}\t\t\t\t\t</entry>");
    }
    let _ = writeln!(out, "{pad}\t\t\t\t</entry>");
    let _ = writeln!(out, "{pad}\t\t\t</entry>");
    let _ = writeln!(out, "{pad}\t\t</entry>");
    let _ = writeln!(out, "{pad}\t</entry>");
    let _ = writeln!(out, "{pad}</entry>");
    out
}

fn render_json_entries(
    schema: &SchemaNode,
    ports: &PortTree,
    attr: &str,
    indent: usize,
    path: &[String],
    targets: &[NestedTarget],
) -> String {
    let mut out = String::new();
    if schema.repeating {
        let pad = "\t".repeat(indent);
        let _ = writeln!(out, "{pad}<entry name=\"array\" expanded=\"1\">");
        let _ = writeln!(
            out,
            "{pad}\t<entry name=\"item\" type=\"json-item\" expanded=\"1\">"
        );
        render_json_value(
            schema,
            ports,
            attr,
            indent + 2,
            &mut path.to_vec(),
            targets,
            &mut out,
        );
        let _ = writeln!(out, "{pad}\t</entry>");
        let _ = writeln!(out, "{pad}</entry>");
    } else {
        render_json_value(
            schema,
            ports,
            attr,
            indent,
            &mut path.to_vec(),
            targets,
            &mut out,
        );
    }
    out
}

fn render_json_value(
    node: &SchemaNode,
    ports: &PortTree,
    attr: &str,
    indent: usize,
    path: &mut Vec<String>,
    targets: &[NestedTarget],
    out: &mut String,
) {
    let pad = "\t".repeat(indent);
    let Some(key) = ports.key_for_abs(path) else {
        return;
    };
    match &node.kind {
        SchemaKind::Scalar { ty } => {
            let _ = writeln!(
                out,
                "{pad}<entry name=\"{}\" {attr}=\"{key}\"/>",
                json_type_name(*ty)
            );
        }
        SchemaKind::Group { children, .. } => {
            let _ = writeln!(
                out,
                "{pad}<entry name=\"object\" {attr}=\"{key}\" expanded=\"1\">"
            );
            for child in children {
                let _ = writeln!(
                    out,
                    "{pad}\t<entry name=\"{}\" type=\"json-property\" expanded=\"1\">",
                    xml_escape(&child.name)
                );
                path.push(child.name.clone());
                if child.repeating {
                    let _ = writeln!(out, "{pad}\t\t<entry name=\"array\" expanded=\"1\">");
                    let _ = writeln!(
                        out,
                        "{pad}\t\t\t<entry name=\"item\" type=\"json-item\" expanded=\"1\">"
                    );
                    render_json_value(child, ports, attr, indent + 4, path, targets, out);
                    let _ = writeln!(out, "{pad}\t\t\t</entry>");
                    let _ = writeln!(out, "{pad}\t\t</entry>");
                } else {
                    render_json_value(child, ports, attr, indent + 2, path, targets, out);
                }
                path.pop();
                let _ = writeln!(out, "{pad}\t</entry>");
            }
            for target in targets.iter().filter(|target| target.owner == *path) {
                render_nested_target(target, indent + 1, out);
            }
            let _ = writeln!(out, "{pad}</entry>");
        }
    }
}

fn render_nested_target(target: &NestedTarget, indent: usize, out: &mut String) {
    let pad = "\t".repeat(indent);
    let _ = writeln!(
        out,
        "{pad}<entry name=\"property\" type=\"json-property\" inpkey=\"{}\" expanded=\"1\">",
        target.property_input
    );
    let _ = writeln!(
        out,
        "{pad}\t<entry name=\"name\" type=\"json-propertyname\" inpkey=\"{}\"/>",
        target.field.key_input
    );
    let _ = writeln!(
        out,
        "{pad}\t<entry name=\"{}\" inpkey=\"{}\"/>",
        json_type_name(target.value_type),
        target.field.value_input
    );
    let _ = writeln!(out, "{pad}</entry>");
}

fn render_source_entries(
    schema: &SchemaNode,
    ports: &PortTree,
    indent: usize,
    path: &[String],
    sites: &[&SourceSite],
) -> String {
    let mut out = String::new();
    if schema.repeating {
        let pad = "\t".repeat(indent);
        let _ = writeln!(out, "{pad}<entry name=\"array\" expanded=\"1\">");
        let _ = writeln!(
            out,
            "{pad}\t<entry name=\"item\" type=\"json-item\" expanded=\"1\">"
        );
        render_source_value(
            schema,
            ports,
            indent + 2,
            &mut path.to_vec(),
            sites,
            &mut out,
        );
        let _ = writeln!(out, "{pad}\t</entry>");
        let _ = writeln!(out, "{pad}</entry>");
    } else {
        render_source_value(schema, ports, indent, &mut path.to_vec(), sites, &mut out);
    }
    out
}

fn render_source_value(
    node: &SchemaNode,
    ports: &PortTree,
    indent: usize,
    path: &mut Vec<String>,
    sites: &[&SourceSite],
    out: &mut String,
) {
    let pad = "\t".repeat(indent);
    let Some(key) = ports.key_for_abs(path) else {
        return;
    };
    match &node.kind {
        SchemaKind::Scalar { ty } => {
            let _ = writeln!(
                out,
                "{pad}<entry name=\"{}\" outkey=\"{key}\"/>",
                json_type_name(*ty)
            );
        }
        SchemaKind::Group { children, .. } => {
            let _ = writeln!(
                out,
                "{pad}<entry name=\"object\" outkey=\"{key}\" expanded=\"1\">"
            );
            for child in children {
                let _ = writeln!(
                    out,
                    "{pad}\t<entry name=\"{}\" type=\"json-property\" expanded=\"1\">",
                    xml_escape(&child.name)
                );
                path.push(child.name.clone());
                if child.repeating {
                    let _ = writeln!(out, "{pad}\t\t<entry name=\"array\" expanded=\"1\">");
                    let _ = writeln!(
                        out,
                        "{pad}\t\t\t<entry name=\"item\" type=\"json-item\" expanded=\"1\">"
                    );
                    render_source_value(child, ports, indent + 4, path, sites, out);
                    let _ = writeln!(out, "{pad}\t\t\t</entry>");
                    let _ = writeln!(out, "{pad}\t\t</entry>");
                } else {
                    render_source_value(child, ports, indent + 2, path, sites, out);
                }
                path.pop();
                let _ = writeln!(out, "{pad}\t</entry>");
            }
            for site in sites.iter().filter(|site| site.owner == *path) {
                let _ = writeln!(
                    out,
                    "{pad}\t<entry name=\"property\" type=\"json-property\" expanded=\"1\">"
                );
                let _ = writeln!(
                    out,
                    "{pad}\t\t<entry name=\"name\" type=\"json-propertyname\" outkey=\"{}\"/>",
                    site.name_port
                );
                let _ = writeln!(
                    out,
                    "{pad}\t\t<entry name=\"boolean\" outkey=\"{}\"/>",
                    site.value_port
                );
                let _ = writeln!(out, "{pad}\t</entry>");
            }
            let _ = writeln!(out, "{pad}</entry>");
        }
    }
}

fn json_type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "number",
        ScalarType::Bool => "boolean",
    }
}
