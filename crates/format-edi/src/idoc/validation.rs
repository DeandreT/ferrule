//! Explicit IDoc validation; descriptor metadata never changes legacy I/O.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{IdocLayout, IdocNativeConfig, IdocNativeNode, IdocNativeStatus};

use crate::{EdiFormatError, MAX_RUNTIME_INPUT_BYTES};

const MAX_WORK: usize = 1_000_000;
const MAX_ISSUES: usize = 10_000;
const MAX_DIAGNOSTIC_BYTES: usize = 1024 * 1024;
const MAX_OUTPUT_WORK: usize = 100_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdocConstraintViolation {
    Occurrences {
        status: IdocNativeStatus,
        minimum: u64,
        maximum: u64,
        effective_minimum: u64,
        actual: usize,
    },
    NotAllowed {
        allowed_count: usize,
    },
}

impl fmt::Display for IdocConstraintViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Occurrences {
                status,
                minimum,
                maximum,
                effective_minimum,
                actual,
            } => write!(
                formatter,
                "has {actual} occurrence(s), expected {effective_minimum}..={maximum} ({}; configured LOOPMIN {minimum})",
                status.keyword()
            ),
            Self::NotAllowed { allowed_count } => write!(
                formatter,
                "is not one of the {allowed_count} configured code-list value(s)"
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdocValidationIssue {
    location: String,
    violation: IdocConstraintViolation,
}

impl IdocValidationIssue {
    pub fn location(&self) -> &str {
        &self.location
    }

    pub const fn violation(&self) -> IdocConstraintViolation {
        self.violation
    }
}

impl fmt::Display for IdocValidationIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "IDoc `{}` {}", self.location, self.violation)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdocValidationReport {
    issues: Vec<IdocValidationIssue>,
    diagnostic_bytes: usize,
    truncated: bool,
}

impl IdocValidationReport {
    pub fn issues(&self) -> &[IdocValidationIssue] {
        &self.issues
    }

    pub fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }

    /// Further checks stop after 10,000 issues or 1 MiB of location text.
    pub const fn truncated(&self) -> bool {
        self.truncated
    }

    fn push(&mut self, location: &str, violation: IdocConstraintViolation) {
        if self.issues.len() == MAX_ISSUES
            || location.len() > MAX_DIAGNOSTIC_BYTES - self.diagnostic_bytes
        {
            self.truncated = true;
            return;
        }
        self.diagnostic_bytes += location.len();
        self.issues.push(IdocValidationIssue {
            location: location.into(),
            violation,
        });
    }
}

impl fmt::Display for IdocValidationReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "IDoc validation found {} issue(s)",
            self.issues.len()
        )?;
        for issue in self.issues.iter().take(8) {
            write!(formatter, "; {issue}")?;
        }
        if self.issues.len() > 8 || self.truncated {
            formatter.write_str("; additional issues omitted")?;
        }
        Ok(())
    }
}

/// Checks a descriptor paired exactly with its projected schema and layout.
///
/// Ferrule's explicit validation policy accepts an absent OPTIONAL node;
/// LOOPMIN applies when it is present. A MANDATORY node requires at least one
/// occurrence, even if its configured LOOPMIN is zero. Counts are independent
/// for each parent occurrence, not summed across the document. These rules are
/// an explicit API contract, not evidence of native application conformance.
///
/// Missing and Null scalar fields are not code-list violations. Present values
/// use the writer's ordinary string lexical coercion; an empty String is checked
/// against any declared empty code. Field widths and wire text remain checked
/// by serialization. Work is capped at one million node/field visits and 64 MiB
/// of instance name/value bytes. Issue collection is separately bounded.
pub fn validate_native(
    schema: &SchemaNode,
    instance: &Instance,
    layout: &IdocLayout,
    descriptor: &IdocNativeConfig,
) -> Result<IdocValidationReport, EdiFormatError> {
    require_pair(schema, layout, descriptor)?;
    let plan = GroupPlan::new(descriptor.nodes())?;
    let mut validator = Validator::default();
    validator.group(&plan, instance, &mut String::new(), &schema.name)?;
    Ok(validator.report)
}

pub(super) fn require_pair(
    schema: &SchemaNode,
    layout: &IdocLayout,
    descriptor: &IdocNativeConfig,
) -> Result<(), EdiFormatError> {
    let matches = descriptor
        .project()
        .is_ok_and(|(projected_schema, projected_layout)| {
            &projected_schema == schema && &projected_layout == layout
        });
    if matches {
        Ok(())
    } else {
        Err(EdiFormatError::IdocDescriptorMismatch)
    }
}

pub(super) fn require_valid(
    schema: &SchemaNode,
    instance: &Instance,
    layout: &IdocLayout,
    descriptor: &IdocNativeConfig,
) -> Result<(), EdiFormatError> {
    let report = validate_native(schema, instance, layout, descriptor)?;
    if report.is_empty() && !report.truncated() {
        Ok(())
    } else {
        Err(EdiFormatError::IdocValidation(report))
    }
}

// The legacy parser materializes every configured field even for a short
// record. Bound matching and projected materialization before calling it, so
// the explicit API cannot expand tiny input into an unbounded instance tree.
pub(super) fn bound_input_work(
    bytes: &[u8],
    layout: &IdocLayout,
    descriptor: &IdocNativeConfig,
) -> Result<(), EdiFormatError> {
    if bytes.len() > MAX_RUNTIME_INPUT_BYTES {
        return Err(EdiFormatError::IdocLimit("input size"));
    }
    fn collect<'a>(
        nodes: &'a [IdocNativeNode],
        ancestors: usize,
        ancestor_bytes: usize,
        budgets: &mut HashMap<&'a str, (usize, usize)>,
    ) {
        for node in nodes {
            match node {
                IdocNativeNode::Group(group) => collect(
                    group.children(),
                    ancestors + 1,
                    ancestor_bytes + format!("SG{}", group.number()).len(),
                    budgets,
                ),
                IdocNativeNode::Segment(segment) => {
                    let names = segment
                        .fields()
                        .iter()
                        .map(|field| field.name().len())
                        .sum::<usize>();
                    budgets.insert(
                        segment.record_name(),
                        (
                            ancestors + segment.fields().len() + 1,
                            ancestor_bytes + names + segment.record_name().len(),
                        ),
                    );
                }
            }
        }
    }
    let mut budgets = HashMap::new();
    collect(descriptor.nodes(), 0, 0, &mut budgets);
    let mut budget = Validator::default();
    for record in super::records(bytes)
        .map(super::trim_record)
        .filter(|record| !record.is_empty())
    {
        budget.charge(1, 0)?;
        for segment in layout.segments() {
            let name = segment.name().as_bytes();
            budget.charge(1, record.len().min(name.len()))?;
            if record.starts_with(name)
                && record
                    .get(name.len())
                    .is_none_or(|byte| byte.is_ascii_whitespace())
            {
                if let Some(&(work, bytes)) = budgets.get(segment.name()) {
                    budget.charge(work, bytes)?;
                }
                break;
            }
        }
    }
    Ok(())
}

// Legacy shape validation and record emission use ordered linear searches.
// Account conservatively for their pairwise field/name comparisons before
// serialization. This guard belongs only to the explicit descriptor API.
pub(super) fn bound_output_work(
    schema: &SchemaNode,
    instance: &Instance,
    layout: &IdocLayout,
) -> Result<(), EdiFormatError> {
    fn visit(
        schema: &SchemaNode,
        instance: &Instance,
        segment_search: usize,
        remaining: &mut usize,
    ) -> Result<(), EdiFormatError> {
        if let Instance::Repeated(items) = instance {
            for item in items {
                visit(schema, item, segment_search, remaining)?;
            }
            return Ok(());
        }
        let (SchemaKind::Group { children, .. }, Instance::Group(fields)) =
            (&schema.kind, instance)
        else {
            return Ok(());
        };
        let count = children.len();
        let present = fields.len();
        let longest_name = children
            .iter()
            .map(|child| child.name.len())
            .chain(fields.iter().map(|(name, _)| name.len()))
            .max()
            .unwrap_or(1)
            .max(1);
        let work = count
            .checked_mul(present)
            .and_then(|work| work.checked_mul(4))
            .and_then(|work| {
                present
                    .checked_mul(present)
                    .and_then(|pairs| work.checked_add(pairs))
            })
            .and_then(|work| {
                count
                    .checked_mul(count)
                    .and_then(|pairs| work.checked_add(pairs))
            })
            .and_then(|work| work.checked_mul(longest_name))
            .and_then(|work| work.checked_add(segment_search))
            .ok_or(EdiFormatError::IdocLimit("validation output work"))?;
        *remaining = remaining
            .checked_sub(work)
            .ok_or(EdiFormatError::IdocLimit("validation output work"))?;
        for child in children {
            if let Some(value) = instance.field(&child.name) {
                visit(child, value, segment_search, remaining)?;
            }
        }
        Ok(())
    }
    let segment_search = layout
        .segments()
        .iter()
        .map(|segment| segment.name().len())
        .sum();
    let mut remaining = MAX_OUTPUT_WORK;
    visit(schema, instance, segment_search, &mut remaining)
}

struct GroupPlan<'a> {
    children: Vec<NodePlan<'a>>,
    indices: HashMap<String, usize>,
}

impl<'a> GroupPlan<'a> {
    fn new(nodes: &'a [IdocNativeNode]) -> Result<Self, EdiFormatError> {
        let children = nodes
            .iter()
            .map(NodePlan::new)
            .collect::<Result<Vec<_>, _>>()?;
        let indices: HashMap<_, _> = children
            .iter()
            .enumerate()
            .map(|(i, child)| (child.name.clone(), i))
            .collect();
        if indices.len() != children.len() {
            return Err(EdiFormatError::IdocDescriptorMismatch);
        }
        Ok(Self { children, indices })
    }
}

struct NodePlan<'a> {
    name: String,
    repeating: bool,
    status: IdocNativeStatus,
    minimum: u64,
    maximum: u64,
    content: Content<'a>,
}

enum Content<'a> {
    Group(GroupPlan<'a>),
    Segment {
        fields: Vec<FieldPlan<'a>>,
        indices: HashMap<&'a str, usize>,
    },
}

struct FieldPlan<'a> {
    name: &'a str,
    codes: HashSet<&'a str>,
}

impl<'a> NodePlan<'a> {
    fn new(node: &'a IdocNativeNode) -> Result<Self, EdiFormatError> {
        let (name, status, minimum, maximum, content) = match node {
            IdocNativeNode::Group(group) => (
                format!("SG{}", group.number()),
                group.status(),
                group.loop_min(),
                group.loop_max(),
                Content::Group(GroupPlan::new(group.children())?),
            ),
            IdocNativeNode::Segment(segment) => (
                segment.record_name().into(),
                segment.status(),
                segment.loop_min(),
                segment.loop_max(),
                Content::Segment {
                    fields: segment
                        .fields()
                        .iter()
                        .map(|field| FieldPlan {
                            name: field.name(),
                            codes: field.codes().iter().map(|code| code.value()).collect(),
                        })
                        .collect(),
                    indices: segment
                        .fields()
                        .iter()
                        .enumerate()
                        .map(|(i, field)| (field.name(), i))
                        .collect(),
                },
            ),
        };
        Ok(Self {
            name,
            repeating: status.is_optional() || maximum > 1,
            status,
            minimum,
            maximum,
            content,
        })
    }
}

#[derive(Default)]
struct Validator {
    work: usize,
    bytes: usize,
    report: IdocValidationReport,
}

impl Validator {
    fn charge(&mut self, work: usize, bytes: usize) -> Result<(), EdiFormatError> {
        self.work = self
            .work
            .checked_add(work)
            .ok_or(EdiFormatError::IdocLimit("validation work"))?;
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or(EdiFormatError::IdocLimit("validation bytes"))?;
        if self.work > MAX_WORK {
            return Err(EdiFormatError::IdocLimit("validation work"));
        }
        if self.bytes > MAX_RUNTIME_INPUT_BYTES {
            return Err(EdiFormatError::IdocLimit("validation bytes"));
        }
        Ok(())
    }

    fn index_fields<'a, K>(
        &mut self,
        instance: &'a Instance,
        indices: &HashMap<K, usize>,
        group: &str,
    ) -> Result<HashMap<usize, &'a Instance>, EdiFormatError>
    where
        K: std::borrow::Borrow<str> + std::hash::Hash + Eq,
    {
        self.charge(1, 0)?;
        let Instance::Group(fields) = instance else {
            return Err(shape(group, "a group", instance));
        };
        self.charge(fields.len(), 0)?;
        let mut values = HashMap::with_capacity(fields.len());
        for (name, instance) in fields {
            self.charge(0, name.len())?;
            let Some(index) = indices.get(name.as_str()) else {
                return Err(EdiFormatError::UnexpectedField {
                    group: group.into(),
                    field: name.clone(),
                });
            };
            if values.insert(*index, instance).is_some() {
                return Err(EdiFormatError::DuplicateField {
                    group: group.into(),
                    field: name.clone(),
                });
            }
        }
        Ok(values)
    }

    fn group(
        &mut self,
        plan: &GroupPlan<'_>,
        instance: &Instance,
        path: &mut String,
        name: &str,
    ) -> Result<(), EdiFormatError> {
        let values = self.index_fields(instance, &plan.indices, name)?;
        for (index, node) in plan.children.iter().enumerate() {
            if self.report.truncated {
                break;
            }
            let base = append_path(path, &node.name);
            self.node(node, values.get(&index).copied(), path)?;
            path.truncate(base);
        }
        Ok(())
    }

    fn node(
        &mut self,
        plan: &NodePlan<'_>,
        instance: Option<&Instance>,
        path: &mut String,
    ) -> Result<(), EdiFormatError> {
        self.charge(1, 0)?;
        let items = match instance {
            None => &[][..],
            Some(Instance::Repeated(items)) if plan.repeating => items.as_slice(),
            Some(value) if !plan.repeating && !matches!(value, Instance::Repeated(_)) => {
                std::slice::from_ref(value)
            }
            Some(value) => {
                return Err(shape(
                    &plan.name,
                    if plan.repeating {
                        "repeating values"
                    } else {
                        "one value"
                    },
                    value,
                ));
            }
        };
        let actual = items.len();
        let effective_minimum = if plan.status == IdocNativeStatus::Mandatory {
            plan.minimum.max(1)
        } else if actual == 0 {
            0
        } else {
            plan.minimum
        };
        if (actual as u64) < effective_minimum || (actual as u64) > plan.maximum {
            self.report.push(
                path,
                IdocConstraintViolation::Occurrences {
                    status: plan.status,
                    minimum: plan.minimum,
                    maximum: plan.maximum,
                    effective_minimum,
                    actual,
                },
            );
        }
        self.charge(items.len(), 0)?;
        for (index, item) in items.iter().enumerate() {
            if self.report.truncated {
                break;
            }
            let base = path.len();
            if plan.repeating {
                use fmt::Write as _;
                let _ = write!(path, "[{}]", index + 1);
            }
            match &plan.content {
                Content::Group(group) => self.group(group, item, path, &plan.name)?,
                Content::Segment { fields, indices } => {
                    self.segment(fields, indices, item, path, &plan.name)?
                }
            }
            path.truncate(base);
        }
        Ok(())
    }

    fn segment(
        &mut self,
        fields: &[FieldPlan<'_>],
        indices: &HashMap<&str, usize>,
        instance: &Instance,
        path: &mut String,
        name: &str,
    ) -> Result<(), EdiFormatError> {
        let values = self.index_fields(instance, indices, name)?;
        for (index, field) in fields.iter().enumerate() {
            if self.report.truncated {
                break;
            }
            self.charge(1, 0)?;
            let Some(instance) = values.get(&index) else {
                continue;
            };
            let Instance::Scalar(value) = instance else {
                return Err(shape(field.name, "a scalar", instance));
            };
            let Some(lexical) = lexical(value, field.name)? else {
                continue;
            };
            self.charge(0, lexical.len())?;
            if !field.codes.is_empty() && !field.codes.contains(lexical.as_ref()) {
                let base = append_path(path, field.name);
                self.report.push(
                    path,
                    IdocConstraintViolation::NotAllowed {
                        allowed_count: field.codes.len(),
                    },
                );
                path.truncate(base);
            }
        }
        Ok(())
    }
}

fn append_path(path: &mut String, name: &str) -> usize {
    let base = path.len();
    if !path.is_empty() {
        path.push('/');
    }
    path.push_str(name);
    base
}

fn lexical<'a>(value: &'a Value, name: &str) -> Result<Option<Cow<'a, str>>, EdiFormatError> {
    Ok(match value {
        Value::Null | Value::JsonNull(_) => None,
        Value::String(value) => Some(Cow::Borrowed(value)),
        Value::Bool(value) => Some(Cow::Owned(value.to_string())),
        Value::Int(value) => Some(Cow::Owned(value.to_string())),
        Value::Float(value) if value.is_finite() => Some(Cow::Owned(value.to_string())),
        Value::Float(_) => {
            return Err(EdiFormatError::NonFiniteFloat {
                element: name.into(),
            });
        }
        Value::XmlNil(_) => {
            return Err(EdiFormatError::ValueType {
                element: name.into(),
                expected: ScalarType::String,
                got: value.type_name(),
            });
        }
    })
}

fn shape(name: &str, expected: &'static str, instance: &Instance) -> EdiFormatError {
    EdiFormatError::InstanceShape {
        name: name.into(),
        expected,
        got: match instance {
            Instance::Scalar(_) => "a scalar",
            Instance::Group(_) => "a group",
            Instance::Repeated(_) => "repeating values",
            Instance::MappedSequence(_) => "a mapped sequence",
            Instance::DocumentSet(_) => "a document set",
        },
    }
}

#[cfg(test)]
mod tests;
