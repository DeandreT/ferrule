use serde::{Deserialize, Serialize};

use crate::{XlsxColumn, XlsxRow};

/// Starting position for one output row range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum XlsxRangeStart {
    /// An absolute one-based worksheet row.
    Absolute { row: XlsxRow },
    /// A one-based offset after the last row occupied by the previous range.
    AfterPrevious { offset: XlsxRow },
}

/// Physical XLSX cell representation retained separately from the IR scalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XlsxCellKind {
    String,
    Number,
    Boolean,
    Date,
    DateTime,
    Time,
}

/// One mapped cell in a hierarchical output row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XlsxOutputColumn {
    /// Path relative to the range row group.
    pub path: Vec<String>,
    pub column: XlsxColumn,
    /// Header value written when the range enables a title row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    pub kind: XlsxCellKind,
}

/// One ordered row band within every generated worksheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XlsxOutputRange {
    /// Path relative to the worksheet group. A repeating terminal group
    /// supplies dynamic rows; a non-repeating group supplies one row.
    pub path: Vec<String>,
    pub start: XlsxRangeStart,
    /// Maximum data rows, excluding the optional title row. `None` is dynamic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<XlsxRow>,
    pub has_header: bool,
    pub columns: Vec<XlsxOutputColumn>,
}

/// Hierarchical XLSX target with runtime-named repeated worksheets and
/// ordered row ranges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XlsxHierarchicalLayout {
    /// Absolute path to the repeated worksheet group in the target instance.
    pub worksheets_path: Vec<String>,
    /// Scalar path relative to each worksheet group.
    pub worksheet_name_path: Vec<String>,
    pub ranges: Vec<XlsxOutputRange>,
}

#[cfg(test)]
mod tests {
    use crate::{FormatOptions, XlsxCellKind, XlsxColumn, XlsxRow};

    use super::{XlsxHierarchicalLayout, XlsxOutputColumn, XlsxOutputRange, XlsxRangeStart};

    #[test]
    fn hierarchical_layout_roundtrips() {
        let layout = XlsxHierarchicalLayout {
            worksheets_path: vec!["Sheets".into()],
            worksheet_name_path: vec!["Name".into()],
            ranges: vec![XlsxOutputRange {
                path: vec!["People".into()],
                start: XlsxRangeStart::Absolute {
                    row: XlsxRow::new(2).unwrap(),
                },
                count: None,
                has_header: true,
                columns: vec![XlsxOutputColumn {
                    path: vec!["DisplayName".into()],
                    column: XlsxColumn::new(3).unwrap(),
                    header: Some("Display name".into()),
                    kind: XlsxCellKind::String,
                }],
            }],
        };
        let options = FormatOptions {
            xlsx_hierarchical: Some(layout.clone()),
            ..FormatOptions::default()
        };

        let encoded = serde_json::to_string(&options).unwrap();
        let decoded: FormatOptions = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded.xlsx_hierarchical, Some(layout));
    }
}
