use std::collections::BTreeSet;
use std::fmt;

use ir::SchemaKind;
use mapping::{FormatOptions, TabularBoundaryKind};

use crate::{IterationOutput, Program, ProgramValidationError, TargetScope};

/// Literal policy for an explicitly selected generated flat CSV output.
/// Records use LF. The generated runtimes bound each UTF-8 document to 64 MiB.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvOutputPolicy {
    pub delimiter: Option<char>,
    pub quote: Option<char>,
    pub quote_disabled: bool,
    pub has_headers: bool,
    pub utf8_bom: bool,
}

impl Default for CsvOutputPolicy {
    fn default() -> Self {
        Self {
            delimiter: None,
            quote: None,
            quote_disabled: false,
            has_headers: true,
            utf8_bom: false,
        }
    }
}

impl CsvOutputPolicy {
    /// Capture native CSV writer settings without inferring the output format.
    /// The caller must explicitly select CSV. Conflicting physical adapters and
    /// unresolved text repairs are refused before generation.
    pub fn from_format_options(options: &FormatOptions) -> Result<Self, CsvOutputError> {
        if options.csv_text_repair_dependency.is_some() {
            return Err(CsvOutputError::RepairRequired);
        }
        let accepted = FormatOptions {
            delimiter: options.delimiter,
            csv_quote: options.csv_quote,
            csv_quote_disabled: options.csv_quote_disabled,
            csv_utf8_bom: options.csv_utf8_bom,
            // This input-only setting has no effect on native CSV output.
            csv_preserve_empty_strings: options.csv_preserve_empty_strings,
            has_header_row: options.has_header_row,
            tabular_kind: options
                .tabular_kind
                .filter(|kind| *kind == TabularBoundaryKind::Csv),
            ..FormatOptions::default()
        };
        if *options != accepted {
            return Err(CsvOutputError::ConflictingFormatOptions);
        }
        let policy = Self {
            delimiter: options.delimiter,
            quote: options.csv_quote,
            quote_disabled: options.csv_quote_disabled,
            has_headers: options.has_header_row.unwrap_or(true),
            utf8_bom: options.csv_utf8_bom,
        };
        policy.validate_dialect()?;
        Ok(policy)
    }

    fn validate_dialect(&self) -> Result<(), CsvOutputError> {
        let delimiter = self.delimiter.unwrap_or(',');
        if !delimiter.is_ascii() || matches!(delimiter, '\0' | '\r' | '\n') {
            return Err(CsvOutputError::BadDelimiter(delimiter));
        }
        if self.quote_disabled {
            if self.quote.is_some() {
                return Err(CsvOutputError::ConflictingQuoteSettings);
            }
            return Ok(());
        }
        let quote = self.quote.unwrap_or('"');
        if !('!'..='~').contains(&quote) {
            return Err(CsvOutputError::BadQuote(quote));
        }
        if delimiter == quote {
            return Err(CsvOutputError::DelimiterQuoteConflict);
        }
        Ok(())
    }
}

/// A typed refusal from the explicit generated CSV admission boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvOutputError {
    InvalidProgram(ProgramValidationError),
    RepairRequired,
    ConflictingFormatOptions,
    BadDelimiter(char),
    BadQuote(char),
    ConflictingQuoteSettings,
    DelimiterQuoteConflict,
    XmlBoundary,
    NamedInputs,
    NamedOutputs,
    TargetSchema {
        field: Option<String>,
        reason: &'static str,
    },
    PrimaryRows,
    DynamicDocuments,
}

impl fmt::Display for CsvOutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProgram(error) => error.fmt(f),
            Self::RepairRequired => {
                f.write_str("generated CSV output requires repair of the stored text settings")
            }
            Self::ConflictingFormatOptions => {
                f.write_str("generated CSV output cannot use conflicting format options")
            }
            Self::BadDelimiter(c) => write!(f, "invalid generated CSV delimiter {c:?}"),
            Self::BadQuote(c) => write!(f, "invalid generated CSV quote {c:?}"),
            Self::ConflictingQuoteSettings => {
                f.write_str("generated CSV quote cannot be set while quoting is disabled")
            }
            Self::DelimiterQuoteConflict => {
                f.write_str("generated CSV delimiter and quote must differ")
            }
            Self::XmlBoundary => {
                f.write_str("generated CSV output cannot be combined with an XML document adapter")
            }
            Self::NamedInputs => {
                f.write_str("generated CSV output does not yet support named inputs")
            }
            Self::NamedOutputs => {
                f.write_str("generated CSV output does not yet support named outputs")
            }
            Self::TargetSchema { field, reason } => {
                write!(f, "unsupported generated CSV target")?;
                if let Some(field) = field {
                    write!(f, " field {field:?}")?;
                }
                write!(f, ": {reason}")
            }
            Self::PrimaryRows => {
                f.write_str("generated CSV output requires a repeated primary row collection")
            }
            Self::DynamicDocuments => {
                f.write_str("generated CSV output does not yet support dynamic document iteration")
            }
        }
    }
}

impl std::error::Error for CsvOutputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidProgram(error) => Some(error),
            _ => None,
        }
    }
}

/// Prove the first generated CSV adapter's complete static output contract.
/// Existing mapping validation retains its typed failure and runs first.
pub fn validate_csv_output(
    program: &Program,
    policy: &CsvOutputPolicy,
) -> Result<(), CsvOutputError> {
    crate::validate_program(program).map_err(CsvOutputError::InvalidProgram)?;
    policy.validate_dialect()?;
    if program.xml_boundary.is_some() {
        return Err(CsvOutputError::XmlBoundary);
    }
    if !program.extra_sources.is_empty() {
        return Err(CsvOutputError::NamedInputs);
    }
    if !program.extra_targets.is_empty() {
        return Err(CsvOutputError::NamedOutputs);
    }
    let target = &program.target;
    let bad = |field, reason| CsvOutputError::TargetSchema { field, reason };
    let SchemaKind::Group {
        children,
        alternatives,
        dynamic,
        ..
    } = &target.kind
    else {
        return Err(bad(None, "a flat group schema is required"));
    };
    if target.repeating
        || target.recursive_ref.is_some()
        || !alternatives.is_empty()
        || dynamic.is_some()
    {
        return Err(bad(
            None,
            "a closed non-repeating group without alternatives is required",
        ));
    }
    let mut names = BTreeSet::new();
    for child in children {
        if child.repeating || !matches!(child.kind, SchemaKind::Scalar { .. }) {
            return Err(bad(
                Some(child.name.clone()),
                "each column must have one non-repeating scalar type",
            ));
        }
        if !names.insert(&child.name) {
            return Err(bad(Some(child.name.clone()), "column names must be unique"));
        }
    }
    if !program
        .root
        .iteration
        .as_ref()
        .is_some_and(|iteration| iteration.output() == IterationOutput::Repeated)
    {
        return Err(CsvOutputError::PrimaryRows);
    }
    if has_dynamic_documents(&program.root) {
        return Err(CsvOutputError::DynamicDocuments);
    }
    Ok(())
}

fn has_dynamic_documents(scope: &TargetScope) -> bool {
    if let Some(iteration) = &scope.iteration {
        if iteration.dynamic_document_iteration().is_some() {
            return true;
        }
        if let Some(sequence) = iteration.concatenated()
            && sequence.iter().any(has_dynamic_documents)
        {
            return true;
        }
    }
    scope.children.iter().any(has_dynamic_documents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Binding, Expression, ExpressionNode, IterationPlan, NamedSourceProgram, NamedTargetProgram,
        SourceIteration,
    };
    use ir::{ScalarType, SchemaNode, Value};

    fn program() -> Program {
        Program {
            xml_boundary: None,
            source: SchemaNode::group("Source", Vec::new()).repeating(),
            extra_sources: Vec::new(),
            target: SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)]),
            expressions: vec![ExpressionNode {
                id: 9,
                expression: Expression::Const {
                    value: Value::String("fixed".into()),
                },
            }],
            user_functions: Vec::new(),
            failure_rules: Vec::new(),
            extra_targets: Vec::new(),
            root: TargetScope {
                target_field: String::new(),
                repeating: false,
                iteration: Some(IterationPlan::source(Vec::new())),
                construction: Default::default(),
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    expression: 9,
                    target_domain: ScalarType::String.into(),
                    repeating: false,
                }],
                children: Vec::new(),
            },
        }
    }

    #[test]
    fn explicit_native_policy_retains_headers_bom_and_custom_dialect() {
        let options = FormatOptions {
            tabular_kind: Some(TabularBoundaryKind::Csv),
            delimiter: Some(';'),
            csv_quote: Some('\''),
            csv_utf8_bom: true,
            has_header_row: Some(false),
            csv_preserve_empty_strings: true,
            ..Default::default()
        };
        let policy = CsvOutputPolicy::from_format_options(&options).unwrap();
        assert_eq!(
            policy,
            CsvOutputPolicy {
                delimiter: Some(';'),
                quote: Some('\''),
                quote_disabled: false,
                has_headers: false,
                utf8_bom: true
            }
        );
        validate_csv_output(&program(), &policy).unwrap();
    }

    #[test]
    fn conflicting_physical_formats_are_never_silently_discarded() {
        for options in [
            FormatOptions {
                xml_document: true,
                ..Default::default()
            },
            FormatOptions {
                json_document: true,
                ..Default::default()
            },
            FormatOptions {
                json_lines: true,
                ..Default::default()
            },
            FormatOptions {
                json5: true,
                ..Default::default()
            },
            FormatOptions {
                xlsx_sheet: Some("Sheet1".into()),
                ..Default::default()
            },
            FormatOptions {
                xlsx_update_existing: true,
                ..Default::default()
            },
            FormatOptions {
                lenient_segments: true,
                ..Default::default()
            },
        ] {
            assert!(matches!(
                CsvOutputPolicy::from_format_options(&options),
                Err(CsvOutputError::ConflictingFormatOptions)
            ));
        }
        let options = FormatOptions {
            csv_text_repair_dependency: Some(mapping::CsvTextRepairDependency::new(
                mapping::CsvTextRepairCause::Encoding,
            )),
            ..Default::default()
        };
        assert!(matches!(
            CsvOutputPolicy::from_format_options(&options),
            Err(CsvOutputError::RepairRequired)
        ));
    }

    #[test]
    fn dialect_refusals_keep_the_exact_offending_character() {
        for delimiter in ['\0', '\r', '\n', 'é'] {
            assert!(
                matches!(validate_csv_output(&program(), &CsvOutputPolicy { delimiter: Some(delimiter), ..Default::default() }), Err(CsvOutputError::BadDelimiter(c)) if c == delimiter)
            );
        }
        for quote in ['\t', ' ', 'é'] {
            assert!(
                matches!(validate_csv_output(&program(), &CsvOutputPolicy { quote: Some(quote), ..Default::default() }), Err(CsvOutputError::BadQuote(c)) if c == quote)
            );
        }
        assert!(matches!(
            validate_csv_output(
                &program(),
                &CsvOutputPolicy {
                    delimiter: Some('|'),
                    quote: Some('|'),
                    ..Default::default()
                }
            ),
            Err(CsvOutputError::DelimiterQuoteConflict)
        ));
        assert!(matches!(
            validate_csv_output(
                &program(),
                &CsvOutputPolicy {
                    quote_disabled: true,
                    quote: Some('"'),
                    ..Default::default()
                }
            ),
            Err(CsvOutputError::ConflictingQuoteSettings)
        ));
    }

    #[test]
    fn singleton_rows_are_refused_and_invalid_mapped_root_keeps_its_cause() {
        let mut candidate = program();
        candidate.root.iteration = None;
        assert!(matches!(
            validate_csv_output(&candidate, &CsvOutputPolicy::default()),
            Err(CsvOutputError::PrimaryRows)
        ));
        candidate.root.iteration = Some(IterationPlan::new(
            SourceIteration::new(Vec::new()),
            None,
            None,
            Vec::new(),
            IterationOutput::First,
        ));
        assert!(matches!(
            validate_csv_output(&candidate, &CsvOutputPolicy::default()),
            Err(CsvOutputError::PrimaryRows)
        ));
        candidate.root.iteration = Some(IterationPlan::new(
            SourceIteration::new(Vec::new()),
            None,
            None,
            Vec::new(),
            IterationOutput::MappedSequence,
        ));
        assert!(matches!(
            validate_csv_output(&candidate, &CsvOutputPolicy::default()),
            Err(CsvOutputError::InvalidProgram(
                ProgramValidationError::InvalidIterationOutput {
                    target_path,
                    output: IterationOutput::MappedSequence,
                }
            )) if target_path.is_empty()
        ));
    }

    #[test]
    fn named_inputs_and_outputs_keep_their_separate_refusals() {
        let mut candidate = program();
        candidate.extra_sources.push(NamedSourceProgram {
            name: "Other".into(),
            source: SchemaNode::group("Other", Vec::new()),
            dynamic: None,
        });
        assert!(matches!(
            validate_csv_output(&candidate, &CsvOutputPolicy::default()),
            Err(CsvOutputError::NamedInputs)
        ));
        candidate.extra_sources.clear();
        candidate.extra_targets.push(NamedTargetProgram {
            name: "Other".into(),
            target: candidate.target.clone(),
            root: candidate.root.clone(),
        });
        assert!(matches!(
            validate_csv_output(&candidate, &CsvOutputPolicy::default()),
            Err(CsvOutputError::NamedOutputs)
        ));
    }

    #[test]
    fn invalid_mapping_cause_precedes_policy_admission() {
        use std::error::Error as _;
        let mut candidate = program();
        candidate.root.bindings[0].expression = 999;
        let error = validate_csv_output(
            &candidate,
            &CsvOutputPolicy {
                delimiter: Some('\n'),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(matches!(&error, CsvOutputError::InvalidProgram(_)));
        assert!(
            error
                .source()
                .unwrap()
                .downcast_ref::<ProgramValidationError>()
                .is_some()
        );
    }
}
