//! Opt-in, finite flat-workbook budgets. See the crate README for the accepted
//! package profile and the distinction between data budgets and process RAM.

use ir::{Instance, SchemaKind, SchemaNode};

use crate::{XlsxFormatError, column_indexes, row_fields, rows_from_range};

mod package;
#[cfg(test)]
mod tests;
mod write;
mod xml;

pub use write::{to_bytes_bounded, to_bytes_with_options_bounded};

/// The resource whose configured budget was exceeded (or configured invalidly).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XlsxLimit {
    InputBytes,
    Entries,
    EntryBytes,
    ExpandedBytes,
    XmlDepth,
    XmlNodes,
    XmlTextBytes,
    SharedStrings,
    StringBytes,
    Rows,
    Columns,
    Cells,
    DenseCells,
    MaterializedCells,
    MaterializedTextBytes,
    OutputBytes,
}

/// Limits for the opt-in flat XLSX APIs. Byte limits count UTF-8 bytes, not
/// characters. Row/column limits also cap physical one-based coordinates.
/// All values must be positive and at most `isize::MAX`; coordinates cannot
/// exceed Excel's limits, and XML depth cannot exceed 256.
#[derive(Clone, Debug)]
pub struct XlsxLimitOptions {
    pub max_input_bytes: usize,
    pub max_entries: usize,
    pub max_entry_bytes: usize,
    pub max_expanded_bytes: usize,
    pub max_xml_depth: usize,
    pub max_xml_nodes: usize,
    pub max_xml_text_bytes: usize,
    pub max_shared_strings: usize,
    pub max_string_bytes: usize,
    pub max_rows: usize,
    pub max_columns: usize,
    pub max_cells: usize,
    pub max_dense_cells: usize,
    pub max_materialized_cells: usize,
    pub max_materialized_text_bytes: usize,
    pub max_output_bytes: usize,
}

impl Default for XlsxLimitOptions {
    fn default() -> Self {
        Self {
            max_input_bytes: 16 * 1024 * 1024,
            max_entries: 256,
            max_entry_bytes: 32 * 1024 * 1024,
            max_expanded_bytes: 64 * 1024 * 1024,
            max_xml_depth: 64,
            max_xml_nodes: 2_000_000,
            max_xml_text_bytes: 32 * 1024 * 1024,
            max_shared_strings: 100_000,
            max_string_bytes: 128 * 1024,
            max_rows: 100_000,
            max_columns: 1024,
            max_cells: 1_000_000,
            max_dense_cells: 1_000_000,
            max_materialized_cells: 1_000_000,
            max_materialized_text_bytes: 64 * 1024 * 1024,
            max_output_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Validated, immutable limits. Changing the options requires validation again.
#[derive(Clone, Debug, Default)]
pub struct XlsxLimits(XlsxLimitOptions);

impl XlsxLimits {
    pub fn new(options: XlsxLimitOptions) -> Result<Self, XlsxFormatError> {
        use XlsxLimit::*;
        for (limit, value) in [
            (InputBytes, options.max_input_bytes),
            (Entries, options.max_entries),
            (EntryBytes, options.max_entry_bytes),
            (ExpandedBytes, options.max_expanded_bytes),
            (XmlDepth, options.max_xml_depth),
            (XmlNodes, options.max_xml_nodes),
            (XmlTextBytes, options.max_xml_text_bytes),
            (SharedStrings, options.max_shared_strings),
            (StringBytes, options.max_string_bytes),
            (Rows, options.max_rows),
            (Columns, options.max_columns),
            (Cells, options.max_cells),
            (DenseCells, options.max_dense_cells),
            (MaterializedCells, options.max_materialized_cells),
            (MaterializedTextBytes, options.max_materialized_text_bytes),
            (OutputBytes, options.max_output_bytes),
        ] {
            if value == 0 || value > isize::MAX as usize {
                return Err(XlsxFormatError::InvalidLimit(limit));
            }
        }
        if options.max_rows > crate::MAX_WORKSHEET_ROW as usize {
            return Err(XlsxFormatError::InvalidLimit(Rows));
        }
        if options.max_columns > crate::MAX_WORKSHEET_COLUMN as usize {
            return Err(XlsxFormatError::InvalidLimit(Columns));
        }
        if options.max_xml_depth > 256 {
            return Err(XlsxFormatError::InvalidLimit(XmlDepth));
        }
        Ok(Self(options))
    }

    pub fn options(&self) -> &XlsxLimitOptions {
        &self.0
    }
}

pub(super) fn check(
    limit: XlsxLimit,
    actual: usize,
    maximum: usize,
) -> Result<(), XlsxFormatError> {
    if actual > maximum {
        Err(XlsxFormatError::LimitExceeded {
            limit,
            maximum,
            actual,
        })
    } else {
        Ok(())
    }
}

pub(super) fn add(
    total: &mut usize,
    amount: usize,
    limit: XlsxLimit,
    maximum: usize,
) -> Result<(), XlsxFormatError> {
    *total = total
        .checked_add(amount)
        .ok_or(XlsxFormatError::LimitExceeded {
            limit,
            maximum,
            actual: usize::MAX,
        })?;
    check(limit, *total, maximum)
}

pub(super) fn invalid(message: impl Into<String>) -> XlsxFormatError {
    XlsxFormatError::InvalidBoundedPackage(message.into())
}

pub(super) fn unsupported(message: &'static str) -> XlsxFormatError {
    XlsxFormatError::UnsupportedBoundedProfile(message)
}

pub(super) fn fields<'a>(
    schema: &'a SchemaNode,
    limits: &XlsxLimitOptions,
) -> Result<Vec<(&'a str, ir::ScalarType)>, XlsxFormatError> {
    // Bound schema projection before row_fields allocates its output vector.
    if let SchemaKind::Group { children, .. } = &schema.kind {
        check(XlsxLimit::Columns, children.len(), limits.max_columns)?;
        for field in children {
            check(
                XlsxLimit::StringBytes,
                field.name.len(),
                limits.max_string_bytes,
            )?;
        }
    }
    row_fields(schema)
}

/// Reads one flat table only after validating actual ZIP expansion, XML, shared
/// strings, every worksheet's cells and dense extent, and projected output size.
/// No filesystem, formula evaluation, or external-resource access is performed.
/// Existing [`crate::from_bytes`] behavior is unchanged.
pub fn from_bytes_bounded(
    bytes: &[u8],
    schema: &SchemaNode,
    sheet: Option<&str>,
    start_row: u32,
    columns: &[u32],
    has_header: bool,
    limits: &XlsxLimits,
) -> Result<Vec<Instance>, XlsxFormatError> {
    let limits = limits.options();
    check(XlsxLimit::InputBytes, bytes.len(), limits.max_input_bytes)?;
    if start_row == 0 || start_row > crate::MAX_WORKSHEET_ROW {
        return Err(XlsxFormatError::InvalidCoordinate);
    }
    check(XlsxLimit::Rows, start_row as usize, limits.max_rows)?;
    let fields = fields(schema, limits)?;
    let columns = column_indexes(fields.len(), columns)?;
    for column in &columns {
        check(XlsxLimit::Columns, *column as usize + 1, limits.max_columns)?;
    }
    let summary = package::validate(bytes, limits)?;
    let selected = match sheet {
        Some(name) => summary
            .sheets
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| XlsxFormatError::MissingWorksheet(name.to_string()))?,
        None => summary
            .sheets
            .first()
            .ok_or(XlsxFormatError::NoWorksheets)?,
    };
    let row_count = selected
        .last_row
        .saturating_sub(start_row as usize - 1 + usize::from(has_header));
    for (row, column, value) in &selected.integer_risks {
        if *row < start_row as usize + usize::from(has_header) {
            continue;
        }
        if let Some((name, _)) = fields.iter().zip(&columns).find_map(|(field, col)| {
            (field.1 == ir::ScalarType::Int && *col as usize + 1 == *column).then_some(field)
        }) {
            return Err(XlsxFormatError::Parse {
                row: *row as u32,
                field: (*name).to_string(),
                expected: ir::ScalarType::Int,
                value: value.clone(),
            });
        }
    }
    let projected = row_count.saturating_mul(fields.len());
    check(
        XlsxLimit::MaterializedCells,
        projected,
        limits.max_materialized_cells,
    )?;
    // Range<Data> and returned values can both own cell text. Field names and
    // stringification of numeric/date/bool cells are additional output storage.
    let field_bytes = fields
        .iter()
        .fold(0usize, |sum, (name, _)| sum.saturating_add(name.len()));
    let mut text = summary.cell_text_bytes.saturating_mul(2);
    add(
        &mut text,
        row_count.saturating_mul(field_bytes),
        XlsxLimit::MaterializedTextBytes,
        limits.max_materialized_text_bytes,
    )?;
    add(
        &mut text,
        projected.saturating_mul(384),
        XlsxLimit::MaterializedTextBytes,
        limits.max_materialized_text_bytes,
    )?;
    let range = crate::worksheet_range(bytes, Some(&selected.name))?;
    rows_from_range(&range, &fields, start_row, &columns, has_header)
}
