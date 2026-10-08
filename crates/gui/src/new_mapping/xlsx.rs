use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Context as _, bail};
use ir::SchemaNode;
use mapping::{FormatOptions, TabularBoundaryKind, XlsxColumn, XlsxRow};

use super::ImportedSchema;

/// A flat worksheet setup remains separate from the open mapping and does not
/// read a workbook. Field identity and order come from the imported schema.
pub(crate) struct XlsxBoundaryDraft {
    pub(crate) schema_path: PathBuf,
    pub(crate) schema: SchemaNode,
    pub(crate) data_path: String,
    pub(crate) sheet: String,
    pub(crate) start_row: String,
    pub(crate) has_header_row: bool,
    pub(crate) columns: Vec<String>,
    pub(crate) headers: Vec<String>,
    pub(crate) transposed: bool,
    pub(crate) rows: Vec<String>,
}

impl XlsxBoundaryDraft {
    pub(crate) fn supports_schema(schema: &SchemaNode) -> bool {
        Self::fields(schema).is_ok()
    }

    fn fields(schema: &SchemaNode) -> anyhow::Result<Vec<&SchemaNode>> {
        if let ir::SchemaKind::Group {
            alternatives,
            dynamic,
            ..
        } = &schema.kind
            && (!alternatives.is_empty() || dynamic.is_some())
        {
            bail!(
                "workbook setup needs one closed row shape without alternative or computed fields"
            );
        }
        let fields =
            crate::extra_targets::flat_scalar_fields(schema).map_err(anyhow::Error::msg)?;
        if fields.len() > 256 {
            bail!("workbook setup supports at most 256 fields");
        }
        if fields.iter().any(|field| field.attribute || field.text) {
            bail!("workbook fields must be ordinary scalar fields, not XML attributes or text");
        }
        Ok(fields)
    }

    pub(crate) fn from_imported(imported: &ImportedSchema) -> anyhow::Result<Self> {
        let fields = Self::fields(&imported.schema)?;
        Ok(Self {
            schema_path: imported.path.clone(),
            schema: imported.schema.clone(),
            data_path: String::new(),
            sheet: String::new(),
            start_row: "1".into(),
            has_header_row: true,
            columns: (1..=fields.len())
                .map(|column| column.to_string())
                .collect(),
            headers: fields.iter().map(|field| field.name.clone()).collect(),
            transposed: false,
            rows: (1..=fields.iter().filter(|field| field.name != "n").count())
                .map(|row| row.to_string())
                .collect(),
        })
    }

    pub(crate) fn into_imported(self) -> ImportedSchema {
        ImportedSchema {
            path: self.schema_path,
            schema: self.schema,
        }
    }

    pub(crate) fn supports_transposed(&self) -> bool {
        Self::transposed_field_count(&self.schema).is_ok()
    }

    fn transposed_field_count(schema: &SchemaNode) -> anyhow::Result<usize> {
        let fields = Self::fields(schema)?;
        let positions = fields
            .iter()
            .filter(|field| field.name == "n")
            .collect::<Vec<_>>();
        if positions.len() > 1
            || positions.iter().any(|field| {
                !matches!(
                    &field.kind,
                    ir::SchemaKind::Scalar {
                        ty: ir::ScalarType::Int
                    }
                )
            })
        {
            bail!("transposed source field `n` must be one integer physical-column position");
        }
        let count = fields.len() - positions.len();
        if count == 0 {
            bail!("transposed sources need at least one worksheet data field besides `n`");
        }
        Ok(count)
    }

    fn selected_rows(&self) -> anyhow::Result<Vec<u32>> {
        let count = Self::transposed_field_count(&self.schema)?;
        if self.rows.len() != count {
            bail!(
                "expected {count} worksheet row selector(s), got {}",
                self.rows.len()
            );
        }
        let mut selected = Vec::with_capacity(count);
        let mut unique = BTreeSet::new();
        for raw in &self.rows {
            let row = raw
                .trim()
                .parse::<u32>()
                .ok()
                .and_then(XlsxRow::new)
                .context("worksheet row must be between 1 and 1,048,576")?;
            if !unique.insert(row) {
                bail!("each data field needs a different worksheet row");
            }
            selected.push(row.get());
        }
        Ok(selected)
    }

    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        self.validate_for(false)
    }

    pub(crate) fn validate_for(&self, target: bool) -> anyhow::Result<()> {
        let fields = Self::fields(&self.schema)?;
        if target && self.transposed {
            bail!("transposed workbook layout is available only for sources");
        }
        if self.transposed {
            self.selected_rows()?;
        } else if self.columns.len() != fields.len() || self.headers.len() != fields.len() {
            bail!(
                "workbook column choices no longer match the schema; configure the workbook again"
            );
        }
        if self.data_path.as_bytes().contains(&0) {
            bail!("workbook file path cannot contain a null character");
        }
        let fallback = FormatOptions {
            tabular_kind: Some(TabularBoundaryKind::Xlsx),
            ..FormatOptions::default()
        };
        if !super::format_options::uses_xlsx_format(&fallback, &self.data_path) {
            bail!("choose an .xlsx file or a path without another recognized file type");
        }
        if !self.sheet.is_empty() {
            if self.sheet.chars().count() > 31 {
                bail!("sheet name cannot exceed 31 characters");
            }
            if self
                .sheet
                .contains(['*', '?', ':', '[', ']', '\\', '/', '\0'])
            {
                bail!("sheet name cannot contain *, ?, :, [, ], a slash, or a null character");
            }
            if self.sheet.starts_with('\'') || self.sheet.ends_with('\'') {
                bail!("sheet name cannot start or end with an apostrophe");
            }
        }
        if self.transposed {
            return Ok(());
        }
        self.start_row
            .trim()
            .parse::<u32>()
            .ok()
            .and_then(XlsxRow::new)
            .context("table row must be between 1 and 1,048,576")?;
        let mut columns = BTreeSet::new();
        for (field, raw) in fields.iter().zip(&self.columns) {
            let column = raw
                .trim()
                .parse::<u32>()
                .ok()
                .and_then(XlsxColumn::new)
                .with_context(|| {
                    format!("field `{}` needs a column between 1 and 16,384", field.name)
                })?;
            if !columns.insert(column) {
                bail!("each field needs a different worksheet column");
            }
        }
        Ok(())
    }

    pub(crate) fn options(&self, target: bool) -> anyhow::Result<FormatOptions> {
        if target {
            self.validate_for(true)?;
        } else {
            self.validate()?;
        }
        if self.transposed {
            return Ok(FormatOptions {
                tabular_kind: Some(TabularBoundaryKind::Xlsx),
                xlsx_sheet: (!self.sheet.is_empty()).then(|| self.sheet.clone()),
                xlsx_rows: self.selected_rows()?,
                ..FormatOptions::default()
            });
        }
        Ok(FormatOptions {
            tabular_kind: Some(TabularBoundaryKind::Xlsx),
            xlsx_sheet: (!self.sheet.is_empty()).then(|| self.sheet.clone()),
            xlsx_start_row: Some(self.start_row.trim().parse()?),
            xlsx_columns: self
                .columns
                .iter()
                .map(|value| value.trim().parse())
                .collect::<Result<Vec<_>, _>>()?,
            xlsx_headers: if target && self.has_header_row {
                self.headers.clone()
            } else {
                Vec::new()
            },
            has_header_row: Some(self.has_header_row),
            ..FormatOptions::default()
        })
    }

    pub(crate) fn instance_path(&self) -> Option<String> {
        let path = self.data_path.trim();
        (!path.is_empty()).then(|| path.to_owned())
    }
}
