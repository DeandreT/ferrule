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
    Csv,
    Xml,
}

impl PrimaryDocumentFormat {
    pub fn name(self) -> &'static str {
        match self {
            Self::Csv => "CSV",
            Self::Xml => "XML",
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
    /// Neither a format option nor a recognized saved path names the format.
    MissingIdentity { side: PrimaryRouteSide },
    /// The format has no primary document route.
    NotAdmitted {
        side: PrimaryRouteSide,
        format: &'static str,
    },
    /// The target does not use the source's format.
    TargetFormat { source: PrimaryDocumentFormat },
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
    if options.idoc.is_some() || options.swift_mt.is_some() || options.edi_kind.is_some() {
        return refuse("EDI");
    }
    if options.xml_document {
        return Ok(PrimaryDocumentFormat::Xml);
    }
    if options.json_document || options.json5 || options.json_lines {
        return refuse("JSON");
    }
    if options.fixed_width.is_some() {
        return refuse("fixed-width text");
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
        Some("json" | "json5" | "jsonl" | "ndjson") => refuse("JSON"),
        Some("edi" | "x12" | "edifact" | "hl7" | "idoc" | "fin" | "swift") => refuse("EDI"),
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
                _ => PrimaryRouteError::TargetFormat { source: format },
            });
        }
        let wildcard = matches!(
            self.primary_source().selection,
            PrimaryFileSelection::FileNameWildcard { .. }
        );
        let shape = match format {
            PrimaryDocumentFormat::Csv => PrimaryDocumentShape::Rows,
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
