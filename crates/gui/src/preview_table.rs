//! Decoded output cells, preserving CSV lexical values (including empty
//! strings). Display limits never turn a retained prefix into a whole table.

use crate::run_report::MAX_PREVIEW_BYTES;

pub(crate) const MAX_COLUMNS: usize = 256;
pub(crate) const MAX_CELLS: usize = 65_536;
pub(crate) const MAX_ROWS: usize = 8192;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PreviewTable {
    pub(crate) headers: Vec<String>,
    pub(crate) rows: Vec<Vec<String>>,
    /// False when the names identify schema columns of a headerless artifact.
    pub(crate) has_headers: bool,
}

/// Only the dialect and the headerless writer's ordered field names are
/// needed after the project has changed.
#[derive(Debug)]
pub(crate) struct CsvLayout {
    delimiter: u8,
    quote: u8,
    quoting: bool,
    has_headers: bool,
    schema_names: Vec<String>,
}

impl CsvLayout {
    pub(crate) fn new(
        options: &mapping::FormatOptions,
        schema: &ir::SchemaNode,
    ) -> Result<Self, String> {
        let delimiter = options.delimiter.unwrap_or(',');
        let quote = options.csv_quote.unwrap_or('"');
        if !delimiter.is_ascii()
            || matches!(delimiter, '\r' | '\n' | '\0')
            || !quote.is_ascii_graphic()
            || (!options.csv_quote_disabled && quote == delimiter)
            || (options.csv_quote_disabled && options.csv_quote.is_some())
        {
            return Err("Invalid CSV delimiter or quoting configuration.".into());
        }
        let has_headers = options.has_header_row.unwrap_or(true);
        let mut schema_names = Vec::new();
        if !has_headers {
            let ir::SchemaKind::Group { children, .. } = &schema.kind else {
                return Err("Headerless CSV requires a row schema.".into());
            };
            if children.len() > MAX_COLUMNS
                || children.iter().map(|child| child.name.len()).sum::<usize>() > MAX_PREVIEW_BYTES
            {
                return Err("Headerless column names exceed the inspection limit.".into());
            }
            schema_names.extend(children.iter().map(|child| child.name.clone()));
        }
        Ok(Self {
            delimiter: delimiter as u8,
            quote: quote as u8,
            quoting: !options.csv_quote_disabled,
            has_headers,
            schema_names,
        })
    }

    pub(crate) fn decode(&self, bytes: &[u8]) -> Result<PreviewTable, String> {
        if bytes.len() > MAX_PREVIEW_BYTES {
            return Err("CSV exceeds the 1 MiB inspection limit. See the raw preview.".into());
        }
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .delimiter(self.delimiter)
            .quote(self.quote)
            .quoting(self.quoting)
            .from_reader(bytes);
        let mut table = PreviewTable {
            headers: self.schema_names.clone(),
            rows: Vec::new(),
            has_headers: self.has_headers,
        };
        let mut record = csv::StringRecord::new();
        let mut first = true;
        let mut cells = 0;
        while reader
            .read_record(&mut record)
            .map_err(|error| format!("CSV parse failed: {error}"))?
        {
            if record.len() > MAX_COLUMNS {
                return Err(format!(
                    "CSV exceeds the {MAX_COLUMNS}-column inspection limit."
                ));
            }
            cells += record.len();
            if first && self.has_headers {
                table.headers = record.iter().map(str::to_owned).collect();
            } else {
                if table.rows.len() == MAX_ROWS || cells > MAX_CELLS {
                    return Err(
                        "CSV exceeds the row or cell inspection limit. See the raw preview.".into(),
                    );
                }
                if record.len() != table.headers.len() {
                    return Err(format!(
                        "CSV data row {} has {} cells; expected {}.",
                        table.rows.len() + 1,
                        record.len(),
                        table.headers.len()
                    ));
                }
                table.rows.push(record.iter().map(str::to_owned).collect());
            }
            first = false;
        }
        Ok(table)
    }
}

impl PreviewTable {
    pub(crate) fn parse(
        bytes: &[u8],
        options: &mapping::FormatOptions,
        schema: &ir::SchemaNode,
    ) -> Result<Self, String> {
        CsvLayout::new(options, schema)?.decode(bytes)
    }

    pub(crate) fn column(&self, name: &str) -> Result<usize, String> {
        let mut matches = self
            .headers
            .iter()
            .enumerate()
            .filter(|(_, header)| *header == name);
        let Some((column, _)) = matches.next() else {
            return Err(format!("Missing column `{name}`."));
        };
        if matches.next().is_some() {
            return Err(format!(
                "Ambiguous column `{name}`: the header occurs more than once."
            ));
        }
        Ok(column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> ir::SchemaNode {
        ir::SchemaNode::group(
            "row",
            vec![
                ir::SchemaNode::scalar("id", ir::ScalarType::String),
                ir::SchemaNode::scalar("text", ir::ScalarType::String),
            ],
        )
    }

    #[test]
    fn reads_real_headers_dialect_bom_and_multiline_cells_without_converting_values() {
        let options = mapping::FormatOptions {
            delimiter: Some(';'),
            csv_quote: Some('\''),
            ..Default::default()
        };
        let table = PreviewTable::parse(
            b"\xef\xbb\xbf'ID;part';note\r\n001;'a;''b''\nc'\r\n002;\r\n",
            &options,
            &schema(),
        )
        .unwrap();
        assert_eq!(table.headers, ["ID;part", "note"]);
        assert_eq!(table.rows[0], ["001", "a;'b'\nc"]);
        assert_eq!(table.rows[1], ["002", ""]);
        assert!(table.has_headers);
    }

    #[test]
    fn headerless_empty_ambiguous_and_oversized_outputs_stay_distinct() {
        let options = mapping::FormatOptions {
            has_header_row: Some(false),
            csv_quote_disabled: true,
            ..Default::default()
        };
        let row = PreviewTable::parse(b"001,\"quoted\"\n", &options, &schema()).unwrap();
        assert_eq!(row.headers, ["id", "text"]);
        assert_eq!(row.rows[0], ["001", "\"quoted\""]);
        assert!(!row.has_headers);
        for input in [b"".as_slice(), b"id,text\n"] {
            assert!(
                PreviewTable::parse(input, &Default::default(), &schema())
                    .unwrap()
                    .rows
                    .is_empty()
            );
        }
        let duplicates =
            PreviewTable::parse(b"k,k\nx,y\n", &Default::default(), &schema()).unwrap();
        assert!(duplicates.column("k").is_err());
        assert!(PreviewTable::parse(b"x,y\n1\n", &Default::default(), &schema()).is_err());
        assert!(
            PreviewTable::parse(&vec![b'x'; MAX_PREVIEW_BYTES + 1], &options, &schema()).is_err()
        );
    }

    #[test]
    fn an_explicit_document_format_wins_over_a_csv_filename() {
        let options = mapping::FormatOptions {
            xml_document: true,
            ..Default::default()
        };
        let path = std::path::Path::new("logical.csv");
        assert!(!cli::preview_writes_csv(path, &options));
        assert!(cli::preview_reads_xml(path, &options));
        assert!(cli::preview_writes_csv(path, &Default::default()));
    }
}
