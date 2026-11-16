use ir::{SchemaKind, SchemaNode};
use mapping::{FormatOptions, Graph, Node, Scope};
use thiserror::Error;

use crate::{DisplayText, NativeLocation, Project, ProjectFileLayout, ValidationEndpoint};

const MAXIMUM_STRUCTURE_DEPTH: usize = 32;

/// Bounds structural inspection and the display text and pointers returned to the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InspectionLimits {
    pub maximum_items: u32,
    pub maximum_text_bytes: usize,
}

impl Default for InspectionLimits {
    fn default() -> Self {
        Self {
            maximum_items: 8_192,
            maximum_text_bytes: 2_048,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum InspectionError {
    #[error("The native model exceeds the inspection item limit.")]
    ItemLimitExceeded,
    #[error("Inspection item count overflowed.")]
    ItemCountOverflow,
    #[error("The schema exceeds the inspection depth limit.")]
    SchemaDepthExceeded,
    #[error("The scope exceeds the inspection depth limit.")]
    ScopeDepthExceeded,
}

/// Stable identity within a project snapshot. Named indices are declaration indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryId {
    Source,
    NamedSource(usize),
    Target,
    NamedTarget(usize),
}

impl BoundaryId {
    pub(crate) fn from_endpoint(endpoint: &ValidationEndpoint) -> Self {
        match endpoint {
            ValidationEndpoint::Source => Self::Source,
            ValidationEndpoint::NamedSource { index, .. } => Self::NamedSource(*index),
            ValidationEndpoint::Target => Self::Target,
            ValidationEndpoint::NamedTarget { index, .. } => Self::NamedTarget(*index),
        }
    }

    pub(crate) fn pointer(self) -> String {
        match self {
            Self::Source => "/source".into(),
            Self::NamedSource(index) => format!("/extra_sources/{index}"),
            Self::Target => "/target".into(),
            Self::NamedTarget(index) => format!("/extra_targets/{index}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryShape {
    Scalar,
    ScalarUnion,
    Object,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Boundary {
    pub id: BoundaryId,
    pub name: DisplayText,
    pub root_name: DisplayText,
    pub shape: BoundaryShape,
    pub repeating: bool,
    pub native_location: NativeLocation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResourceKind {
    DesignTimeInput,
    DesignTimeOutput,
    DynamicInput,
    DynamicOutput,
    EdiConfiguration,
    ExternalSource,
    XmlSchemaHint,
    SchemaReference,
}

/// An unresolved resource. Its identity and location are exposed without its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceReference {
    pub boundary_id: BoundaryId,
    pub kind: ResourceKind,
    pub native_location: NativeLocation,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StructuralCounts {
    pub schema_nodes: u32,
    pub graph_nodes: u32,
    pub scope_nodes: u32,
    pub user_functions: u32,
    pub failure_rules: u32,
    /// All charged items, including references and dependencies.
    pub total_items: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspection {
    /// Primary input, named inputs, primary output, then named outputs.
    pub boundaries: Vec<Boundary>,
    pub counts: StructuralCounts,
    pub unresolved_resources: Vec<ResourceReference>,
    /// True if any boundary display name was shortened.
    pub truncated: bool,
}

/// Inspect a project without executing it or resolving any resources.
///
/// The layout must describe the file whose native locations the caller will use.
/// Schema and scope depth are each limited to 32 levels below their roots.
pub fn inspect(
    project: &Project,
    layout: ProjectFileLayout,
    limits: InspectionLimits,
) -> Result<Inspection, InspectionError> {
    let mut inventory = Inventory {
        counts: StructuralCounts::default(),
        boundaries: Vec::new(),
        unresolved_resources: Vec::new(),
        layout,
        limits,
    };
    inventory.scan_structure(project)?;
    inventory.add_boundary(
        BoundaryId::Source,
        &project.source.name,
        &project.source,
        &project.source_options,
        project.source_path.is_some(),
        false,
    )?;
    for (index, source) in project.extra_sources.iter().enumerate() {
        inventory.add_boundary(
            BoundaryId::NamedSource(index),
            &source.name,
            &source.schema,
            &source.options,
            true,
            source.dynamic_path.is_some(),
        )?;
    }
    inventory.add_boundary(
        BoundaryId::Target,
        &project.target.name,
        &project.target,
        &project.target_options,
        project.target_path.is_some(),
        project.root.output_path().is_some(),
    )?;
    for (index, target) in project.extra_targets.iter().enumerate() {
        inventory.add_boundary(
            BoundaryId::NamedTarget(index),
            &target.name,
            &target.schema,
            &target.options,
            target.path.is_some(),
            target.root.output_path().is_some(),
        )?;
    }
    Ok(Inspection {
        truncated: inventory
            .boundaries
            .iter()
            .any(|boundary| boundary.name.truncated || boundary.root_name.truncated),
        boundaries: inventory.boundaries,
        counts: inventory.counts,
        unresolved_resources: inventory.unresolved_resources,
    })
}

struct Inventory {
    counts: StructuralCounts,
    boundaries: Vec<Boundary>,
    unresolved_resources: Vec<ResourceReference>,
    layout: ProjectFileLayout,
    limits: InspectionLimits,
}

impl Inventory {
    fn charge(&mut self, count: usize) -> Result<u32, InspectionError> {
        let count = u32::try_from(count).map_err(|_| InspectionError::ItemLimitExceeded)?;
        let total = self
            .counts
            .total_items
            .checked_add(count)
            .ok_or(InspectionError::ItemCountOverflow)?;
        if total > self.limits.maximum_items {
            return Err(InspectionError::ItemLimitExceeded);
        }
        self.counts.total_items = total;
        Ok(count)
    }

    fn scan_structure(&mut self, project: &Project) -> Result<(), InspectionError> {
        self.scan_graph(&project.graph)?;
        self.scan_scope(&project.root)?;
        for target in &project.extra_targets {
            self.scan_scope(&target.root)?;
        }
        self.counts.failure_rules = self.charge(project.failure_rules.len())?;
        self.counts.user_functions = self.charge(project.user_functions.len())?;
        for function in project.user_functions.values() {
            self.charge(function.parameters.len())?;
            self.scan_graph(&function.body)?;
        }
        Ok(())
    }

    fn scan_graph(&mut self, graph: &Graph) -> Result<(), InspectionError> {
        self.counts.graph_nodes += self.charge(graph.nodes.len())?;
        for node in graph.nodes.values() {
            self.charge(node.dependencies().len())?;
            match node {
                Node::ValueMap { table, .. } => {
                    self.charge(table.len().saturating_mul(2))?;
                }
                Node::XmlSerialize { schema, .. } => self.scan_schema(schema)?,
                Node::OwnedXmlSerialize(serialize) => self.scan_schema(&serialize.schema)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn scan_schema(&mut self, root: &SchemaNode) -> Result<(), InspectionError> {
        let mut pending = vec![(root, 0)];
        while let Some((schema, depth)) = pending.pop() {
            if depth > MAXIMUM_STRUCTURE_DEPTH {
                return Err(InspectionError::SchemaDepthExceeded);
            }
            self.charge(1)?;
            self.counts.schema_nodes += 1;
            let child_depth = depth + 1;
            if let SchemaKind::Group {
                children,
                alternatives,
                required,
                xml_restricted_alternatives,
                dynamic,
            } = &schema.kind
            {
                self.charge(
                    alternatives.len() + required.len() + xml_restricted_alternatives.len(),
                )?;
                for alternative in alternatives {
                    self.charge(
                        alternative.members.len()
                            + alternative.required.len()
                            + alternative.constraints.len(),
                    )?;
                }
                pending.extend(children.iter().map(|child| (child, child_depth)));
                pending.extend(dynamic.as_deref().map(|child| (child, child_depth)));
            }
            if let Some(contains) = &schema.json_contains {
                self.charge(contains.as_slice().len())?;
                pending.extend(contains.as_slice().iter().filter_map(|term| {
                    term.predicate()
                        .as_schema()
                        .map(|child| (child, child_depth))
                }));
            }
            if let Some(dependent) = &schema.json_dependent_schemas {
                self.charge(dependent.as_slice().len())?;
                pending.extend(dependent.as_slice().iter().filter_map(|term| {
                    term.predicate()
                        .as_schema()
                        .map(|child| (child, child_depth))
                }));
            }
        }
        Ok(())
    }

    fn scan_scope(&mut self, root: &Scope) -> Result<(), InspectionError> {
        let mut pending = vec![(root, 0)];
        while let Some((scope, depth)) = pending.pop() {
            if depth > MAXIMUM_STRUCTURE_DEPTH {
                return Err(InspectionError::ScopeDepthExceeded);
            }
            self.charge(1)?;
            self.counts.scope_nodes += 1;
            self.charge(
                scope.bindings.len()
                    + scope.dynamic_bindings.len()
                    + scope.dynamic_children.len()
                    + scope.windows.len()
                    + scope.sort_then_by.len(),
            )?;
            let children = scope
                .children
                .iter()
                .chain(scope.dynamic_children.iter().map(|child| &child.scope))
                .chain(
                    scope
                        .concatenated()
                        .into_iter()
                        .flat_map(|segments| segments.iter()),
                );
            pending.extend(children.map(|child| (child, depth + 1)));
        }
        Ok(())
    }

    fn add_boundary(
        &mut self,
        id: BoundaryId,
        name: &str,
        schema: &SchemaNode,
        options: &FormatOptions,
        has_design_path: bool,
        has_dynamic_path: bool,
    ) -> Result<(), InspectionError> {
        self.charge(1)?;
        self.scan_schema(schema)?;
        let maximum_text = self.limits.maximum_text_bytes;
        let boundary_path = id.pointer();
        self.boundaries.push(Boundary {
            id,
            name: DisplayText::bounded(name, maximum_text),
            root_name: DisplayText::bounded(&schema.name, maximum_text),
            shape: match schema.kind {
                SchemaKind::Scalar { .. } => BoundaryShape::Scalar,
                SchemaKind::ScalarUnion { .. } => BoundaryShape::ScalarUnion,
                SchemaKind::Group { .. } => BoundaryShape::Object,
            },
            repeating: schema.repeating,
            native_location: NativeLocation::pointer(self.layout, &boundary_path, maximum_text),
        });

        let (options_path, design_path, dynamic_path) = match id {
            BoundaryId::Source => (
                "/source_options".into(),
                "/source_path".into(),
                String::new(),
            ),
            BoundaryId::Target => (
                "/target_options".into(),
                "/target_path".into(),
                "/root/output_path".into(),
            ),
            BoundaryId::NamedSource(_) => (
                format!("{boundary_path}/options"),
                format!("{boundary_path}/path"),
                format!("{boundary_path}/dynamic_path"),
            ),
            BoundaryId::NamedTarget(_) => (
                format!("{boundary_path}/options"),
                format!("{boundary_path}/path"),
                format!("{boundary_path}/root/output_path"),
            ),
        };
        let (design_kind, dynamic_kind) = match id {
            BoundaryId::Source | BoundaryId::NamedSource(_) => {
                (ResourceKind::DesignTimeInput, ResourceKind::DynamicInput)
            }
            BoundaryId::Target | BoundaryId::NamedTarget(_) => {
                (ResourceKind::DesignTimeOutput, ResourceKind::DynamicOutput)
            }
        };
        if has_design_path {
            self.add_resource(id, design_kind, &design_path)?;
        }
        if has_dynamic_path {
            self.add_resource(id, dynamic_kind, &dynamic_path)?;
        }
        for (present, kind, field) in [
            (
                options.edi_config_reference.is_some(),
                ResourceKind::EdiConfiguration,
                "edi_config_reference",
            ),
            (
                options.external_source.is_some(),
                ResourceKind::ExternalSource,
                "external_source",
            ),
            (
                options.xml_schema_hints.is_some(),
                ResourceKind::XmlSchemaHint,
                "xml_schema_hints",
            ),
            (
                options.json_schema_unresolved_reference.is_some(),
                ResourceKind::SchemaReference,
                "json_schema_unresolved_reference",
            ),
        ] {
            if present {
                self.add_resource(id, kind, &format!("{options_path}/{field}"))?;
            }
        }
        Ok(())
    }

    fn add_resource(
        &mut self,
        boundary_id: BoundaryId,
        kind: ResourceKind,
        path: &str,
    ) -> Result<(), InspectionError> {
        self.charge(1)?;
        self.unresolved_resources.push(ResourceReference {
            boundary_id,
            kind,
            native_location: NativeLocation::pointer(
                self.layout,
                path,
                self.limits.maximum_text_bytes,
            ),
        });
        Ok(())
    }
}
