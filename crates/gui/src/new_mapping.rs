use std::path::PathBuf;

use anyhow::{Context as _, bail};
use ir::{ScalarType, SchemaNode};
use mapping::{FormatOptions, Graph, Project, Scope, TabularBoundaryKind};

mod fixed_width;
mod flextext;
mod format_options;
mod protobuf;
pub(super) use fixed_width::FixedWidthBoundaryDraft;
pub(super) use flextext::FlexTextBoundaryDraft;
pub(crate) use flextext::is_flextext_configuration;
pub(crate) use format_options::{
    can_update_existing_workbook, configured_layout_label_for_path, uses_csv_format,
    uses_path_format,
};
pub(super) use protobuf::ProtobufBoundaryDraft;
pub(crate) use protobuf::{
    is_protobuf_schema, show_protobuf_root_message, validate_schema_replacement,
};

#[derive(Default)]
pub(super) struct NewMappingSetup {
    pub(super) source: Option<MappingBoundary>,
    pub(super) target: Option<MappingBoundary>,
}

pub(super) struct ImportedSchema {
    pub(super) path: PathBuf,
    pub(super) schema: SchemaNode,
}

pub(super) enum MappingBoundary {
    Schema(Box<ImportedSchema>),
    FixedWidth(Box<FixedWidthBoundaryDraft>),
    Csv(CsvBoundaryDraft),
    Sqlite(Box<SqliteBoundaryDraft>),
    Protobuf(Box<ProtobufBoundaryDraft>),
    FlexText(Box<FlexTextBoundaryDraft>),
}

pub(super) struct SqliteBoundaryDraft {
    pub(super) schema_path: String,
    pub(super) output_path: String,
    pub(super) table: String,
    pub(super) schema: Option<SchemaNode>,
    pub(super) introspection_error: Option<String>,
}

impl SqliteBoundaryDraft {
    pub(super) fn source(path: PathBuf) -> anyhow::Result<Self> {
        Self::from_existing(path, false)
    }

    pub(super) fn target(path: PathBuf) -> anyhow::Result<Self> {
        Self::from_existing(path, true)
    }

    fn from_existing(path: PathBuf, target: bool) -> anyhow::Result<Self> {
        validate_sqlite_path(&path)?;
        let path = path
            .to_str()
            .context("SQLite path must be valid UTF-8")?
            .to_owned();
        Ok(Self {
            output_path: if target { path.clone() } else { String::new() },
            schema_path: path,
            table: String::new(),
            schema: None,
            introspection_error: None,
        })
    }

    pub(super) fn set_table(&mut self, table: String) {
        if self.table != table {
            self.table = table;
            self.schema = None;
            self.introspection_error = None;
        }
    }

    pub(super) fn load_schema(&mut self) -> anyhow::Result<()> {
        self.schema = None;
        let result = (|| {
            let path = std::path::Path::new(&self.schema_path);
            validate_sqlite_path(path)?;
            let table = self.table.trim();
            if table.is_empty() {
                bail!("SQLite table name is required");
            }
            if table.chars().count() > 256 {
                bail!("SQLite table name cannot exceed 256 characters");
            }
            let json = cli::import_db(path, table)?;
            let schema: SchemaNode = serde_json::from_str(&json)?;
            let ir::SchemaKind::Group { children, .. } = &schema.kind else {
                bail!("SQLite table must contain scalar columns");
            };
            if !schema.repeating {
                bail!("SQLite table schema must repeat for each row");
            }
            if children.len() > 256 {
                bail!("SQLite table has more than 256 columns");
            }
            Ok(schema)
        })();
        match result {
            Ok(schema) => {
                self.schema = Some(schema);
                self.introspection_error = None;
                Ok(())
            }
            Err(error) => {
                self.introspection_error = Some(error.to_string());
                Err(error)
            }
        }
    }

    pub(super) fn schema(&self, target: bool) -> anyhow::Result<SchemaNode> {
        validate_sqlite_path(std::path::Path::new(&self.schema_path))?;
        if self.table.trim().is_empty() {
            bail!("SQLite table name is required");
        }
        if target {
            let output = self.output_path.trim();
            if output.is_empty() {
                bail!("SQLite output path is required");
            }
            validate_sqlite_extension(std::path::Path::new(output))?;
        }
        if let Some(error) = &self.introspection_error {
            bail!("SQLite table could not be loaded: {error}");
        }
        self.schema.clone().context("load the SQLite table schema")
    }
}

fn validate_sqlite_extension(path: &std::path::Path) -> anyhow::Result<()> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    if !matches!(extension.as_deref(), Some("db" | "sqlite" | "sqlite3")) {
        bail!("SQLite path must have a .db, .sqlite, or .sqlite3 extension");
    }
    Ok(())
}

fn validate_sqlite_path(path: &std::path::Path) -> anyhow::Result<()> {
    validate_sqlite_extension(path)?;
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("SQLite database {} must already exist", path.display()))?;
    if !metadata.is_file() {
        bail!("SQLite database {} is not a file", path.display());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum CsvQuoteMode {
    #[default]
    Standard,
    Custom,
    Disabled,
}

pub(super) struct CsvBoundaryDraft {
    pub(super) path: String,
    pub(super) delimiter: char,
    pub(super) quote_mode: CsvQuoteMode,
    pub(super) custom_quote: String,
    pub(super) has_header_row: bool,
    pub(super) preserve_empty_strings: bool,
    pub(super) utf8_bom: bool,
    pub(super) columns: Vec<CsvColumnDraft>,
    pub(super) preview_rows: Vec<Vec<String>>,
    pub(super) sample_error: Option<String>,
}

pub(super) struct CsvColumnDraft {
    pub(super) name: String,
    pub(super) ty: ScalarType,
}

impl CsvBoundaryDraft {
    pub(super) fn source(path: PathBuf) -> anyhow::Result<Self> {
        let delimiter = if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("tsv"))
        {
            '\t'
        } else {
            ','
        };
        let path = path
            .to_str()
            .context("CSV source path must be valid UTF-8")?
            .to_owned();
        let mut draft = Self {
            path,
            delimiter,
            quote_mode: CsvQuoteMode::Standard,
            custom_quote: "'".to_owned(),
            has_header_row: true,
            preserve_empty_strings: false,
            utf8_bom: false,
            columns: Vec::new(),
            preview_rows: Vec::new(),
            sample_error: None,
        };
        if let Err(error) = draft.refresh_sample()
            && !matches!(
                error.downcast_ref::<cli::CsvFormatError>(),
                Some(cli::CsvFormatError::SampleTooWide | cli::CsvFormatError::SampleTooLarge)
            )
        {
            return Err(error);
        }
        Ok(draft)
    }

    pub(super) fn target() -> Self {
        Self {
            path: String::new(),
            delimiter: ',',
            quote_mode: CsvQuoteMode::Standard,
            custom_quote: "'".to_owned(),
            has_header_row: true,
            preserve_empty_strings: false,
            utf8_bom: false,
            columns: vec![CsvColumnDraft {
                name: String::new(),
                ty: ScalarType::String,
            }],
            preview_rows: Vec::new(),
            sample_error: None,
        }
    }

    pub(super) fn refresh_sample(&mut self) -> anyhow::Result<()> {
        let sample = match self.quote_settings().and_then(|(quote, quote_disabled)| {
            cli::sample_csv_with_dialect(
                std::path::Path::new(&self.path),
                Some(self.delimiter),
                quote,
                quote_disabled,
                self.has_header_row,
            )
            .map_err(anyhow::Error::from)
        }) {
            Ok(sample) => sample,
            Err(error) => {
                self.sample_error = Some(error.to_string());
                return Err(error);
            }
        };
        let previous_types = self
            .columns
            .iter()
            .map(|column| column.ty)
            .collect::<Vec<_>>();
        self.columns = sample
            .columns
            .into_iter()
            .enumerate()
            .map(|(index, name)| CsvColumnDraft {
                name,
                ty: previous_types
                    .get(index)
                    .copied()
                    .unwrap_or(ScalarType::String),
            })
            .collect();
        self.preview_rows = sample.rows;
        self.sample_error = None;
        Ok(())
    }

    pub(super) fn validate(&self) -> anyhow::Result<()> {
        if let Some(error) = &self.sample_error {
            bail!("CSV sample could not be read: {error}");
        }
        if self.path.trim().is_empty() {
            bail!("CSV path is required");
        }
        if let Some(extension) = std::path::Path::new(self.path.trim())
            .extension()
            .and_then(|extension| extension.to_str())
            && !matches!(
                extension.to_ascii_lowercase().as_str(),
                "csv" | "txt" | "tsv"
            )
        {
            bail!("CSV path must have a .csv, .txt, or .tsv extension");
        }
        self.quote_settings()?;
        if self.columns.is_empty() {
            bail!("CSV must have at least one column");
        }
        let mut names = std::collections::BTreeSet::new();
        for column in &self.columns {
            if column.name.trim().is_empty() {
                bail!("CSV column names cannot be empty");
            }
            if !names.insert(column.name.as_str()) {
                bail!("duplicate CSV column name `{}`", column.name);
            }
        }
        Ok(())
    }

    fn schema(&self) -> anyhow::Result<SchemaNode> {
        self.validate()?;
        Ok(SchemaNode::group(
            "row",
            self.columns
                .iter()
                .map(|column| SchemaNode::scalar(&column.name, column.ty))
                .collect(),
        ))
    }

    fn quote_settings(&self) -> anyhow::Result<(Option<char>, bool)> {
        if !self.delimiter.is_ascii() || matches!(self.delimiter, '\0' | '\r' | '\n') {
            bail!("CSV delimiter must be a single ASCII character other than NUL or a line break");
        }
        let quote = match self.quote_mode {
            CsvQuoteMode::Standard => '"',
            CsvQuoteMode::Custom => {
                let mut characters = self.custom_quote.chars();
                let Some(quote) = characters.next() else {
                    bail!("CSV quote must be exactly one printable ASCII character");
                };
                if characters.next().is_some() || !quote.is_ascii_graphic() {
                    bail!("CSV quote must be exactly one printable ASCII character");
                }
                quote
            }
            CsvQuoteMode::Disabled => return Ok((None, true)),
        };
        if quote == self.delimiter {
            bail!("CSV quote and delimiter must be different characters");
        }
        Ok((
            (self.quote_mode == CsvQuoteMode::Custom).then_some(quote),
            false,
        ))
    }

    fn options(&self) -> anyhow::Result<FormatOptions> {
        let (quote, quote_disabled) = self.quote_settings()?;
        Ok(FormatOptions {
            tabular_kind: Some(TabularBoundaryKind::Csv),
            delimiter: Some(self.delimiter),
            csv_quote: quote,
            csv_quote_disabled: quote_disabled,
            has_header_row: Some(self.has_header_row),
            csv_preserve_empty_strings: self.preserve_empty_strings,
            csv_utf8_bom: self.utf8_bom,
            ..FormatOptions::default()
        })
    }
}

#[cfg(test)]
#[path = "new_mapping/csv_quote_tests.rs"]
mod csv_quote_tests;

impl NewMappingSetup {
    pub(super) fn can_create(&self) -> bool {
        let ready = |boundary: &MappingBoundary, target| match boundary {
            MappingBoundary::Schema(_) => true,
            MappingBoundary::FixedWidth(draft) => draft.validate().is_ok(),
            MappingBoundary::Csv(draft) => draft.validate().is_ok(),
            MappingBoundary::Sqlite(draft) => draft.schema(target).is_ok(),
            MappingBoundary::Protobuf(draft) => draft.validate().is_ok(),
            MappingBoundary::FlexText(draft) => draft.validate().is_ok(),
        };
        self.source
            .as_ref()
            .is_some_and(|source| ready(source, false))
            && self
                .target
                .as_ref()
                .is_some_and(|target| ready(target, true))
    }

    pub(super) fn build_project(&self) -> anyhow::Result<Project> {
        let source = self.source.as_ref().context("choose a source boundary")?;
        let target = self.target.as_ref().context("choose a target boundary")?;
        let mut project = blank_project();
        match source {
            MappingBoundary::Schema(imported) => project.source = imported.schema.clone(),
            MappingBoundary::FixedWidth(draft) => {
                project.source = draft.schema.clone();
                project.source_options = draft.options()?;
                project.source_path = draft.instance_path();
            }
            MappingBoundary::Csv(draft) => {
                project.source = draft.schema()?;
                project.source_options = draft.options()?;
                project.source_path = Some(draft.path.clone());
            }
            MappingBoundary::Sqlite(draft) => {
                project.source = draft.schema(false)?;
                project.source_path = Some(draft.schema_path.clone());
            }
            MappingBoundary::Protobuf(draft) => {
                project.source = draft.schema()?;
                project.source_options = draft.options()?;
                project.source_path = draft.instance_path();
            }
            MappingBoundary::FlexText(draft) => {
                project.source = draft.schema()?;
                project.source_options = draft.options()?;
                project.source_path = draft.instance_path();
            }
        }
        match target {
            MappingBoundary::Schema(imported) => project.target = imported.schema.clone(),
            MappingBoundary::FixedWidth(draft) => {
                project.target = draft.schema.clone();
                project.target_options = draft.options()?;
                project.target_path = draft.instance_path();
            }
            MappingBoundary::Csv(draft) => {
                project.target = draft.schema()?;
                project.target_options = draft.options()?;
                project.target_path = Some(draft.path.clone());
            }
            MappingBoundary::Sqlite(draft) => {
                project.target = draft.schema(true)?;
                project.target_path = Some(draft.output_path.trim().to_owned());
            }
            MappingBoundary::Protobuf(draft) => {
                project.target = draft.schema()?;
                project.target_options = draft.options()?;
                project.target_path = draft.instance_path();
            }
            MappingBoundary::FlexText(draft) => {
                project.target = draft.schema()?;
                project.target_options = draft.options()?;
                project.target_path = draft.instance_path();
            }
        }
        Ok(project)
    }
}

#[derive(Clone, Copy)]
pub(super) enum SchemaSide {
    Source,
    Target,
}

impl SchemaSide {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Source => "Source",
            Self::Target => "Target",
        }
    }
}

pub(super) fn import_schema(path: &std::path::Path) -> anyhow::Result<SchemaNode> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    let json = match extension.as_deref() {
        Some("xsd") => cli::import_xsd(path)?,
        Some("json") => cli::import_json_schema(path)?,
        _ => anyhow::bail!("schema must be an XSD or JSON Schema file"),
    };
    Ok(serde_json::from_str(&json)?)
}

pub(super) fn blank_project() -> Project {
    Project {
        source: SchemaNode::group("root", vec![]),
        target: SchemaNode::group("root", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_file(tag: &str, extension: &str, content: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-new-mapping-{tag}-{}.{}",
            std::process::id(),
            extension
        ));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn csv_draft_requires_unique_names_and_retains_explicit_types() {
        let path = source_file("duplicate", "csv", "name,name\nJane,Doe\n");
        let mut source = CsvBoundaryDraft::source(path.clone()).unwrap();
        assert_eq!(source.columns.len(), 2);
        assert_eq!(source.preview_rows[0], ["Jane", "Doe"]);
        assert!(
            source
                .validate()
                .unwrap_err()
                .to_string()
                .contains("duplicate")
        );
        source.columns[1].name = "surname".into();
        source.columns[1].ty = ScalarType::Bool;
        let schema = source.schema().unwrap();
        assert_eq!(
            schema,
            SchemaNode::group(
                "row",
                vec![
                    SchemaNode::scalar("name", ScalarType::String),
                    SchemaNode::scalar("surname", ScalarType::Bool),
                ]
            )
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn csv_source_empty_text_and_target_bom_choices_survive_project_save() {
        let path = source_file("empty-text", "csv", "A,B\n,beta\n");
        let mut source = CsvBoundaryDraft::source(path.clone()).unwrap();
        assert!(!source.preserve_empty_strings);
        source.preserve_empty_strings = true;
        let mut target = CsvBoundaryDraft::target();
        target.path = "output.csv".into();
        target.utf8_bom = true;
        target.columns = vec![
            CsvColumnDraft {
                name: "A".into(),
                ty: ScalarType::String,
            },
            CsvColumnDraft {
                name: "B".into(),
                ty: ScalarType::String,
            },
        ];
        let project = NewMappingSetup {
            source: Some(MappingBoundary::Csv(source)),
            target: Some(MappingBoundary::Csv(target)),
        }
        .build_project()
        .unwrap();
        let reopened = mapping::project_file::decode_str(
            &mapping::project_file::encode_pretty(&project).unwrap(),
        )
        .unwrap();
        assert!(reopened.source_options.csv_preserve_empty_strings);
        assert!(!reopened.target_options.csv_preserve_empty_strings);
        assert!(!reopened.source_options.csv_utf8_bom);
        assert!(reopened.target_options.csv_utf8_bom);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn tsv_source_defaults_to_tab_and_can_switch_header_mode() {
        let path = source_file("tab", "tsv", "name\tage\nJane\t29\n");
        let mut source = CsvBoundaryDraft::source(path.clone()).unwrap();
        assert_eq!(source.delimiter, '\t');
        assert_eq!(
            source
                .columns
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            ["name", "age"]
        );
        source.has_header_row = false;
        source.refresh_sample().unwrap();
        assert_eq!(
            source
                .columns
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            ["Column 1", "Column 2"]
        );
        assert_eq!(source.preview_rows.len(), 2);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn sqlite_source_does_not_create_a_missing_database() {
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-missing-sqlite-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        assert!(SqliteBoundaryDraft::source(path.clone()).is_err());
        assert!(!path.exists());
    }
}
