use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::{Cursor, ErrorKind, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::XlsxHierarchicalLayout;
use thiserror::Error;
use umya_spreadsheet::structs::{Style, Worksheet};

use super::{
    FlatTableWriteOptions, MAX_WORKSHEET_COLUMN, MAX_WORKSHEET_ROW, XlsxFormatError, XlsxLimit,
    XlsxLimitOptions, XlsxLimits, boolean_lexical, column_indexes, exact_f64, lexical_f64,
    lexical_i64, row_fields, validate_row,
};
use crate::bounded::{add, check};
use crate::hierarchical::{
    CellContent, datetime_format_code, range_end, range_start, repeated_at, validate_layout,
    visit_worksheet, worksheet_name,
};

mod package;

struct ReplacementOptions<'a> {
    header_row: u32,
    headers: &'a [String],
    has_header: bool,
}

/// Replaces one selected table while preserving the rest of an existing
/// workbook. The workbook is fully serialized before the original file is
/// replaced, so validation or serialization failures leave it untouched.
pub fn update(
    path: &Path,
    schema: &SchemaNode,
    rows: &[Instance],
    sheet: Option<&str>,
    start_row: u32,
    columns: &[u32],
    has_header: bool,
) -> Result<(), XlsxFormatError> {
    update_with_options(
        path,
        schema,
        rows,
        FlatTableWriteOptions {
            sheet,
            start_row,
            columns,
            headers: &[],
            has_header,
        },
    )
}

pub fn update_with_options(
    path: &Path,
    schema: &SchemaNode,
    rows: &[Instance],
    options: FlatTableWriteOptions<'_>,
) -> Result<(), XlsxFormatError> {
    let FlatTableWriteOptions {
        sheet,
        start_row,
        columns,
        headers,
        has_header,
    } = options;
    if start_row == 0 || start_row > MAX_WORKSHEET_ROW {
        return Err(XlsxFormatError::InvalidCoordinate);
    }
    let fields = row_fields(schema)?;
    if !headers.is_empty() && headers.len() != fields.len() {
        return Err(XlsxFormatError::HeaderCount {
            expected: fields.len(),
            got: headers.len(),
        });
    }
    let columns = column_indexes(fields.len(), columns)?;
    let records = rows
        .iter()
        .enumerate()
        .map(|(row, instance)| validate_row(row, instance, &fields))
        .collect::<Result<Vec<_>, _>>()?;
    let data_start = start_row
        .checked_add(u32::from(has_header))
        .ok_or(XlsxFormatError::InvalidCoordinate)?;
    let last_data_row = data_start
        .checked_add(u32::try_from(records.len()).map_err(|_| XlsxFormatError::InvalidCoordinate)?)
        .and_then(|row| row.checked_sub(1))
        .unwrap_or(data_start);
    if !records.is_empty() && last_data_row > MAX_WORKSHEET_ROW {
        return Err(XlsxFormatError::InvalidCoordinate);
    }

    let mut workbook = umya_spreadsheet::reader::xlsx::read(path)
        .map_err(|error| XlsxFormatError::Update(error.to_string()))?;
    let worksheet = match sheet {
        Some(name) => workbook
            .sheet_by_name_mut(name)
            .map_err(|_| XlsxFormatError::MissingWorksheet(name.to_string()))?,
        None => workbook
            .sheet_mut(0)
            .map_err(|_| XlsxFormatError::NoWorksheets)?,
    };
    replace_table(
        worksheet,
        &fields,
        &columns,
        &records,
        ReplacementOptions {
            header_row: start_row,
            headers,
            has_header,
        },
    )?;

    let mut bytes = Vec::new();
    umya_spreadsheet::writer::xlsx::write_writer(&workbook, &mut bytes)
        .map_err(|error| XlsxFormatError::Update(error.to_string()))?;
    std::fs::write(path, bytes)?;
    Ok(())
}

fn replace_table(
    worksheet: &mut Worksheet,
    fields: &[(&str, ScalarType)],
    columns: &[u32],
    records: &[Vec<&Value>],
    options: ReplacementOptions<'_>,
) -> Result<(), XlsxFormatError> {
    let header_row = options.header_row;
    let data_start = header_row
        .checked_add(u32::from(options.has_header))
        .ok_or(XlsxFormatError::InvalidCoordinate)?;
    let columns = columns.iter().map(|column| column + 1).collect::<Vec<_>>();
    let data_styles = columns
        .iter()
        .map(|column| {
            worksheet
                .cell((*column, data_start))
                .map(|cell| cell.style().clone())
        })
        .collect::<Vec<_>>();
    let highest_row = worksheet.highest_row();
    for row in data_start..=highest_row {
        for column in &columns {
            clear_cell(worksheet, *column, row);
        }
    }
    if options.has_header {
        for (index, ((name, _), column)) in fields.iter().zip(&columns).enumerate() {
            let header = options.headers.get(index).map_or(*name, String::as_str);
            replace_value(
                worksheet,
                *column,
                header_row,
                None,
                ScalarType::String,
                &Value::String(header.to_string()),
                0,
                name,
            )?;
        }
    }
    for (offset, record) in records.iter().enumerate() {
        let row = data_start
            .checked_add(u32::try_from(offset).map_err(|_| XlsxFormatError::InvalidCoordinate)?)
            .ok_or(XlsxFormatError::InvalidCoordinate)?;
        for ((((name, ty), value), column), style) in
            fields.iter().zip(record).zip(&columns).zip(&data_styles)
        {
            replace_value(
                worksheet,
                *column,
                row,
                style.as_ref(),
                *ty,
                value,
                offset,
                name,
            )?;
        }
    }
    Ok(())
}

fn clear_cell(worksheet: &mut Worksheet, column: u32, row: u32) {
    if worksheet
        .cell((column, row))
        .is_some_and(|cell| cell.is_formula())
    {
        return;
    }
    let style = worksheet
        .cell((column, row))
        .map(|cell| cell.style().clone());
    worksheet.remove_cell((column, row));
    if let Some(style) = style {
        worksheet.cell_mut((column, row)).set_style(style);
    }
}

#[allow(clippy::too_many_arguments)]
fn replace_value(
    worksheet: &mut Worksheet,
    column: u32,
    row: u32,
    fallback_style: Option<&Style>,
    ty: ScalarType,
    value: &Value,
    row_index: usize,
    field: &str,
) -> Result<(), XlsxFormatError> {
    let existing_style = worksheet
        .cell((column, row))
        .map(|cell| cell.style().clone())
        .or_else(|| fallback_style.cloned());
    worksheet.remove_cell((column, row));
    if matches!(value, Value::Null | Value::JsonNull(_)) {
        if let Some(style) = existing_style {
            worksheet.cell_mut((column, row)).set_style(style);
        }
        return Ok(());
    }
    let cell = worksheet.cell_mut((column, row));
    if let Some(style) = existing_style {
        cell.set_style(style);
    }
    let bad = |got| XlsxFormatError::ValueType {
        row: row_index,
        field: field.to_string(),
        expected: ty,
        got,
    };
    match (ty, value) {
        (ScalarType::String, Value::String(value)) => {
            cell.set_value_string(value);
        }
        (ScalarType::String, Value::Bool(value)) => {
            cell.set_value_bool(*value);
        }
        (ScalarType::String, Value::Int(value)) => {
            cell.set_value_string(value.to_string());
        }
        (ScalarType::String, Value::Float(value)) if value.is_finite() => {
            cell.set_value_string(value.to_string());
        }
        (ScalarType::Int | ScalarType::Float, Value::Int(value)) => {
            cell.set_value_number(
                exact_f64(*value).ok_or_else(|| bad("int outside the exact f64 range"))?,
            );
        }
        (ScalarType::Float, Value::Float(value)) if value.is_finite() => {
            cell.set_value_number(*value);
        }
        (ScalarType::Int, Value::String(value)) => {
            cell.set_value_number(
                lexical_i64(value)
                    .and_then(exact_f64)
                    .ok_or_else(|| bad("string"))?,
            );
        }
        (ScalarType::Float, Value::String(value)) => {
            cell.set_value_number(lexical_f64(value).ok_or_else(|| bad("string"))?);
        }
        (ScalarType::Bool, Value::Bool(value)) => {
            cell.set_value_bool(*value);
        }
        (ScalarType::Bool, Value::String(value)) => {
            cell.set_value_bool(boolean_lexical(value).ok_or_else(|| bad("string"))?);
        }
        (_, Value::Float(_)) => return Err(bad("non-finite float")),
        (_, other) => return Err(bad(other.type_name())),
    }
    Ok(())
}

/// Why a hierarchical update would discard existing workbook content. Every
/// refusal leaves the original workbook unchanged.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum HierarchicalUpdateRefusal {
    /// The package relates or contains a part that the workbook writer would
    /// drop or rewrite.
    #[error("`{part}` has a `{relationship}` relationship that the update cannot preserve")]
    UnsupportedPart { part: String, relationship: String },
    /// A relationships part the workbook reader would not use for its source
    /// part (it is not where the reader looks, or its source never keeps
    /// relationships), so the writer would drop the parts it relates.
    #[error("relationships in `{part}` would not be loaded with their source part")]
    UnloadableRelationships { part: String },
    /// A relationship reaches a part that is already being walked; the
    /// workbook reader would follow it without end.
    #[error("relationships reach `{part}` again from within itself")]
    RelationshipCycle { part: String },
    /// A worksheet element refers to a relationship the workbook reader does
    /// not keep for that element (such as a header/footer picture's
    /// `legacyDrawingHF`), or names a missing or differently typed one.
    #[error("`{part}` refers to a relationship from `<{element}>` that the update cannot preserve")]
    UnsupportedReference { part: String, element: String },
    /// An internal relationship nothing references; the workbook writer
    /// would drop its part.
    #[error("relationship `{id}` in `{part}` is not referenced, so its part would be dropped")]
    UnreferencedRelationship { part: String, id: String },
    /// A VML drawing holds shapes other than comment notes, or belongs to a
    /// worksheet without comments; the workbook writer keeps neither.
    #[error("VML drawing `{part}` holds shapes the update cannot preserve")]
    UnsupportedVmlShape { part: String },
    /// A drawing holds content the workbook reader drops, such as an
    /// absolute anchor, `mc:AlternateContent`, or a chart, connector, or
    /// nested group inside a group, so its pictures or charts would be lost.
    #[error("drawing `{part}` holds `<{element}>`, which the update cannot preserve")]
    UnsupportedDrawingContent { part: String, element: String },
    /// A chart holds content the workbook reader does not model, such as a
    /// stock or surface chart, so rewriting it would drop that content.
    #[error("chart `{part}` holds `<{element}>`, which the update cannot preserve")]
    UnsupportedChart { part: String, element: String },
    /// An all-null row in an open-ended band. Reads end such a band at its
    /// first blank row, so the row and every later band would be misplaced.
    #[error("worksheet `{sheet}` row {row} of open-ended band `{band}` has no values")]
    BlankOpenRow {
        sheet: String,
        band: String,
        row: u32,
    },
    /// Two row bands address the same cell.
    #[error("worksheet `{sheet}` cell {cell} is addressed by more than one row band")]
    OverlappingWrite { sheet: String, cell: String },
    /// A cell the layout writes or clears holds a formula.
    #[error("worksheet `{sheet}` cell {cell} inside the layout holds a formula")]
    FormulaCell { sheet: String, cell: String },
    /// A cell the layout writes or clears belongs to a merged range.
    #[error("worksheet `{sheet}` cell {cell} inside the layout belongs to merged range {merge}")]
    MergedCell {
        sheet: String,
        cell: String,
        merge: String,
    },
    /// A newly addressed cell holds content outside the layout's previous extent.
    #[error("worksheet `{sheet}` cell {cell} holds content outside the previous layout extent")]
    OccupiedCell { sheet: String, cell: String },
}

#[derive(Debug, Error)]
pub enum HierarchicalUpdateError {
    #[error(transparent)]
    Format(#[from] XlsxFormatError),
    #[error("cannot update hierarchical XLSX workbook: {0}")]
    Refused(#[from] HierarchicalUpdateRefusal),
}

impl From<std::io::Error> for HierarchicalUpdateError {
    fn from(error: std::io::Error) -> Self {
        Self::Format(error.into())
    }
}

impl From<rust_xlsxwriter::XlsxError> for HierarchicalUpdateError {
    fn from(error: rust_xlsxwriter::XlsxError) -> Self {
        Self::Format(error.into())
    }
}

const MAX_CELL_TEXT_CHARS: usize = 32_767;
const MAX_STAGING_ATTEMPTS: u32 = 64;

/// Writes hierarchical row bands into the runtime-named worksheets of an
/// existing workbook and returns the worksheet count.
///
/// Every worksheet produced by the mapping must already exist; other
/// worksheets and cells outside the layout keep their content and styles.
/// Rewritten cells keep their style, and new data cells take the style of
/// their band's first data row. The cells the layout occupied before the
/// update (the extent a hierarchical read would consume) are cleared first,
/// so shorter output leaves no stale rows. Nothing is inserted or shifted.
///
/// Parts no relationship reaches and external hyperlinks nothing references
/// are not used by the workbook and are dropped. An internal worksheet or
/// drawing relationship nothing references is refused, except a worksheet's
/// comments, which the reader loads without a reference; so are drawing
/// content and chart content the reader drops.
///
/// As in the flat update, worksheet features the workbook library does not
/// model are dropped, for example `extLst` sparklines and x14 conditional
/// formats, `ignoredErrors`, `protectedRanges`, and `customSheetViews`; chart
/// formatting below the modeled chart, plot area, and axis elements is
/// rewritten from the library's model.
///
/// The complete workbook is validated and serialized before it atomically
/// replaces the original. A symlink is resolved, and its target, which must
/// be a regular file, is replaced.
/// The replacement receives the original's permission bits, but not its
/// owner, group, or other metadata; a workbook the caller cannot open for
/// writing is refused.
/// Refusals, invalid values, and limit violations leave the file unchanged.
pub fn update_hierarchical(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    layout: &XlsxHierarchicalLayout,
    limits: &XlsxLimits,
) -> Result<usize, HierarchicalUpdateError> {
    let target = std::fs::canonicalize(path)?;
    let metadata = std::fs::metadata(&target)?;
    // Opening a FIFO would block and renaming over a device would replace it.
    if !metadata.is_file() {
        return Err(
            std::io::Error::new(ErrorKind::InvalidInput, "workbook is not a regular file").into(),
        );
    }
    // Renaming over a file the caller cannot write would bypass its
    // protection on Unix and fail on Windows; neither is an update of a
    // writable workbook. Opening without truncation changes nothing; the
    // permission bits still apply to callers the open would allow anyway.
    if metadata.permissions().readonly() {
        return Err(
            std::io::Error::new(ErrorKind::PermissionDenied, "workbook is read-only").into(),
        );
    }
    OpenOptions::new().write(true).open(&target)?;
    let maximum = limits.options().max_input_bytes;
    check(
        XlsxLimit::InputBytes,
        usize::try_from(metadata.len()).unwrap_or(usize::MAX),
        maximum,
    )?;
    let mut existing = Vec::new();
    std::fs::File::open(&target)?
        .take(u64::try_from(maximum).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut existing)?;
    let (bytes, worksheet_count) =
        update_hierarchical_bytes(&existing, schema, instance, layout, limits)?;
    publish(&target, &bytes, &metadata.permissions())?;
    Ok(worksheet_count)
}

/// In-memory form of [`update_hierarchical`]: returns the updated workbook
/// bytes and the worksheet count without touching the filesystem.
///
/// The output plan is built and bounded before the workbook is opened. The
/// package is then expanded with real byte counts, every XML part is checked
/// against the XML budgets, and the relationship graph is walked the way the
/// workbook reader resolves it, all before that reader parses anything.
///
/// The planned cells, the previous extent the update clears, and the merged
/// ranges and probes checked against both each have their own `max_cells`
/// allowance, so a workbook can take output as large as the output it holds.
pub fn update_hierarchical_bytes(
    existing: &[u8],
    schema: &SchemaNode,
    instance: &Instance,
    layout: &XlsxHierarchicalLayout,
    limits: &XlsxLimits,
) -> Result<(Vec<u8>, usize), HierarchicalUpdateError> {
    let options = limits.options();
    check(
        XlsxLimit::InputBytes,
        existing.len(),
        options.max_input_bytes,
    )?;
    validate_layout(schema, layout)?;
    let worksheets = repeated_at(instance, &layout.worksheets_path)?;
    if worksheets.is_empty() {
        return Err(XlsxFormatError::NoWorksheets.into());
    }
    let mut names = BTreeSet::new();
    let mut cell_count = 0_usize;
    let mut plans = Vec::with_capacity(worksheets.len());
    for worksheet_instance in worksheets {
        let name = worksheet_name(worksheet_instance, layout)?;
        if !names.insert(name) {
            return Err(XlsxFormatError::DuplicateWorksheet(name.to_string()).into());
        }
        let cells = plan_cells(name, worksheet_instance, layout, options, &mut cell_count)?;
        plans.push((name, cells));
    }

    package::inspect_package(existing, options)?;
    let mut workbook = guarded("read", || {
        umya_spreadsheet::reader::xlsx::read_reader(Cursor::new(existing), true)
    })?;
    let (mut previous_count, mut probe_count) = (0_usize, 0_usize);
    for (name, planned) in plans {
        let worksheet = workbook
            .sheet_by_name_mut(name)
            .map_err(|_| XlsxFormatError::MissingWorksheet(name.to_string()))?;
        let previous = previous_extent(worksheet, layout, options, &mut previous_count)?;
        check_region(
            worksheet,
            name,
            &planned,
            &previous,
            options,
            &mut probe_count,
        )?;
        for &(row, column) in previous.iter().filter(|cell| !planned.contains_key(cell)) {
            clear_cell(worksheet, column, row);
        }
        for ((row, column), cell) in planned {
            write_layout_cell(worksheet, column, row, cell);
        }
    }

    let bytes = guarded("write", || {
        let mut bytes = Vec::new();
        umya_spreadsheet::writer::xlsx::write_writer(&workbook, &mut bytes).map(|()| bytes)
    })?;
    check(
        XlsxLimit::OutputBytes,
        bytes.len(),
        options.max_output_bytes,
    )?;
    Ok((bytes, worksheets.len()))
}

/// Runs a workbook reader/writer step, converting its errors and panics on
/// content the preflight could not anticipate into an update error.
fn guarded<T, E: std::fmt::Display>(
    step: &str,
    operation: impl FnOnce() -> Result<T, E>,
) -> Result<T, XlsxFormatError> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)) {
        Ok(result) => result.map_err(|error| XlsxFormatError::Update(error.to_string())),
        Err(_) => Err(XlsxFormatError::Update(format!(
            "workbook {step} failed on unsupported content"
        ))),
    }
}

/// `(row, column)`, both one-based; ordered row-major.
type Coordinate = (u32, u32);

struct PlannedCell<'a> {
    content: Option<CellContent<'a>>,
    band_data_start: Option<u32>,
}

fn plan_cells<'a>(
    sheet: &str,
    instance: &'a Instance,
    layout: &'a XlsxHierarchicalLayout,
    limits: &XlsxLimitOptions,
    cell_count: &mut usize,
) -> Result<BTreeMap<Coordinate, PlannedCell<'a>>, HierarchicalUpdateError> {
    let mut planned = BTreeMap::new();
    // Cells of one band row are visited contiguously: (cells seen, any value).
    let mut open_row = (0_usize, false);
    visit_worksheet(
        instance,
        layout,
        |cell| -> Result<(), HierarchicalUpdateError> {
            add(cell_count, 1, XlsxLimit::Cells, limits.max_cells)?;
            check(XlsxLimit::Rows, cell.row as usize, limits.max_rows)?;
            check(XlsxLimit::Columns, cell.column as usize, limits.max_columns)?;
            if let Some(CellContent::Text(text)) = &cell.content {
                check(XlsxLimit::StringBytes, text.len(), limits.max_string_bytes)?;
                // Matches the new-workbook writer's refusal of oversized text.
                if text.chars().count() > MAX_CELL_TEXT_CHARS {
                    return Err(rust_xlsxwriter::XlsxError::MaxStringLengthExceeded.into());
                }
            }
            if let Some(data) = cell.data.filter(|data| data.range.count.is_none()) {
                open_row.0 += 1;
                open_row.1 |= cell.content.is_some();
                if open_row.0 == data.range.columns.len() {
                    if !open_row.1 {
                        return Err(HierarchicalUpdateRefusal::BlankOpenRow {
                            sheet: sheet.to_string(),
                            band: data.range.path.join("/"),
                            row: cell.row,
                        }
                        .into());
                    }
                    open_row = (0, false);
                }
            }
            let coordinate = (cell.row, cell.column);
            let previous = planned.insert(
                coordinate,
                PlannedCell {
                    content: cell.content,
                    band_data_start: cell.data.map(|data| data.data_start),
                },
            );
            if previous.is_some() {
                return Err(HierarchicalUpdateRefusal::OverlappingWrite {
                    sheet: sheet.to_string(),
                    cell: cell_name(coordinate),
                }
                .into());
            }
            Ok(())
        },
    )?;
    Ok(planned)
}

/// Returns the occupied cells the layout addresses in the existing worksheet,
/// walking the bands exactly as a hierarchical read would.
fn previous_extent(
    worksheet: &Worksheet,
    layout: &XlsxHierarchicalLayout,
    limits: &XlsxLimitOptions,
    cell_count: &mut usize,
) -> Result<BTreeSet<Coordinate>, XlsxFormatError> {
    let highest_row = worksheet.highest_row();
    let mut extent = BTreeSet::new();
    let mut include = |row: u32, column: u32| -> Result<(), XlsxFormatError> {
        if is_occupied(worksheet, column, row) {
            add(cell_count, 1, XlsxLimit::Cells, limits.max_cells)?;
            extent.insert((row, column));
        }
        Ok(())
    };
    let mut previous_end = 0_u32;
    for range in &layout.ranges {
        let start = range_start(range, previous_end)?;
        if range.has_header {
            for column in range
                .columns
                .iter()
                .filter(|column| column.header.is_some())
            {
                include(start, column.column.get())?;
            }
        }
        let data_start = start
            .checked_add(u32::from(range.has_header))
            .ok_or(XlsxFormatError::InvalidCoordinate)?;
        let next_row = match range.count {
            Some(count) => {
                let next_row = data_start
                    .checked_add(count.get())
                    .ok_or(XlsxFormatError::InvalidCoordinate)?;
                // Rows past the last stored row cannot hold content.
                for row in data_start..next_row.min(highest_row.saturating_add(1)) {
                    for column in &range.columns {
                        include(row, column.column.get())?;
                    }
                }
                next_row
            }
            None => {
                let mut row = data_start;
                while row <= MAX_WORKSHEET_ROW
                    && range
                        .columns
                        .iter()
                        .any(|column| is_occupied(worksheet, column.column.get(), row))
                {
                    for column in &range.columns {
                        include(row, column.column.get())?;
                    }
                    row = row
                        .checked_add(1)
                        .ok_or(XlsxFormatError::InvalidCoordinate)?;
                }
                row
            }
        };
        previous_end = range_end(range, start, next_row)?;
    }
    Ok(extent)
}

fn is_occupied(worksheet: &Worksheet, column: u32, row: u32) -> bool {
    worksheet
        .cell((column, row))
        .is_some_and(|cell| cell.is_formula() || !cell.cell_value().is_empty())
}

/// Refuses addressed cells inside merged ranges, holding formulas, or holding
/// template content. Merge probes are indexed by row and charged to the cell
/// budget, so a workbook with many merged ranges cannot make this unbounded.
fn check_region(
    worksheet: &Worksheet,
    sheet: &str,
    planned: &BTreeMap<Coordinate, PlannedCell<'_>>,
    previous: &BTreeSet<Coordinate>,
    limits: &XlsxLimitOptions,
    cell_count: &mut usize,
) -> Result<(), HierarchicalUpdateError> {
    let mut addressed = BTreeMap::<u32, BTreeSet<u32>>::new();
    for &(row, column) in planned.keys().chain(previous) {
        addressed.entry(row).or_default().insert(column);
    }
    for merge in worksheet.merge_cells() {
        add(cell_count, 1, XlsxLimit::Cells, limits.max_cells)?;
        let first_column = merge
            .coordinate_start_col()
            .map_or(1, |column| column.num());
        let first_row = merge.coordinate_start_row().map_or(1, |row| row.num());
        let last_column = merge
            .coordinate_end_col()
            .map_or(MAX_WORKSHEET_COLUMN, |column| column.num());
        let last_row = merge
            .coordinate_end_row()
            .map_or(MAX_WORKSHEET_ROW, |row| row.num());
        if first_row > last_row || first_column > last_column {
            continue;
        }
        for (&row, columns) in addressed.range(first_row..=last_row) {
            add(cell_count, 1, XlsxLimit::Cells, limits.max_cells)?;
            if let Some(&column) = columns.range(first_column..=last_column).next() {
                return Err(HierarchicalUpdateRefusal::MergedCell {
                    sheet: sheet.to_string(),
                    cell: cell_name((row, column)),
                    merge: merge.range(),
                }
                .into());
            }
        }
    }
    for &coordinate in planned.keys().chain(previous) {
        let (row, column) = coordinate;
        if worksheet
            .cell((column, row))
            .is_some_and(|cell| cell.is_formula())
        {
            return Err(HierarchicalUpdateRefusal::FormulaCell {
                sheet: sheet.to_string(),
                cell: cell_name(coordinate),
            }
            .into());
        }
        if !previous.contains(&coordinate) && is_occupied(worksheet, column, row) {
            return Err(HierarchicalUpdateRefusal::OccupiedCell {
                sheet: sheet.to_string(),
                cell: cell_name(coordinate),
            }
            .into());
        }
    }
    Ok(())
}

fn write_layout_cell(worksheet: &mut Worksheet, column: u32, row: u32, cell: PlannedCell<'_>) {
    let style = worksheet
        .cell((column, row))
        .or_else(|| {
            cell.band_data_start
                .and_then(|data_start| worksheet.cell((column, data_start)))
        })
        .map(|existing| existing.style().clone());
    worksheet.remove_cell((column, row));
    let Some(content) = cell.content else {
        if let Some(style) = style {
            worksheet.cell_mut((column, row)).set_style(style);
        }
        return;
    };
    let target = worksheet.cell_mut((column, row));
    if let Some(style) = style {
        target.set_style(style);
    }
    match content {
        CellContent::Text(value) => {
            target.set_value_string(excel_text(&value).into_owned());
        }
        CellContent::Number(value) => {
            target.set_value_number(value);
        }
        CellContent::Boolean(value) => {
            target.set_value_bool(value);
        }
        CellContent::DateTime(datetime, kind) => {
            target.set_value_number(datetime.to_excel());
            // Keep a template's own date/time format; any other format would
            // show (and read back) the bare serial number.
            if !target.style().number_format().is_some_and(|format| {
                matches!(format.number_format_id(), 14..=22 | 45..=47)
                    || is_datetime_format_code(format.format_code())
            }) {
                target
                    .style_mut()
                    .number_format_mut()
                    .set_format_code(datetime_format_code(kind));
            }
        }
    }
}

/// Encodes cell text as the new-workbook writer (and Excel) does, since the
/// workbook writer escapes only `&`, `<` and `>`: a literal `_xHHHH_` gains
/// an escaped underscore, and XML-illegal control characters and
/// U+FFFE/U+FFFF become `_xHHHH_`. Tab and line feed stay literal.
fn excel_text(text: &str) -> Cow<'_, str> {
    let illegal =
        |c: char| matches!(c, '\0'..='\u{8}' | '\u{b}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}');
    let escape_at = |index: usize| {
        text.get(index..index + 7).is_some_and(|candidate| {
            candidate.starts_with("_x")
                && candidate.ends_with('_')
                && candidate[2..6].bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    };
    if !text.chars().any(illegal) && !text.match_indices("_x").any(|(index, _)| escape_at(index)) {
        return Cow::Borrowed(text);
    }
    let mut encoded = String::with_capacity(text.len() + 7);
    for (index, character) in text.char_indices() {
        if character == '_' && escape_at(index) {
            encoded.push_str("_x005F");
        }
        if illegal(character) {
            encoded.push_str(&format!("_x{:04X}_", u32::from(character)));
        } else {
            encoded.push(character);
        }
    }
    Cow::Owned(encoded)
}

/// Whether the workbook reader classifies a custom number format as a date or
/// time: a date/time token in the first section, outside quotes, brackets, and
/// escaped characters.
fn is_datetime_format_code(code: &str) -> bool {
    let (mut escaped, mut quoted, mut brackets, mut meridiem) = (false, false, 0_u32, false);
    for character in code.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '_' | '\\' | '*' => escaped = true,
            '"' => quoted = !quoted,
            _ if quoted => {}
            ';' => return false,
            '[' => brackets += 1,
            ']' => brackets = brackets.saturating_sub(1),
            _ if brackets > 0 => {}
            'a' | 'A' if !meridiem => meridiem = true,
            'p' | 'P' | 'm' | 'M' | '/' if meridiem => return true,
            'd' | 'D' | 'm' | 'M' | 'h' | 'H' | 'y' | 'Y' | 's' | 'S' if !meridiem => return true,
            _ => {}
        }
    }
    false
}

fn cell_name((row, column): Coordinate) -> String {
    umya_spreadsheet::helper::coordinate::coordinate_from_index(column, row)
}

/// Stages the bytes beside `target` and renames them over it, so readers see
/// either the original workbook or the complete update.
fn publish(target: &Path, bytes: &[u8], permissions: &std::fs::Permissions) -> std::io::Result<()> {
    let invalid_path =
        || std::io::Error::new(ErrorKind::InvalidInput, "workbook path has no file name");
    let directory = target.parent().ok_or_else(invalid_path)?;
    let file_name = target.file_name().ok_or_else(invalid_path)?;
    let (staged, file) = create_staging(directory, &file_name.to_string_lossy())?;
    let result = stage(file, bytes, permissions).and_then(|()| std::fs::rename(&staged, target));
    if result.is_err() {
        let _ = std::fs::remove_file(&staged);
        return result;
    }
    // Make the rename itself durable. Best effort: the workbook has already
    // been replaced, so a failure here must not be reported as a failed
    // update. Windows cannot open a directory as a file.
    #[cfg(unix)]
    if let Ok(directory) = std::fs::File::open(directory) {
        let _ = directory.sync_all();
    }
    Ok(())
}

/// Creates a new, owner-only staging file beside the workbook.
fn create_staging(
    directory: &Path,
    file_name: &str,
) -> std::io::Result<(std::path::PathBuf, std::fs::File)> {
    static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);
    for _ in 0..MAX_STAGING_ATTEMPTS {
        let staged = directory.join(format!(
            ".{file_name}.{}-{}.ferrule-update",
            std::process::id(),
            NEXT_STAGE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        match options.open(&staged) {
            Ok(file) => return Ok((staged, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        ErrorKind::AlreadyExists,
        "could not create a unique staging file beside the workbook",
    ))
}

/// Writes and syncs the staged workbook and gives it the original's
/// permission bits, all before it replaces the original.
fn stage(
    mut file: std::fs::File,
    bytes: &[u8],
    permissions: &std::fs::Permissions,
) -> std::io::Result<()> {
    file.write_all(bytes)?;
    file.set_permissions(permissions.clone())?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use calamine::{Data, Reader};
    use rust_xlsxwriter::{Format, Formula, Workbook};

    use super::*;

    fn row(month: &str, west: f64) -> Instance {
        Instance::Group(
            (vec![
                (
                    "Month".into(),
                    Instance::Scalar(Value::String(month.into())),
                ),
                ("West".into(), Instance::Scalar(Value::Float(west))),
            ])
            .into(),
        )
    }

    #[test]
    fn replaces_only_the_selected_table_and_removes_stale_rows() {
        let path =
            std::env::temp_dir().join(format!("ferrule_xlsx_update_{}.xlsx", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut workbook = Workbook::new();
        let sales = workbook.add_worksheet();
        sales.set_name("Sales").unwrap();
        sales
            .merge_range(0, 0, 0, 1, "Report title", &Format::new())
            .unwrap();
        sales.write_string(4, 0, "Old month").unwrap();
        sales.write_string(4, 1, "Old west").unwrap();
        sales.write_string(5, 0, "Old row").unwrap();
        sales.write_number(5, 1, 1.0).unwrap();
        sales.write_string(6, 0, "Stale row").unwrap();
        sales.write_number(6, 1, 2.0).unwrap();
        sales.write_string(5, 3, "Preserve beside table").unwrap();
        sales
            .write_formula(7, 1, Formula::new("=SUM(B6:B7)"))
            .unwrap();
        sales
            .write_formula(7, 2, Formula::new("=SUM(B6:B7)"))
            .unwrap();
        let keep = workbook.add_worksheet();
        keep.set_name("Keep").unwrap();
        keep.write_string(0, 0, "Preserve other sheet").unwrap();
        workbook.save(&path).unwrap();
        let schema = SchemaNode::group(
            "Sales",
            vec![
                SchemaNode::scalar("Month", ScalarType::String),
                SchemaNode::scalar("West", ScalarType::Float),
            ],
        );

        let headers = vec!["Measure".to_string(), "Measure".to_string()];
        update_with_options(
            &path,
            &schema,
            &[row("January", 4.5)],
            FlatTableWriteOptions {
                sheet: Some("Sales"),
                start_row: 5,
                columns: &[1, 2],
                headers: &headers,
                has_header: true,
            },
        )
        .unwrap();

        let preserved = umya_spreadsheet::reader::xlsx::read(&path).unwrap();
        let preserved_sales = preserved.sheet_by_name("Sales").unwrap();
        assert!(
            preserved_sales
                .cell((2, 8))
                .is_some_and(|cell| cell.is_formula())
        );
        assert!(
            preserved_sales
                .cell((3, 8))
                .is_some_and(|cell| cell.is_formula())
        );
        assert!(
            preserved_sales
                .merge_cells()
                .iter()
                .any(|range| range.range() == "A1:B1")
        );

        let mut result: calamine::Xlsx<_> = calamine::open_workbook(&path).unwrap();
        let sales = result.worksheet_range("Sales").unwrap();
        let keep = result.worksheet_range("Keep").unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            sales.get_value((0, 0)),
            Some(&Data::String("Report title".into()))
        );
        assert_eq!(
            sales.get_value((4, 0)),
            Some(&Data::String("Measure".into()))
        );
        assert_eq!(
            sales.get_value((4, 1)),
            Some(&Data::String("Measure".into()))
        );
        assert_eq!(
            sales.get_value((5, 0)),
            Some(&Data::String("January".into()))
        );
        assert_eq!(sales.get_value((5, 1)), Some(&Data::Float(4.5)));
        assert!(matches!(sales.get_value((6, 0)), None | Some(Data::Empty)));
        assert_eq!(
            sales.get_value((5, 3)),
            Some(&Data::String("Preserve beside table".into()))
        );
        assert_eq!(
            keep.get_value((0, 0)),
            Some(&Data::String("Preserve other sheet".into()))
        );
    }

    #[test]
    fn classifies_datetime_number_formats_like_the_reader() {
        for code in [
            "yyyy-mm-dd",
            "dd/mm/yyyy",
            "hh:mm AM/PM",
            "[h]:mm:ss",
            "mmm-yy",
        ] {
            assert!(is_datetime_format_code(code), "{code}");
        }
        for code in ["General", "0.00", "[Red]0", "\"d\"0", "0;dd", "_d0", "@"] {
            assert!(!is_datetime_format_code(code), "{code}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn staging_files_are_created_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let directory =
            std::env::temp_dir().join(format!("ferrule_xlsx_staging_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();

        let (staged, file) = create_staging(&directory, "report.xlsx").unwrap();
        let mode = file.metadata().unwrap().permissions().mode() & 0o777;
        drop(file);
        std::fs::remove_dir_all(&directory).unwrap();

        assert_eq!(mode, 0o600, "{}", staged.display());
    }
}
