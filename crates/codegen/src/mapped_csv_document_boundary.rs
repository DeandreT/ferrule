//! Explicit projection of constructed document wrappers, separate from the
//! unchanged-row CSV boundary. This does not admit filename acquisition or MFDs.

use std::fmt;

use ir::{SchemaKind, SchemaNode};
use mapping::Project;

use crate::{CsvOutputError, CsvOutputPolicy, IterationOutput, Program, TargetConstruction};

/// Explicit physical row selection. Only canonical, unconstrained scalar
/// targets are admitted; fixed/default values are neither enforced nor filled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvMappedDocumentOutputPolicy {
    pub rows_field: String,
    pub target: CsvOutputPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvMappedDocumentOutputError {
    Output(CsvOutputError),
    InvalidRowsField,
    Schema { reason: &'static str },
    Scope,
}

impl fmt::Display for CsvMappedDocumentOutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Output(error) => error.fmt(f),
            Self::InvalidRowsField => f.write_str("CSV mapped document row field must be nonempty and NUL-free"),
            Self::Schema { reason } => write!(f, "unsupported CSV mapped document schema: {reason}"),
            Self::Scope => f.write_str("CSV mapped documents require constructed dynamic documents with one repeating constructed row child"),
        }
    }
}

impl std::error::Error for CsvMappedDocumentOutputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Output(error) => Some(error),
            _ => None,
        }
    }
}

impl From<CsvOutputError> for CsvMappedDocumentOutputError {
    fn from(error: CsvOutputError) -> Self {
        Self::Output(error)
    }
}

impl CsvMappedDocumentOutputPolicy {
    /// The caller selects the actual field explicitly. Validate the entire
    /// wrapper; never infer a conventional field name or ignore sibling fields.
    pub fn from_project(
        project: &Project,
        rows_field: impl Into<String>,
    ) -> Result<Self, CsvMappedDocumentOutputError> {
        let policy = Self {
            rows_field: rows_field.into(),
            target: CsvOutputPolicy::from_format_options(&project.target_options)?,
        };
        if !project.extra_sources.is_empty() {
            return Err(CsvOutputError::NamedInputs.into());
        }
        if !project.extra_targets.is_empty() {
            return Err(CsvOutputError::NamedOutputs.into());
        }
        policy.row_schema(&project.target)?;
        Ok(policy)
    }

    /// Return a bounded, canonical nonrepeating physical CSV row schema. Check
    /// kinds and name budgets before allocating; do not clone arbitrary trees.
    pub fn row_schema(
        &self,
        target: &SchemaNode,
    ) -> Result<SchemaNode, CsvMappedDocumentOutputError> {
        let bad = |reason| CsvMappedDocumentOutputError::Schema { reason };
        if self.rows_field.is_empty() || self.rows_field.contains('\0') {
            return Err(CsvMappedDocumentOutputError::InvalidRowsField);
        }
        let SchemaKind::Group { children, .. } = &target.kind else {
            return Err(bad("a closed document group is required"));
        };
        if children.len() != 1 || children[0].name != self.rows_field || !children[0].repeating {
            return Err(bad(
                "the wrapper must contain exactly the selected repeating row group",
            ));
        }
        let rows = &children[0];
        let SchemaKind::Group {
            children: columns, ..
        } = &rows.kind
        else {
            return Err(bad("the selected field must be a flat row group"));
        };
        let bytes = columns.iter().fold(
            target.name.len().saturating_add(rows.name.len()),
            |sum, column| sum.saturating_add(column.name.len()),
        );
        if columns.is_empty()
            || columns.len() > 256
            || bytes > crate::MAX_EMBEDDED_JSON_SCHEMA_BYTES
        {
            return Err(bad("CSV column or schema-name byte limit exceeded"));
        }
        let mut physical = Vec::with_capacity(columns.len());
        for column in columns {
            let SchemaKind::Scalar { ty } = column.kind else {
                return Err(bad("each column must have one nonrepeating scalar type"));
            };
            let canonical = SchemaNode::scalar(&column.name, ty);
            if *column != canonical {
                return Err(bad("column metadata must use canonical defaults"));
            }
            physical.push(canonical);
        }
        let row = SchemaNode::group(&rows.name, physical);
        crate::csv_input_boundary::source_fields(&row)
            .map_err(|_| bad("flat row names and metadata must be canonical and unique"))?;
        let canonical_rows = row.clone().repeating();
        if *rows != canonical_rows
            || *target != SchemaNode::group(&target.name, vec![canonical_rows])
        {
            return Err(bad(
                "wrapper and row metadata must use closed canonical defaults",
            ));
        }
        Ok(row)
    }
}

pub fn validate_csv_mapped_document_output(
    program: &Program,
    policy: &CsvMappedDocumentOutputPolicy,
) -> Result<(), CsvMappedDocumentOutputError> {
    crate::validate_program(program).map_err(CsvOutputError::InvalidProgram)?;
    policy.target.validate_dialect()?;
    if policy.target.raw_delimited_crlf {
        return Err(CsvOutputError::ConflictingFormatOptions.into());
    }
    if program.xml_boundary.is_some() {
        return Err(CsvOutputError::XmlBoundary.into());
    }
    if !program.extra_sources.is_empty() {
        return Err(CsvOutputError::NamedInputs.into());
    }
    if !program.extra_targets.is_empty() {
        return Err(CsvOutputError::NamedOutputs.into());
    }
    policy.row_schema(&program.target)?;
    let root = &program.root;
    let dynamic = root.iteration.as_ref().is_some_and(|iteration| {
        iteration.dynamic_document_iteration().is_some()
            && iteration.output() == IterationOutput::Repeated
    });
    if !dynamic
        || root.repeating
        || !root.target_field.is_empty()
        || root.construction != TargetConstruction::Group
        || !root.bindings.is_empty()
        || root.children.len() != 1
    {
        return Err(CsvMappedDocumentOutputError::Scope);
    }
    let rows = &root.children[0];
    if rows.target_field != policy.rows_field
        || !rows.repeating
        || rows.construction != TargetConstruction::Group
        || !rows.children.is_empty()
        || !rows.iteration.as_ref().is_some_and(|iteration| {
            iteration.source_iteration().is_some()
                && iteration.output() == IterationOutput::Repeated
        })
    {
        return Err(CsvMappedDocumentOutputError::Scope);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Binding, Expression, ExpressionNode, IterationPlan, NamedSourceProgram, NamedTargetProgram,
        TargetScope,
    };
    use ir::{ScalarType, Value};

    fn policy() -> CsvMappedDocumentOutputPolicy {
        CsvMappedDocumentOutputPolicy {
            rows_field: "DetailLines".into(),
            target: CsvOutputPolicy::default(),
        }
    }
    fn program() -> Program {
        Program {
            execution_contract: None,
            xml_boundary: None,
            source: SchemaNode::group("Input", vec![]).repeating(),
            extra_sources: vec![],
            extra_targets: vec![],
            target: SchemaNode::group(
                "Document",
                vec![
                    SchemaNode::group(
                        "DetailLines",
                        vec![SchemaNode::scalar("Total", ScalarType::Int)],
                    )
                    .repeating(),
                ],
            ),
            expressions: vec![
                ExpressionNode {
                    id: 1,
                    expression: Expression::Const {
                        value: Value::String("same.csv".into()),
                    },
                },
                ExpressionNode {
                    id: 2,
                    expression: Expression::Const {
                        value: Value::Int(7),
                    },
                },
            ],
            user_functions: vec![],
            failure_rules: vec![],
            root: TargetScope {
                target_field: String::new(),
                repeating: false,
                construction: TargetConstruction::Group,
                iteration: Some(IterationPlan::dynamic_documents(vec![], 1)),
                bindings: vec![],
                children: vec![TargetScope {
                    target_field: "DetailLines".into(),
                    repeating: true,
                    construction: TargetConstruction::Group,
                    iteration: Some(IterationPlan::source(vec![])),
                    children: vec![],
                    bindings: vec![Binding {
                        target_field: "Total".into(),
                        expression: 2,
                        target_domain: ScalarType::Int.into(),
                        repeating: false,
                    }],
                }],
            },
        }
    }

    #[test]
    fn mapped_admission_is_separate_from_copy_current_rows() {
        let program = program();
        validate_csv_mapped_document_output(&program, &policy()).unwrap();
        assert_eq!(
            crate::validate_csv_document_output(&program, &policy().target),
            Err(CsvOutputError::PrimaryRows)
        );
        let row = policy().row_schema(&program.target).unwrap();
        assert_eq!(
            row,
            SchemaNode::group(
                "DetailLines",
                vec![SchemaNode::scalar("Total", ScalarType::Int)]
            )
        );
        assert_eq!(program.target, self::program().target);
    }

    #[test]
    fn complete_schema_proof_refuses_metadata_extras_duplicates_and_constraints() {
        let policy = policy();
        let target = program().target;
        for location in 0..3 {
            for change in 0..7 {
                let mut bad = target.clone();
                let node = match location {
                    0 => &mut bad,
                    1 => {
                        let SchemaKind::Group { children, .. } = &mut bad.kind else {
                            unreachable!()
                        };
                        &mut children[0]
                    }
                    _ => {
                        let SchemaKind::Group { children, .. } = &mut bad.kind else {
                            unreachable!()
                        };
                        let SchemaKind::Group { children, .. } = &mut children[0].kind else {
                            unreachable!()
                        };
                        &mut children[0]
                    }
                };
                match change {
                    0 => node.fixed = Some("OK".into()),
                    1 => node.default = Some("OK".into()),
                    2 => node.nullable = true,
                    3 => node.json_array_wrapper = true,
                    4 => node.recursive_ref = Some("Other".into()),
                    5 => node.attribute = true,
                    _ => node.json_any = true,
                }
                assert!(
                    matches!(
                        policy.row_schema(&bad),
                        Err(CsvMappedDocumentOutputError::Schema { .. })
                    ),
                    "location={location} change={change}"
                );
            }
        }
        for names in [vec!["Total", "Total"], vec![""], vec![]] {
            let bad = SchemaNode::group(
                "Document",
                vec![
                    SchemaNode::group(
                        "DetailLines",
                        names
                            .into_iter()
                            .map(|name| SchemaNode::scalar(name, ScalarType::Int))
                            .collect(),
                    )
                    .repeating(),
                ],
            );
            assert!(policy.row_schema(&bad).is_err());
        }
        let mut extra = target.clone();
        let SchemaKind::Group { children, .. } = &mut extra.kind else {
            unreachable!()
        };
        children.push(SchemaNode::scalar("Spare", ScalarType::String));
        assert!(policy.row_schema(&extra).is_err());
        for repeating in [false, true] {
            let bad = SchemaNode::group(
                "Document",
                vec![if repeating {
                    SchemaNode::scalar("DetailLines", ScalarType::Int).repeating()
                } else {
                    SchemaNode::group("DetailLines", vec![])
                }],
            );
            assert!(policy.row_schema(&bad).is_err());
        }
        let mut alternatives = target.clone();
        let SchemaKind::Group { required, .. } = &mut alternatives.kind else {
            unreachable!()
        };
        required.push("DetailLines".into());
        assert!(policy.row_schema(&alternatives).is_err());
    }

    #[test]
    fn named_endpoints_and_other_scope_shapes_do_not_enter_new_profile() {
        let policy = policy();
        let mut named = program();
        named.extra_sources.push(NamedSourceProgram {
            name: "Other".into(),
            source: SchemaNode::group("Other", vec![]),
            dynamic: None,
        });
        assert_eq!(
            validate_csv_mapped_document_output(&named, &policy),
            Err(CsvOutputError::NamedInputs.into())
        );
        let mut named = program();
        named.extra_targets.push(NamedTargetProgram {
            name: "Other".into(),
            target: named.target.clone(),
            root: named.root.clone(),
        });
        assert_eq!(
            validate_csv_mapped_document_output(&named, &policy),
            Err(CsvOutputError::NamedOutputs.into())
        );
        let mut singleton = program();
        singleton.root.iteration = None;
        assert!(validate_csv_mapped_document_output(&singleton, &policy).is_err());
        let mut nested_documents = program();
        nested_documents.root.children[0].iteration =
            Some(IterationPlan::dynamic_documents(vec![], 1));
        assert!(validate_csv_mapped_document_output(&nested_documents, &policy).is_err());
        let mut first = program();
        first.root.children[0].iteration = Some(IterationPlan::new(
            crate::SourceIteration::new(vec![]),
            None,
            None,
            vec![],
            IterationOutput::First,
        ));
        assert!(validate_csv_mapped_document_output(&first, &policy).is_err());
    }
}
