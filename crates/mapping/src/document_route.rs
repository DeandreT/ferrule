//! How a primary document plan's documents are read and combined.
//!
//! A route is decided from the saved format identities alone, before any
//! filename is evaluated or any host is called. The source and target must
//! name the same format. Row formats combine every selected document's rows
//! into one row sequence; document formats map one document directly, or a
//! document set of every selected document for a filename wildcard.

use std::fmt;
use std::path::Path;

use crate::{DocumentProject, FormatOptions, PrimaryFileSelection, TabularBoundaryKind};

/// The format of a primary document plan's documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimaryDocumentFormat {
    Edi(crate::EdiBoundaryKind),
    Csv,
    Xml,
    Json,
    FixedWidth,
}

impl PrimaryDocumentFormat {
    pub fn name(self) -> &'static str {
        match self {
            Self::Edi(crate::EdiBoundaryKind::X12) => "X12",
            Self::Edi(crate::EdiBoundaryKind::Edifact) => "EDIFACT",
            Self::Edi(crate::EdiBoundaryKind::Hl7) => "HL7",
            Self::Edi(crate::EdiBoundaryKind::Tradacoms) => "TRADACOMS",
            Self::Edi(crate::EdiBoundaryKind::Idoc) => "IDoc",
            Self::Edi(crate::EdiBoundaryKind::SwiftMt) => "SWIFT MT",
            Self::Csv => "CSV",
            Self::Xml => "XML",
            Self::Json => "JSON",
            Self::FixedWidth => "fixed-width",
        }
    }
}

/// How the selected documents form the one mapping source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimaryDocumentShape {
    /// Every document's rows, in selection then row order.
    Rows,
    /// The one directly named document.
    Document,
    /// A document set of every selected document, in selection order.
    DocumentSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrimaryDocumentRoute {
    pub format: PrimaryDocumentFormat,
    pub shape: PrimaryDocumentShape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimaryRouteSide {
    Source,
    Target,
}

impl fmt::Display for PrimaryRouteSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Source => "source",
            Self::Target => "target",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimaryRouteError {
    /// A single-output document route cannot acquire or publish named endpoints.
    NamedEndpoints,
    FixedWidthSchema {
        side: PrimaryRouteSide,
    },
    FixedWidthFields {
        side: PrimaryRouteSide,
        expected: usize,
        got: usize,
    },
    /// The selected format does not own all of the boundary's options.
    IncompatibleOptions {
        side: PrimaryRouteSide,
        format: PrimaryDocumentFormat,
    },
    /// Neither a format option nor a recognized saved path names the format.
    MissingIdentity {
        side: PrimaryRouteSide,
    },
    /// The format has no primary document route.
    NotAdmitted {
        side: PrimaryRouteSide,
        format: &'static str,
    },
    /// The target does not use the source's format.
    TargetFormat {
        source: PrimaryDocumentFormat,
    },
    /// A directly named document cannot supply a source declared as an XML
    /// document set (`local_xml_file_set`).
    DirectDocumentSet,
    /// An observed XML root view describes one document; a filename wildcard
    /// selects a document set.
    RootViewDocumentSet,
}

impl fmt::Display for PrimaryRouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FixedWidthSchema { side } => write!(
                f,
                "fixed-width {side} requires a flat non-repeating scalar row schema"
            ),
            Self::FixedWidthFields {
                side,
                expected,
                got,
            } => write!(
                f,
                "fixed-width {side} needs {expected} field widths, got {got}"
            ),
            Self::NamedEndpoints => {
                f.write_str("primary document routes have no named sources or targets")
            }
            Self::IncompatibleOptions { side, format } => write!(
                f,
                "primary {} document {side} has incompatible format options",
                format.name()
            ),
            Self::MissingIdentity { side } => write!(
                f,
                "the primary document {side} has no explicit format identity"
            ),
            Self::NotAdmitted { side, format } => {
                write!(f, "a primary document {side} cannot use {format}")
            }
            Self::TargetFormat { source } => write!(
                f,
                "a primary {} document plan needs a {} target",
                source.name(),
                source.name()
            ),
            Self::DirectDocumentSet => f.write_str(
                "a directly named primary document cannot supply an XML document-set source",
            ),
            Self::RootViewDocumentSet => f.write_str(
                "an observed XML root view cannot read a filename wildcard's document set",
            ),
        }
    }
}

impl std::error::Error for PrimaryRouteError {}

/// The format named by explicit options, else by a recognized saved path.
fn classify(
    options: &FormatOptions,
    path: Option<&str>,
    side: PrimaryRouteSide,
) -> Result<PrimaryDocumentFormat, PrimaryRouteError> {
    let refuse = |format| Err(PrimaryRouteError::NotAdmitted { side, format });
    if options.json5_interchange_dependency.is_some() {
        return refuse("native JSON5 interchange");
    }
    if options.sqlite_read.is_some() {
        return refuse("a SQLite query");
    }
    if options.http_get.is_some() {
        return refuse("an HTTP request");
    }
    if options.external_source.is_some() {
        return refuse("a captured external source");
    }
    if options.xbrl.is_some() {
        return refuse("XBRL");
    }
    if options.pdf.is_some() {
        return refuse("PDF");
    }
    if options.flextext.is_some() {
        return refuse("FlexText");
    }
    if options.protobuf.is_some() {
        return refuse("Protocol Buffers");
    }
    if options.wsdl.is_some() {
        return refuse("a WSDL message");
    }
    if options.swift_mt.is_some() || options.edi_kind == Some(crate::EdiBoundaryKind::SwiftMt) {
        return refuse("input-only SWIFT MT");
    }
    if let Some(kind) = options.edi_kind {
        return Ok(PrimaryDocumentFormat::Edi(kind));
    }
    if options.idoc.is_some() {
        return Ok(PrimaryDocumentFormat::Edi(crate::EdiBoundaryKind::Idoc));
    }
    if options.xml_document {
        return Ok(PrimaryDocumentFormat::Xml);
    }
    if options.json5 {
        return refuse("JSON5");
    }
    if options.json_lines {
        return refuse("JSON Lines");
    }
    if options.json_document {
        return Ok(PrimaryDocumentFormat::Json);
    }
    if options.fixed_width.is_some() {
        return Ok(PrimaryDocumentFormat::FixedWidth);
    }
    match options.tabular_kind {
        Some(TabularBoundaryKind::Csv) => return Ok(PrimaryDocumentFormat::Csv),
        Some(TabularBoundaryKind::Xlsx) => return refuse("XLSX"),
        None => {}
    }
    let extension = path
        .and_then(|path| Path::new(path).extension())
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("csv") => Ok(PrimaryDocumentFormat::Csv),
        Some("xml") => Ok(PrimaryDocumentFormat::Xml),
        Some("json") => Ok(PrimaryDocumentFormat::Json),
        Some("json5") => refuse("JSON5"),
        Some("jsonl" | "ndjson") => refuse("JSON Lines"),
        Some("x12") => Ok(PrimaryDocumentFormat::Edi(crate::EdiBoundaryKind::X12)),
        Some("edifact") => Ok(PrimaryDocumentFormat::Edi(crate::EdiBoundaryKind::Edifact)),
        Some("hl7") => Ok(PrimaryDocumentFormat::Edi(crate::EdiBoundaryKind::Hl7)),
        Some("tradacoms") => Ok(PrimaryDocumentFormat::Edi(
            crate::EdiBoundaryKind::Tradacoms,
        )),
        Some("idoc") => Ok(PrimaryDocumentFormat::Edi(crate::EdiBoundaryKind::Idoc)),
        Some("edi") => refuse("ambiguous EDI without an explicit dialect"),
        Some("fin" | "swift") => refuse("input-only SWIFT MT"),
        Some("xlsx") => refuse("XLSX"),
        Some("db" | "sqlite" | "sqlite3") => refuse("SQLite"),
        Some("pdf") => refuse("PDF"),
        _ => Err(PrimaryRouteError::MissingIdentity { side }),
    }
}

impl DocumentProject {
    /// Decides the route from saved identities without evaluating the
    /// filename, reading a preview or calling a host.
    pub fn primary_route(&self) -> Result<PrimaryDocumentRoute, PrimaryRouteError> {
        let project = self.project();
        if !project.extra_sources.is_empty() || !project.extra_targets.is_empty() {
            return Err(PrimaryRouteError::NamedEndpoints);
        }
        let format = classify(
            &project.source_options,
            project.source_path.as_deref(),
            PrimaryRouteSide::Source,
        )?;
        let target = classify(
            &project.target_options,
            project.target_path.as_deref(),
            PrimaryRouteSide::Target,
        );
        if target.as_ref().ok() != Some(&format) {
            return Err(match target {
                Err(error @ PrimaryRouteError::NotAdmitted { .. }) => error,
                // Extending JSON-to-JSON plans must not change existing mixed
                // CSV/XML plan refusals into a different error contract.
                Ok(PrimaryDocumentFormat::Json) => PrimaryRouteError::NotAdmitted {
                    side: PrimaryRouteSide::Target,
                    format: "JSON",
                },
                Ok(PrimaryDocumentFormat::FixedWidth) => PrimaryRouteError::NotAdmitted {
                    side: PrimaryRouteSide::Target,
                    format: "fixed-width text",
                },
                _ => PrimaryRouteError::TargetFormat { source: format },
            });
        }
        validate_options(&project.source_options, format, PrimaryRouteSide::Source)?;
        validate_options(&project.target_options, format, PrimaryRouteSide::Target)?;
        let wildcard = matches!(
            self.primary_source().selection,
            PrimaryFileSelection::FileNameWildcard { .. }
        );
        let shape = match format {
            PrimaryDocumentFormat::Edi(_) => {
                if project.root.output_path().is_some() {
                    return Err(PrimaryRouteError::NotAdmitted {
                        side: PrimaryRouteSide::Target,
                        format: "dynamic EDI output documents",
                    });
                }
                if wildcard {
                    PrimaryDocumentShape::DocumentSet
                } else {
                    PrimaryDocumentShape::Document
                }
            }
            PrimaryDocumentFormat::FixedWidth => {
                for (side, schema, options) in [
                    (
                        PrimaryRouteSide::Source,
                        &project.source,
                        &project.source_options,
                    ),
                    (
                        PrimaryRouteSide::Target,
                        &project.target,
                        &project.target_options,
                    ),
                ] {
                    let ir::SchemaKind::Group { children, .. } = &schema.kind else {
                        return Err(PrimaryRouteError::FixedWidthSchema { side });
                    };
                    if schema.repeating
                        || schema.contains_json_array_wrapper()
                        || children.iter().any(|child| {
                            child.repeating || !matches!(child.kind, ir::SchemaKind::Scalar { .. })
                        })
                    {
                        return Err(PrimaryRouteError::FixedWidthSchema { side });
                    }
                    let got = options
                        .fixed_width
                        .as_ref()
                        .map_or(0, |layout| layout.field_widths().len());
                    if got != children.len() {
                        return Err(PrimaryRouteError::FixedWidthFields {
                            side,
                            expected: children.len(),
                            got,
                        });
                    }
                }
                if project.root.output_path().is_some() {
                    return Err(PrimaryRouteError::NotAdmitted {
                        side: PrimaryRouteSide::Target,
                        format: "dynamic fixed-width output documents",
                    });
                }
                PrimaryDocumentShape::Rows
            }
            PrimaryDocumentFormat::Csv => PrimaryDocumentShape::Rows,
            PrimaryDocumentFormat::Json => {
                if project.root.output_path().is_some() {
                    return Err(PrimaryRouteError::NotAdmitted {
                        side: PrimaryRouteSide::Target,
                        format: "dynamic JSON output documents",
                    });
                }
                if wildcard {
                    PrimaryDocumentShape::DocumentSet
                } else {
                    PrimaryDocumentShape::Document
                }
            }
            PrimaryDocumentFormat::Xml => {
                let options = &project.source_options;
                if !wildcard && options.local_xml_file_set {
                    return Err(PrimaryRouteError::DirectDocumentSet);
                }
                if wildcard
                    && (options.xml_root_view_read_policy
                        || options.xml_allow_inactive_root_type_members)
                {
                    return Err(PrimaryRouteError::RootViewDocumentSet);
                }
                if wildcard {
                    PrimaryDocumentShape::DocumentSet
                } else {
                    PrimaryDocumentShape::Document
                }
            }
        };
        Ok(PrimaryDocumentRoute { format, shape })
    }
}

/// Keep unrelated adapters out of a route even when selection returns no files.
/// Provenance does not select an adapter; every executable option must belong
/// to the selected format and side.
fn validate_options(
    options: &FormatOptions,
    format: PrimaryDocumentFormat,
    side: PrimaryRouteSide,
) -> Result<(), PrimaryRouteError> {
    let mut accepted = FormatOptions {
        mfd_decimal_input_names: options.mfd_decimal_input_names.clone(),
        mfd_optional_parser_input_names: options.mfd_optional_parser_input_names.clone(),
        ..Default::default()
    };
    match format {
        PrimaryDocumentFormat::Edi(kind) => {
            if options.edi_config_reference.is_some()
                || (kind == crate::EdiBoundaryKind::Idoc && options.idoc.is_none())
            {
                return Err(PrimaryRouteError::NotAdmitted {
                    side,
                    format: "EDI without a resolved runtime layout",
                });
            }
            accepted.edi_kind = options.edi_kind;
            accepted.lenient_segments = options.lenient_segments;
            accepted.edi_value_constraints = options.edi_value_constraints.clone();
            accepted.edi_implied_decimals = options.edi_implied_decimals.clone();
            accepted.edi_lexical_formats = options.edi_lexical_formats.clone();
            if kind == crate::EdiBoundaryKind::X12 {
                accepted.x12_separators = options.x12_separators;
                accepted.x12_interchange_version = options.x12_interchange_version.clone();
            }
            if kind == crate::EdiBoundaryKind::Idoc {
                accepted.idoc = options.idoc.clone();
                accepted.idoc_native_config = options.idoc_native_config.clone();
                accepted.idoc_native_text_settings = options.idoc_native_text_settings.clone();
            }
            if options.edi_autocomplete.as_ref().is_some_and(|completion| {
                matches!(
                    (kind, completion),
                    (crate::EdiBoundaryKind::X12, crate::EdiAutocomplete::X12(_))
                        | (
                            crate::EdiBoundaryKind::Edifact,
                            crate::EdiAutocomplete::Edifact(_)
                        )
                        | (crate::EdiBoundaryKind::Idoc, crate::EdiAutocomplete::Idoc)
                        | (crate::EdiBoundaryKind::Hl7, crate::EdiAutocomplete::Hl7)
                        | (
                            crate::EdiBoundaryKind::Tradacoms,
                            crate::EdiAutocomplete::Tradacoms
                        )
                )
            }) {
                accepted.edi_autocomplete = options.edi_autocomplete.clone();
            }
        }
        PrimaryDocumentFormat::FixedWidth => {
            accepted.fixed_width = options.fixed_width.clone();
        }
        PrimaryDocumentFormat::Json => {
            accepted.json_document = options.json_document;
        }
        PrimaryDocumentFormat::Csv => {
            accepted.tabular_kind = options.tabular_kind;
            accepted.delimiter = options.delimiter;
            accepted.csv_quote = options.csv_quote;
            accepted.csv_quote_disabled = options.csv_quote_disabled;
            accepted.csv_utf8_bom = options.csv_utf8_bom;
            accepted.csv_preserve_empty_strings = options.csv_preserve_empty_strings;
            accepted.has_header_row = options.has_header_row;
        }
        PrimaryDocumentFormat::Xml => {
            accepted.xml_document = options.xml_document;
            if side == PrimaryRouteSide::Source {
                accepted.local_xml_file_set = options.local_xml_file_set;
                accepted.xml_allow_inactive_root_type_members =
                    options.xml_allow_inactive_root_type_members;
                accepted.xml_root_view_read_policy = options.xml_root_view_read_policy;
            } else {
                accepted.xml_schema_hints = options.xml_schema_hints.clone();
            }
        }
    }
    if *options != accepted {
        return Err(PrimaryRouteError::IncompatibleOptions { side, format });
    }
    Ok(())
}
