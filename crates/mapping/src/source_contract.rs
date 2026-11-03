//! Closed primary-tree admission; never a replacement for source resolution.
mod model;

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};

pub const PRIMARY_SOURCE_SAFETY_GUARD_V1_CAPABILITY: &str = "primary-source-safety-guard-v1";
pub const MAX_SOURCE_CONTRACT_DEPTH: usize = 256;
pub const MAX_SOURCE_CONTRACT_SHAPE_NODES: usize = 65_536;
pub const MAX_SOURCE_CONTRACT_METADATA_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_SOURCE_CONTRACT_VISITS: usize = 1_000_000;
pub const MAX_SOURCE_CONTRACT_PATH_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceContractReason {
    UnsupportedModel,
    UnsupportedSchema,
    UnprovedOwner,
    UnexpectedBinding,
    UnexpectedParameter,
    UnexpectedField,
    DuplicateField,
    MissingRepeatedSlot,
    WrongShape,
    WrongScalarDomain,
    ResourceLimit,
}

/// Contains bounded declared names only, never input scalar payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceContractViolation {
    pub path: Vec<String>,
    pub reason: SourceContractReason,
}

impl std::fmt::Display for SourceContractViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "primary source contract at `{}`: {:?}",
            self.path.join("/"),
            self.reason
        )
    }
}
impl std::error::Error for SourceContractViolation {}

pub(super) fn failure(reason: SourceContractReason) -> SourceContractViolation {
    SourceContractViolation {
        path: Vec::new(),
        reason,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimarySourceShapeNode {
    pub name: String,
    pub repeating: bool,
    pub kind: PrimarySourceShapeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimarySourceShapeKind {
    /// Indexes into the plan, sorted by the exact declared name.
    Group {
        children: Vec<usize>,
    },
    Scalar {
        ty: ScalarType,
        nillable: bool,
    },
}

/// Compiled from the current schema, never deserialized as trusted authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedPrimarySourcePlan {
    nodes: Vec<PrimarySourceShapeNode>,
    parents: Vec<Option<usize>>,
    #[cfg(test)]
    lookups: std::cell::Cell<usize>,
}

impl ClosedPrimarySourcePlan {
    pub fn compile(source: &SchemaNode) -> Result<Self, SourceContractViolation> {
        use SourceContractReason::{ResourceLimit, UnsupportedSchema};
        if source.repeating || source.attribute || !matches!(source.kind, SchemaKind::Group { .. })
        {
            return Err(failure(UnsupportedSchema));
        }
        let mut plan = Self {
            nodes: Vec::new(),
            parents: Vec::new(),
            #[cfg(test)]
            lookups: std::cell::Cell::new(0),
        };
        let mut pending = vec![(source, None, 0_usize, 0_usize)];
        let mut metadata = 0_usize;
        while let Some((node, parent, depth, path_bytes)) = pending.pop() {
            let bytes = path_bytes
                .checked_add(node.name.len())
                .ok_or_else(|| failure(ResourceLimit))?;
            if depth > MAX_SOURCE_CONTRACT_DEPTH
                || bytes > MAX_SOURCE_CONTRACT_PATH_BYTES
                || plan.nodes.len() >= MAX_SOURCE_CONTRACT_SHAPE_NODES
            {
                return Err(failure(ResourceLimit));
            }
            if !ir::primary_root_ncname_is_valid(&node.name)
                || node
                    .xml_namespace
                    .as_ref()
                    .is_some_and(|namespace| namespace.uri().is_some())
                || !node.xml_name_alternatives.is_empty()
                || node.xml_wildcard_namespace.is_some()
                || node.xml_wildcard_excluded_names.is_some()
                || node.recursive_ref.is_some()
                || node.text
                || node.json_array_wrapper
                || node.json_tuple.is_some()
                || node.json_any
                || node.json_undeclared_property
                || node.container_nullable
                || node.json_object_item_nullable.is_some()
                || node.xml_type_alternatives
                || node.xml_default_type.is_some()
                || !node.xml_repeating_sequences.is_empty()
                || !node.xml_repeating_choices.is_empty()
                || node.database_relation.is_some()
                || node.alternative_mode != ir::GroupAlternativeMode::Exclusive
                || node.xml_alternative_kind != ir::XmlAlternativeKind::XsiType
            {
                return Err(failure(UnsupportedSchema));
            }
            let (kind, children) = match &node.kind {
                SchemaKind::Scalar { ty } if !node.repeating => (
                    PrimarySourceShapeKind::Scalar {
                        ty: *ty,
                        nillable: node.nillable,
                    },
                    &[][..],
                ),
                SchemaKind::Group {
                    children,
                    alternatives,
                    xml_restricted_alternatives,
                    dynamic,
                    ..
                } if !node.attribute
                    && alternatives.is_empty()
                    && xml_restricted_alternatives.is_empty()
                    && dynamic.is_none() =>
                {
                    (
                        PrimarySourceShapeKind::Group {
                            children: Vec::new(),
                        },
                        children.as_slice(),
                    )
                }
                _ => return Err(failure(UnsupportedSchema)),
            };
            let slots = plan
                .nodes
                .len()
                .checked_add(pending.len())
                .and_then(|n| n.checked_add(children.len()))
                .and_then(|n| n.checked_add(1))
                .ok_or_else(|| failure(ResourceLimit))?;
            if slots > MAX_SOURCE_CONTRACT_SHAPE_NODES {
                return Err(failure(ResourceLimit));
            }
            metadata = metadata
                .checked_add(node.name.len())
                .and_then(|n| {
                    n.checked_add(
                        std::mem::size_of::<PrimarySourceShapeNode>()
                            + 2 * std::mem::size_of::<usize>(),
                    )
                })
                .ok_or_else(|| failure(ResourceLimit))?;
            if metadata > MAX_SOURCE_CONTRACT_METADATA_BYTES {
                return Err(failure(ResourceLimit));
            }
            let index = plan.nodes.len();
            plan.nodes.push(PrimarySourceShapeNode {
                name: node.name.clone(),
                repeating: node.repeating,
                kind,
            });
            plan.parents.push(parent);
            if let Some(parent) = parent
                && let PrimarySourceShapeKind::Group { children } = &mut plan.nodes[parent].kind
            {
                children.push(index);
            }
            pending.extend(
                children
                    .iter()
                    .rev()
                    .map(|child| (child, Some(index), depth + 1, bytes)),
            );
        }
        // Sort borrowed indexes, not names or scalar data; detect every slot collision.
        for index in 0..plan.nodes.len() {
            let PrimarySourceShapeKind::Group { children } = &mut plan.nodes[index].kind else {
                continue;
            };
            let mut children = std::mem::take(children);
            children.sort_unstable_by(|&a, &b| plan.nodes[a].name.cmp(&plan.nodes[b].name));
            if children
                .windows(2)
                .any(|pair| plan.nodes[pair[0]].name == plan.nodes[pair[1]].name)
            {
                return Err(plan.at(index, UnsupportedSchema));
            }
            plan.nodes[index].kind = PrimarySourceShapeKind::Group { children };
        }
        Ok(plan)
    }

    pub fn nodes(&self) -> &[PrimarySourceShapeNode] {
        &self.nodes
    }

    fn at(&self, mut index: usize, reason: SourceContractReason) -> SourceContractViolation {
        let mut path = Vec::new();
        while let Some(parent) = self.parents[index] {
            path.push(self.nodes[index].name.clone());
            index = parent;
        }
        path.reverse();
        SourceContractViolation { path, reason }
    }

    fn child(&self, parent: usize, name: &str) -> Option<usize> {
        #[cfg(test)]
        self.lookups.set(self.lookups.get() + 1);
        let PrimarySourceShapeKind::Group { children } = &self.nodes[parent].kind else {
            return None;
        };
        let slot = children
            .binary_search_by(|&index| self.nodes[index].name.as_str().cmp(name))
            .ok()?;
        Some(children[slot])
    }

    fn exact(&self, path: &[String]) -> Option<usize> {
        if path.len() > MAX_SOURCE_CONTRACT_DEPTH {
            return None;
        }
        path.iter()
            .try_fold(0, |index, name| self.child(index, name))
    }

    pub fn validate_driver(
        &self,
        active_collections: &[Vec<String>],
        driver: &[String],
    ) -> Result<Vec<String>, SourceContractViolation> {
        let owner = active_collections
            .last()
            .map_or(Some(0), |path| self.exact(path))
            .ok_or_else(|| failure(SourceContractReason::UnprovedOwner))?;
        let [name] = driver else {
            return Err(self.at(owner, SourceContractReason::UnprovedOwner));
        };
        let index = self
            .child(owner, name)
            .ok_or_else(|| self.at(owner, SourceContractReason::UnprovedOwner))?;
        if !self.nodes[index].repeating
            || !matches!(self.nodes[index].kind, PrimarySourceShapeKind::Group { .. })
        {
            return Err(self.at(index, SourceContractReason::UnprovedOwner));
        }
        Ok(self.at(index, SourceContractReason::UnprovedOwner).path)
    }

    pub fn validate_read(
        &self,
        path: &[String],
        frame: Option<&[String]>,
        active_collections: &[Vec<String>],
    ) -> Result<(), SourceContractViolation> {
        use SourceContractReason::UnprovedOwner;
        if path.is_empty()
            || path.len() > MAX_SOURCE_CONTRACT_DEPTH
            || active_collections.len() > MAX_SOURCE_CONTRACT_DEPTH
        {
            return Err(failure(UnprovedOwner));
        }
        let mut owner = 0;
        if let Some(pin) = frame {
            owner = self.exact(pin).ok_or_else(|| failure(UnprovedOwner))?;
            if !self.nodes[owner].repeating {
                return Err(self.at(owner, UnprovedOwner));
            }
            // Resolver predicates compare the concrete LOCAL collection label,
            // not the full schema path. The latest matching label is authoritative.
            let actual = active_collections.iter().rev().find(|collection| {
                collection
                    .last()
                    .is_some_and(|label| pin.last() == Some(label))
            });
            if actual.map(Vec::as_slice) != Some(pin) {
                return Err(self.at(owner, UnprovedOwner));
            }
        } else {
            for collection in active_collections {
                let index = self
                    .exact(collection)
                    .ok_or_else(|| failure(UnprovedOwner))?;
                if self.child(index, &path[0]).is_some()
                    || collection
                        .last()
                        .is_some_and(|label| path.first() == Some(label))
                {
                    return Err(self.at(index, UnprovedOwner));
                }
            }
        }
        let mut scalar = owner;
        for name in path {
            scalar = self
                .child(scalar, name)
                .ok_or_else(|| self.at(owner, UnprovedOwner))?;
            if self.nodes[scalar].repeating {
                return Err(self.at(scalar, UnprovedOwner));
            }
        }
        if !matches!(
            self.nodes[scalar].kind,
            PrimarySourceShapeKind::Scalar { .. }
        ) {
            return Err(self.at(scalar, UnprovedOwner));
        }
        // A pin must be the nearest repeated ancestor, not a singleton or a
        // different prefix with the same final spelling.
        let mut ancestor = self.parents[scalar];
        while let Some(index) = ancestor {
            if self.nodes[index].repeating {
                return if index == owner && frame.is_some() {
                    Ok(())
                } else {
                    Err(self.at(index, UnprovedOwner))
                };
            }
            ancestor = self.parents[index];
        }
        if frame.is_some() {
            Err(self.at(owner, UnprovedOwner))
        } else {
            Ok(())
        }
    }

    /// Borrowed iterative validation. No scalar data is cloned, coerced or logged.
    pub fn validate_input(
        &self,
        source: &Instance,
        supplied_extras: usize,
        supplied_parameters: usize,
    ) -> Result<(), SourceContractViolation> {
        self.validate_input_with_work_limit(
            source,
            supplied_extras,
            supplied_parameters,
            MAX_SOURCE_CONTRACT_VISITS,
        )
    }

    fn validate_input_with_work_limit(
        &self,
        source: &Instance,
        supplied_extras: usize,
        supplied_parameters: usize,
        work_limit: usize,
    ) -> Result<(), SourceContractViolation> {
        use SourceContractReason::*;
        if supplied_extras != 0 {
            return Err(failure(UnexpectedBinding));
        }
        if supplied_parameters != 0 {
            return Err(failure(UnexpectedParameter));
        }
        enum Task<'a> {
            Check(usize, &'a Instance, bool),
            Items(usize, std::slice::Iter<'a, Instance>),
            Fields(usize, &'a [(String, Instance)], usize, Vec<bool>),
        }
        let mut pending = vec![Task::Check(0, source, false)];
        let mut visits = 0_usize;
        while let Some(task) = pending.pop() {
            match task {
                Task::Check(index, value, member) => {
                    visits = visits
                        .checked_add(1)
                        .ok_or_else(|| self.at(index, ResourceLimit))?;
                    if visits > work_limit {
                        return Err(self.at(index, ResourceLimit));
                    }
                    let node = &self.nodes[index];
                    if node.repeating && !member {
                        let Instance::Repeated(items) = value else {
                            return Err(self.at(index, WrongShape));
                        };
                        if items.len() > work_limit - visits {
                            return Err(self.at(index, ResourceLimit));
                        }
                        pending.push(Task::Items(index, items.iter()));
                        continue;
                    }
                    match (&node.kind, value) {
                        (PrimarySourceShapeKind::Group { children }, Instance::Group(fields)) => {
                            if fields.len() > work_limit - visits {
                                return Err(self.at(index, ResourceLimit));
                            }
                            // Reserve schema-slot work before allocating the seen
                            // ledger. Wide sparse groups repeated many times must
                            // not multiply metadata scanning without a bound.
                            visits = visits
                                .checked_add(children.len())
                                .ok_or_else(|| self.at(index, ResourceLimit))?;
                            if visits > work_limit {
                                return Err(self.at(index, ResourceLimit));
                            }
                            pending.push(Task::Fields(
                                index,
                                fields,
                                0,
                                vec![false; children.len()],
                            ));
                        }
                        (
                            PrimarySourceShapeKind::Scalar { ty, nillable },
                            Instance::Scalar(value),
                        ) => {
                            let valid = match (ty, value) {
                                (_, Value::Null) => true,
                                (_, Value::XmlNil(_)) => *nillable,
                                (ScalarType::String, Value::String(_))
                                | (ScalarType::Int, Value::Int(_))
                                | (ScalarType::Bool, Value::Bool(_)) => true,
                                (ScalarType::Float, Value::Float(value)) => value.is_finite(),
                                _ => false,
                            };
                            if !valid {
                                return Err(self.at(index, WrongScalarDomain));
                            }
                        }
                        _ => return Err(self.at(index, WrongShape)),
                    }
                }
                Task::Items(index, mut items) => {
                    if let Some(item) = items.next() {
                        pending.push(Task::Items(index, items));
                        pending.push(Task::Check(index, item, true));
                    }
                }
                Task::Fields(index, fields, offset, mut seen) => {
                    let PrimarySourceShapeKind::Group { children } = &self.nodes[index].kind else {
                        return Err(self.at(index, WrongShape));
                    };
                    if let Some((name, value)) = fields.get(offset) {
                        let Ok(slot) = children
                            .binary_search_by(|&child| self.nodes[child].name.as_str().cmp(name))
                        else {
                            return Err(self.at(index, UnexpectedField));
                        };
                        let child = children[slot];
                        if seen[slot] {
                            return Err(self.at(child, DuplicateField));
                        }
                        seen[slot] = true;
                        pending.push(Task::Fields(index, fields, offset + 1, seen));
                        pending.push(Task::Check(child, value, false));
                    } else {
                        for (slot, &child) in children.iter().enumerate() {
                            if self.nodes[child].repeating && !seen[slot] {
                                return Err(self.at(child, MissingRepeatedSlot));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_wide_groups_reserve_slot_work_before_each_seen_allocation() {
        let schema = SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Row",
                    (0..8)
                        .map(|i| SchemaNode::scalar(format!("F{i}"), ScalarType::String))
                        .collect(),
                )
                .repeating(),
            ],
        );
        let plan = ClosedPrimarySourcePlan::compile(&schema).unwrap();
        let source = Instance::Group(
            vec![(
                "Row".into(),
                Instance::Repeated(vec![
                    Instance::Group(vec![].into()),
                    Instance::Group(vec![].into()),
                ]),
            )]
            .into(),
        );
        // Root check + one root slot + repeated wrapper + two group checks
        // + eight slots per group = 21. Twenty fails before the second ledger
        // allocation/scan, not after an uncounted sparse-group traversal.
        assert_eq!(
            plan.validate_input_with_work_limit(&source, 0, 0, 20)
                .unwrap_err()
                .reason,
            SourceContractReason::ResourceLimit
        );
        plan.validate_input_with_work_limit(&source, 0, 0, 21)
            .unwrap();
    }
}
