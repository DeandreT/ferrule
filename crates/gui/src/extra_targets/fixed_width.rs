use ir::{SchemaKind, SchemaNode};
use mapping::{FixedFieldWidth, FixedWidthLayout};

/// Edits a layout without changing the saved boundary until setup is accepted.
#[derive(Debug, Clone)]
pub(crate) struct FixedWidthTargetDraft {
    schema: SchemaNode,
    pub(crate) widths: Vec<String>,
    pub(crate) fill: String,
    /// Delimited records accept LF or CRLF on input and write LF on output.
    pub(crate) record_delimiters: bool,
    pub(crate) treat_empty_as_absent: bool,
}

pub(crate) fn flat_scalar_fields(schema: &SchemaNode) -> Result<Vec<&SchemaNode>, String> {
    if schema.repeating {
        return Err("fixed-width root must not repeat; its scope supplies rows".into());
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err("fixed-width layout needs a flat group of scalar fields".into());
    };
    if children.is_empty() {
        return Err("fixed-width layout needs at least one scalar field".into());
    }
    if children
        .iter()
        .any(|field| !matches!(field.kind, SchemaKind::Scalar { .. }) || field.repeating)
    {
        return Err("fixed-width fields must be direct, non-repeating scalars".into());
    }
    let mut names = std::collections::BTreeSet::new();
    if children
        .iter()
        .any(|field| field.name.is_empty() || !names.insert(field.name.as_str()))
    {
        return Err("fixed-width fields need distinct, nonempty names".into());
    }
    Ok(children.iter().collect())
}

pub(crate) fn validate_layout_for_schema(
    schema: &SchemaNode,
    layout: &FixedWidthLayout,
) -> Result<(), String> {
    let fields = flat_scalar_fields(schema)?;
    if fields.len() != layout.field_widths().len() {
        return Err(format!(
            "fixed-width layout has {} widths for {} fields",
            layout.field_widths().len(),
            fields.len()
        ));
    }
    Ok(())
}

impl FixedWidthTargetDraft {
    pub(crate) fn from_schema(
        schema: &SchemaNode,
        existing: Option<&FixedWidthLayout>,
    ) -> Result<Self, String> {
        let fields = flat_scalar_fields(schema)?;
        if let Some(layout) = existing {
            validate_layout_for_schema(schema, layout)?;
        }
        Ok(Self {
            schema: schema.clone(),
            widths: existing.map_or_else(
                || vec!["8".to_owned(); fields.len()],
                |layout| {
                    layout
                        .field_widths()
                        .iter()
                        .map(|width| width.get().to_string())
                        .collect()
                },
            ),
            fill: existing.map_or_else(|| " ".to_owned(), |layout| layout.fill_char().to_string()),
            record_delimiters: existing.is_none_or(FixedWidthLayout::record_delimiters),
            treat_empty_as_absent: existing.is_none_or(FixedWidthLayout::treat_empty_as_absent),
        })
    }

    pub(crate) fn layout_for_schema(
        &self,
        schema: &SchemaNode,
    ) -> Result<FixedWidthLayout, String> {
        if schema != &self.schema {
            return Err(
                "schema changed while configuring fixed-width fields; start the layout again"
                    .into(),
            );
        }
        let fields = flat_scalar_fields(schema)?;
        if fields.len() != self.widths.len() {
            return Err("fixed-width field count no longer matches the boundary schema".into());
        }
        let widths = self
            .widths
            .iter()
            .zip(fields)
            .map(|(raw, field)| {
                raw.trim()
                    .parse::<u32>()
                    .ok()
                    .and_then(FixedFieldWidth::new)
                    .ok_or_else(|| format!("field `{}` needs a positive width", field.name))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut fill = self.fill.chars();
        let character = fill
            .next()
            .ok_or("fill must be exactly one Unicode character")?;
        if fill.next().is_some() {
            return Err("fill must be exactly one Unicode character".into());
        }
        FixedWidthLayout::new(
            widths,
            character,
            self.record_delimiters,
            self.treat_empty_as_absent,
        )
        .map_err(|error| error.to_string())
    }
}
