//! Contracts the four browser selectors cannot execute, independently of the
//! selected document syntax. Identity and interchange provenance are not codecs.

use std::fmt;

use mapping::FormatOptions;

use super::{DataSide, RuntimeError};

/// A declared boundary contract unavailable in the in-memory browser runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsupportedContractFamily {
    FixedWidth,
    FlexText,
    Pdf,
    Protobuf,
    Idoc,
    SwiftMt,
    Xlsx,
    Edi,
    SqliteRead,
    LocalXmlFileSet,
    EdiConfigReference,
}

impl fmt::Display for UnsupportedContractFamily {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::FixedWidth => "fixed-width layout",
            Self::FlexText => "FlexText layout",
            Self::Pdf => "PDF extraction layout",
            Self::Protobuf => "Protocol Buffers contract",
            Self::Idoc => "IDoc layout",
            Self::SwiftMt => "SWIFT MT contract",
            Self::Xlsx => "XLSX worksheet contract",
            Self::Edi => "EDI runtime policy",
            Self::SqliteRead => "declared SQLite read",
            Self::LocalXmlFileSet => "local XML file set",
            Self::EdiConfigReference => "unresolved EDI configuration",
        })
    }
}

// Stable diagnostic precedence within one side. Check declarations, not whole
// FormatOptions equality: manual selectors may override identity-only flags and
// paths, and host-supplied text does not require transport acquisition.
fn unsupported_family(options: &FormatOptions) -> Option<UnsupportedContractFamily> {
    use UnsupportedContractFamily as Family;
    if options.fixed_width.is_some() {
        Some(Family::FixedWidth)
    } else if options.flextext.is_some() {
        Some(Family::FlexText)
    } else if options.pdf.is_some() {
        Some(Family::Pdf)
    } else if options.protobuf.is_some() {
        Some(Family::Protobuf)
    } else if options.idoc.is_some() {
        Some(Family::Idoc)
    } else if options.swift_mt.is_some() || options.swift_mt_output.is_some() {
        Some(Family::SwiftMt)
    } else if options.xlsx_sheet.is_some()
        || options.xlsx_start_row.is_some()
        || !options.xlsx_columns.is_empty()
        || !options.xlsx_headers.is_empty()
        || !options.xlsx_rows.is_empty()
        || options.xlsx_composite.is_some()
        || options.xlsx_worksheet_set.is_some()
        || options.xlsx_grid.is_some()
        || options.xlsx_hierarchical.is_some()
        || options.xlsx_update_existing
    {
        Some(Family::Xlsx)
    } else if options.lenient_segments
        || !options.edi_implied_decimals.is_empty()
        || !options.edi_lexical_formats.is_empty()
        || !options.edi_value_constraints.is_empty()
        || options.edi_autocomplete.is_some()
        || options.x12_separators.is_some()
        || options.x12_interchange_version.is_some()
    {
        Some(Family::Edi)
    } else if options.sqlite_read.is_some() {
        Some(Family::SqliteRead)
    } else if options.local_xml_file_set {
        Some(Family::LocalXmlFileSet)
    } else if options.edi_config_reference.is_some() {
        Some(Family::EdiConfigReference)
    } else {
        None
    }
}

pub(super) fn admit_boundary(options: &FormatOptions, side: DataSide) -> Result<(), RuntimeError> {
    // Preserve the actionable dependency diagnostics rather than hiding repair
    // state behind a generic unsupported-family error.
    if let Some(dependency) = options.json5_interchange_blocker() {
        return Err(RuntimeError::Json5InterchangeDependency {
            side,
            dependency: dependency.clone(),
        });
    }
    if let Some(dependency) = options.csv_text_repair_dependency {
        return Err(RuntimeError::CsvRepairDependency { side, dependency });
    }
    match unsupported_family(options) {
        Some(UnsupportedContractFamily::FixedWidth) => {
            Err(RuntimeError::FixedWidthNotExecutable { side })
        }
        Some(family) => Err(RuntimeError::UnsupportedBoundaryContract { side, family }),
        None => Ok(()),
    }
}
