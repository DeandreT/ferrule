use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use ir::{Instance, SchemaNode};
use mapping::{EdiAutocomplete, EdiBoundaryKind, FormatOptions};

use super::super as cli;
use super::super::{
    extension_for_dispatch, extension_of, formatted_edi_output, has_legacy_xlsx_layout,
    json5_selected, protobuf_layout, reject_edi_conflicts, reject_fixed_width_csv_options,
    reject_flextext_conflicts, reject_idoc_conflicts, reject_json_conflicts, reject_pdf_conflicts,
    reject_protobuf_conflicts, reject_swift_conflicts, reject_xbrl_conflicts, reject_xml_conflicts,
    validate_tabular_fallback, x12_separators,
};
use super::{
    MAX_PAYLOAD_DOCUMENT_BYTES, MAX_PAYLOAD_RUN_BYTES, PayloadArtifact, PayloadDestination,
    visit_target_documents,
};

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub(super) document: usize,
    pub(super) run: usize,
}

impl Limits {
    pub(super) const PRODUCTION: Self = Self {
        document: MAX_PAYLOAD_DOCUMENT_BYTES,
        run: MAX_PAYLOAD_RUN_BYTES,
    };
}

enum PayloadBytes {
    Ready(Vec<u8>, usize),
    CsvOversize {
        maximum: usize,
        records_written: usize,
    },
}

enum ArtifactBytes {
    Ready(Vec<u8>),
    CsvOversize { maximum: usize },
}

pub(super) struct RenderedArtifact {
    target: String,
    path: PathBuf,
    pub(super) records_written: usize,
    bytes: ArtifactBytes,
}

// Preview retains its unbounded serializer and its own incremental budget order.
pub(crate) fn render_payload(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    current_datetime: &str,
) -> anyhow::Result<(Vec<u8>, usize)> {
    match render_payload_with_limit(path, schema, instance, options, current_datetime, None)? {
        PayloadBytes::Ready(bytes, records) => Ok((bytes, records)),
        PayloadBytes::CsvOversize { .. } => unreachable!("unbounded CSV has no byte ceiling"),
    }
}

pub(super) fn render_target(
    name: &str,
    destination: &PayloadDestination,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    current_datetime: &str,
    limits: Limits,
) -> anyhow::Result<Vec<RenderedArtifact>> {
    let mut artifacts = Vec::new();
    visit_target_documents(destination, instance, |path, instance| {
        let rendered = render_payload_with_limit(
            &path,
            schema,
            instance,
            options,
            current_datetime,
            Some(limits.document),
        )
        .with_context(|| format!("rendering target payload {}", path.display()))?;
        let (bytes, records_written) = match rendered {
            PayloadBytes::Ready(bytes, records_written) => {
                (ArtifactBytes::Ready(bytes), records_written)
            }
            PayloadBytes::CsvOversize {
                maximum,
                records_written,
            } => (ArtifactBytes::CsvOversize { maximum }, records_written),
        };
        artifacts.push(RenderedArtifact {
            target: name.to_string(),
            path,
            records_written,
            bytes,
        });
        Ok(())
    })?;
    Ok(artifacts)
}

pub(super) fn finalize(
    artifacts: Vec<RenderedArtifact>,
    limits: Limits,
) -> anyhow::Result<Vec<PayloadArtifact>> {
    let mut total = 0usize;
    for artifact in &artifacts {
        match &artifact.bytes {
            ArtifactBytes::Ready(bytes) => {
                if bytes.len() > limits.document {
                    bail!(
                        "output artifact `{}` exceeds the {} MiB per-document limit",
                        artifact.path.display(),
                        limits.document / (1024 * 1024)
                    );
                }
                total = total
                    .checked_add(bytes.len())
                    .context("payload output byte count overflowed")?;
            }
            ArtifactBytes::CsvOversize { maximum } => {
                bail!(
                    "output artifact `{}` exceeds the {} MiB per-document limit",
                    artifact.path.display(),
                    maximum / (1024 * 1024)
                );
            }
        }
    }
    if total > limits.run {
        bail!(
            "payload outputs exceed the {} MiB total limit",
            limits.run / (1024 * 1024)
        );
    }
    let artifacts = artifacts
        .into_iter()
        .map(|artifact| {
            let bytes = match artifact.bytes {
                ArtifactBytes::Ready(bytes) => bytes,
                ArtifactBytes::CsvOversize { .. } => unreachable!("oversized CSV refused above"),
            };
            PayloadArtifact {
                target: artifact.target,
                path: artifact.path,
                records_written: artifact.records_written,
                bytes,
            }
        })
        .collect::<Vec<_>>();
    super::validate_artifact_paths(&artifacts)?;
    Ok(artifacts)
}

fn render_payload_with_limit(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    current_datetime: &str,
    csv_maximum: Option<usize>,
) -> anyhow::Result<PayloadBytes> {
    options
        .validate_xml_schema_hint_options(true)
        .map_err(anyhow::Error::msg)?;
    cli::validate_csv_metadata_identity(path, options, "output")?;
    cli::reject_inactive_root_xml_read_options(path, schema, options, "output")?;
    if options.local_xml_file_set {
        bail!("`local_xml_file_set` is input-only");
    }
    if options.xbrl.is_some() {
        reject_xbrl_conflicts(options, "output")?;
        let xbrl = options
            .xbrl
            .as_ref()
            .context("missing XBRL target options")?;
        let text = format_xbrl::to_string(schema, instance, xbrl)
            .context("rendering XBRL output payload")?;
        return Ok(PayloadBytes::Ready(text.into_bytes(), 1));
    }
    if let Some(layout) = &options.idoc {
        reject_idoc_conflicts(options, "output")?;
        let formatted = formatted_edi_output(instance, options)?;
        let bytes = format_edi::idoc::to_bytes(schema, &formatted, layout)
            .context("rendering SAP IDoc output payload")?;
        return Ok(PayloadBytes::Ready(bytes, 1));
    }
    if options.swift_mt.is_some() {
        reject_swift_conflicts(options, "output")?;
        bail!("SWIFT MT output is not supported; `swift_mt` is input-only");
    }
    if options.pdf.is_some() {
        reject_pdf_conflicts(options, "output")?;
        bail!("PDF output is not supported; `pdf` is input-only");
    }
    if let Some(layout) = &options.flextext {
        reject_flextext_conflicts(options, "output")?;
        let text = format_flextext::to_string(schema, instance, layout)
            .context("rendering FlexText output payload")?;
        return Ok(PayloadBytes::Ready(text.into_bytes(), 1));
    }
    if let Some(protobuf) = &options.protobuf {
        reject_protobuf_conflicts(options, "output")?;
        let layout =
            protobuf_layout(protobuf).context("parsing embedded Protocol Buffers schema")?;
        let bytes = format_protobuf::to_vec(&layout, &protobuf.root_message, instance)
            .context("rendering Protocol Buffers output payload")?;
        return Ok(PayloadBytes::Ready(bytes, 1));
    }
    if let Some(kind) = options.edi_kind {
        reject_edi_conflicts(options, "output")?;
        let formatted = formatted_edi_output(instance, options)?;
        let text = render_edi_payload(schema, &formatted, options, kind, current_datetime)?;
        return Ok(PayloadBytes::Ready(text.into_bytes(), 1));
    }
    if options.xml_document {
        reject_xml_conflicts(options, "output")?;
        return format_xml::to_string_with_options(
            schema,
            instance,
            &cli::xml_write_options(options),
        )
        .map(|text| PayloadBytes::Ready(text.into_bytes(), 1))
        .context("rendering XML output payload");
    }
    if options.json_document || options.json5 || options.json_lines {
        reject_json_conflicts(options, "output")?;
        let json5 = json5_selected(path, options)?;
        let text = if options.json_lines {
            format_json::to_lines(schema, instance)
        } else if json5 {
            format_json::to_json5_string(schema, instance)
        } else {
            format_json::to_string(schema, instance)
        }
        .context("rendering JSON output payload")?;
        return Ok(PayloadBytes::Ready(
            text.into_bytes(),
            instance.as_repeated().map_or(1, <[Instance]>::len),
        ));
    }
    if let Some(layout) = &options.fixed_width {
        reject_fixed_width_csv_options(options, "output")?;
        let rows = instance
            .as_repeated()
            .context("mapping did not produce a repeating row set for a fixed-width output")?;
        let text = format_csv::to_string_fixed_width(schema, rows, layout)
            .context("rendering fixed-width output payload")?;
        return Ok(PayloadBytes::Ready(text.into_bytes(), rows.len()));
    }

    validate_tabular_fallback(path, options, "output")?;
    match extension_for_dispatch(path, options)?.as_str() {
        "csv" | "txt" => {
            let rows = instance
                .as_repeated()
                .context("mapping did not produce a repeating row set for a CSV output")?;
            if let Some(maximum) = csv_maximum {
                return match format_csv::to_bytes_with_options_bounded(
                    schema,
                    rows,
                    &format_csv::CsvWriteOptions::from(options),
                    maximum,
                ) {
                    Ok(bytes) => Ok(PayloadBytes::Ready(bytes, rows.len())),
                    Err(format_csv::CsvBoundedError::Format(error)) => {
                        Err(anyhow::Error::new(error)).context("rendering CSV output payload")
                    }
                    Err(format_csv::CsvBoundedError::OutputTooLarge { maximum }) => {
                        Ok(PayloadBytes::CsvOversize {
                            maximum,
                            records_written: rows.len(),
                        })
                    }
                };
            }
            let text = format_csv::to_string_with_options(
                schema,
                rows,
                &format_csv::CsvWriteOptions::from(options),
            )
            .context("rendering CSV output payload")?;
            Ok(PayloadBytes::Ready(text.into_bytes(), rows.len()))
        }
        "xlsx" => render_xlsx_payload(schema, instance, options)
            .map(|(bytes, records)| PayloadBytes::Ready(bytes, records)),
        "xml" => {
            format_xml::to_string_with_options(schema, instance, &cli::xml_write_options(options))
                .map(|text| PayloadBytes::Ready(text.into_bytes(), 1))
                .context("rendering XML output payload")
        }
        "json" | "json5" | "jsonl" | "ndjson" => {
            let lines =
                options.json_lines || matches!(extension_of(path)?.as_str(), "jsonl" | "ndjson");
            let json5 = json5_selected(path, options)?;
            if lines && json5 {
                bail!("JSON5 cannot be combined with JSON Lines");
            }
            let text = if lines {
                format_json::to_lines(schema, instance)
            } else if json5 {
                format_json::to_json5_string(schema, instance)
            } else {
                format_json::to_string(schema, instance)
            }
            .context("rendering JSON output payload")?;
            Ok(PayloadBytes::Ready(
                text.into_bytes(),
                instance.as_repeated().map_or(1, <[Instance]>::len),
            ))
        }
        "db" | "sqlite" | "sqlite3" => {
            bail!("SQLite output requires a persistent database and is unavailable as a payload")
        }
        "edi" | "x12" | "edifact" | "hl7" => {
            let formatted = formatted_edi_output(instance, options)?;
            let text = match format_edi::dialect_of(schema)? {
                format_edi::Dialect::X12 => render_edi_payload(
                    schema,
                    &formatted,
                    options,
                    EdiBoundaryKind::X12,
                    current_datetime,
                ),
                format_edi::Dialect::Edifact => render_edi_payload(
                    schema,
                    &formatted,
                    options,
                    EdiBoundaryKind::Edifact,
                    current_datetime,
                ),
                format_edi::Dialect::Hl7 => {
                    format_edi::hl7::to_string(schema, &formatted).map_err(anyhow::Error::new)
                }
                format_edi::Dialect::Tradacoms => {
                    format_edi::tradacoms::to_string(schema, &formatted).map_err(anyhow::Error::new)
                }
            }?;
            Ok(PayloadBytes::Ready(text.into_bytes(), 1))
        }
        "pdf" => bail!("PDF output is not supported; PDF is input-only"),
        other => bail!("unsupported output payload extension: .{other}"),
    }
}

fn render_xlsx_payload(
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
) -> anyhow::Result<(Vec<u8>, usize)> {
    if options.xlsx_update_existing {
        bail!(
            "update-existing XLSX output requires a persistent workbook and is unavailable as a payload"
        );
    }
    if let Some(layout) = &options.xlsx_hierarchical {
        if options.xlsx_grid.is_some()
            || options.xlsx_composite.is_some()
            || options.xlsx_worksheet_set.is_some()
            || has_legacy_xlsx_layout(options)
        {
            bail!("`xlsx_hierarchical` cannot be combined with other XLSX layout options");
        }
        let (bytes, worksheets) = format_xlsx::to_bytes_hierarchical(schema, instance, layout)
            .context("rendering hierarchical XLSX output payload")?;
        return Ok((bytes, worksheets));
    }
    if options.xlsx_grid.is_some() {
        bail!("grid XLSX output is not supported; `xlsx_grid` is input-only");
    }
    if options.xlsx_worksheet_set.is_some() {
        bail!("worksheet-set XLSX output is not supported; `xlsx_worksheet_set` is input-only");
    }
    if options.xlsx_composite.is_some() {
        bail!("composite XLSX output is not supported; `xlsx_composite` is input-only");
    }
    if !options.xlsx_rows.is_empty() {
        bail!("transposed XLSX output is not supported; `xlsx_rows` is input-only");
    }
    let rows = instance
        .as_repeated()
        .context("mapping did not produce a repeating row set for an XLSX output")?;
    let bytes = format_xlsx::to_bytes_with_options(
        schema,
        rows,
        format_xlsx::FlatTableWriteOptions {
            sheet: options.xlsx_sheet.as_deref(),
            start_row: options.xlsx_start_row.unwrap_or(1),
            columns: &options.xlsx_columns,
            headers: &options.xlsx_headers,
            has_header: options.has_header_row.unwrap_or(true),
        },
    )
    .context("rendering XLSX output payload")?;
    Ok((bytes, rows.len()))
}

fn render_edi_payload(
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
    kind: EdiBoundaryKind,
    current_datetime: &str,
) -> anyhow::Result<String> {
    match kind {
        EdiBoundaryKind::X12 => {
            let separators = options
                .x12_separators
                .map(x12_separators)
                .unwrap_or_default();
            let version = options.x12_interchange_version.as_deref();
            match options.edi_autocomplete.as_ref() {
                Some(EdiAutocomplete::X12(config)) => {
                    format_edi::x12::to_string_with_syntax_and_autocomplete(
                        schema,
                        instance,
                        separators,
                        version,
                        format_edi::x12::Autocomplete {
                            current_datetime,
                            request_acknowledgement: config.request_acknowledgement,
                            transaction_set: config.transaction_set.as_deref(),
                        },
                    )
                }
                _ => format_edi::x12::to_string_with_syntax(schema, instance, separators, version),
            }
        }
        EdiBoundaryKind::Edifact => {
            if let Some(EdiAutocomplete::Edifact(config)) = options.edi_autocomplete.as_ref() {
                format_edi::edifact::to_string_with_autocomplete(
                    schema,
                    instance,
                    format_edi::edifact::Autocomplete {
                        current_datetime,
                        syntax_level: config.syntax_level.as_deref(),
                        syntax_version: config.syntax_version.as_deref(),
                        controlling_agency: config.controlling_agency.as_deref(),
                        message_type: config.message_type.as_deref(),
                    },
                )
            } else {
                format_edi::edifact::to_string(schema, instance)
            }
        }
        EdiBoundaryKind::Hl7 => format_edi::hl7::to_string(schema, instance),
        EdiBoundaryKind::Tradacoms => format_edi::tradacoms::to_string(schema, instance),
        EdiBoundaryKind::Idoc => {
            bail!("SAP IDoc output requires an embedded runtime layout")
        }
        EdiBoundaryKind::SwiftMt => {
            bail!("SWIFT MT output is not supported; SWIFT MT is input-only")
        }
    }
    .context("rendering EDI output payload")
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use ir::{ScalarType, Value, XmlNil};
    use mapping::{Binding, Graph, NamedTarget, Node, Project, Scope, ScopeIteration};

    use super::*;

    struct Directory(PathBuf, Cell<bool>);

    impl Directory {
        fn new() -> anyhow::Result<Self> {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "ferrule-deferred-csv-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path)?;
            Ok(Self(path, Cell::new(false)))
        }

        fn error(&self, error: &anyhow::Error) -> anyhow::Result<()> {
            std::fs::write(self.0.join("error-debug.txt"), format!("{error:?}"))?;
            std::fs::write(
                self.0.join("error-chain.json"),
                serde_json::to_vec_pretty(
                    &error.chain().map(ToString::to_string).collect::<Vec<_>>(),
                )?,
            )?;
            Ok(())
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            if !self.1.get()
                || std::thread::panicking()
                || std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
                    == Some(std::ffi::OsStr::new("1"))
            {
                eprintln!("retained deferred CSV originals: {}", self.0.display());
            } else {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    fn csv_options() -> FormatOptions {
        FormatOptions {
            has_header_row: Some(false),
            ..FormatOptions::default()
        }
    }

    fn scope(node: u32) -> Scope {
        Scope {
            iteration: ScopeIteration::Source(vec!["Rows".into()]),
            bindings: vec![Binding {
                target_field: "Value".into(),
                node,
            }],
            ..Scope::default()
        }
    }

    fn project(first: &str, second: &str) -> Project {
        let schema =
            SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)]);
        Project {
            source: SchemaNode::group(
                "Input",
                vec![
                    SchemaNode::group("Rows", vec![SchemaNode::scalar("File", ScalarType::String)])
                        .repeating(),
                ],
            ),
            target: schema.clone(),
            source_path: None,
            target_path: Some("first.csv".into()),
            source_options: FormatOptions {
                json_document: true,
                ..FormatOptions::default()
            },
            target_options: csv_options(),
            extra_sources: Vec::new(),
            extra_targets: vec![NamedTarget {
                name: "later".into(),
                path: Some("later.csv".into()),
                schema,
                options: csv_options(),
                root: scope(1),
            }],
            failure_rules: Vec::new(),
            user_functions: Default::default(),
            graph: Graph {
                nodes: BTreeMap::from([
                    (
                        0,
                        Node::Const {
                            value: Value::String(first.into()),
                        },
                    ),
                    (
                        1,
                        Node::Const {
                            value: Value::String(second.into()),
                        },
                    ),
                    (
                        2,
                        Node::SourceField {
                            frame: Some(vec!["Rows".into()]),
                            path: vec!["File".into()],
                        },
                    ),
                ]),
            },
            root: scope(0),
        }
    }

    fn source(files: &[&str]) -> Instance {
        Instance::Group(
            vec![(
                "Rows".into(),
                Instance::Repeated(
                    files
                        .iter()
                        .map(|file| {
                            Instance::Group(
                                vec![(
                                    "File".into(),
                                    Instance::Scalar(Value::String((*file).into())),
                                )]
                                .into(),
                            )
                        })
                        .collect(),
                ),
            )]
            .into(),
        )
    }

    fn run(
        directory: &Directory,
        project: &Project,
        files: &[&str],
        limits: Limits,
    ) -> anyhow::Result<(usize, Vec<PayloadArtifact>)> {
        let input = source(files);
        std::fs::write(
            directory.0.join("project.json"),
            mapping::project_file::encode_pretty(project)?,
        )?;
        std::fs::write(
            directory.0.join("input.json"),
            serde_json::to_vec_pretty(&input)?,
        )?;
        let validation = cli::require_valid(project);
        std::fs::write(
            directory.0.join("validation.txt"),
            format!("{validation:?}"),
        )?;
        validation?;
        let output = engine::run_outputs(project, &input);
        std::fs::write(directory.0.join("engine-result.txt"), format!("{output:?}"))?;
        let output = output?;
        let rendered = super::super::render_all_targets(
            project,
            &directory.0.join("project.json"),
            None,
            &output,
            "2026-01-02T03:04:05Z",
            limits,
        );
        if let Err(error) = &rendered {
            directory.error(error)?;
        }
        let (records, artifacts) = rendered?;
        let mut metadata = Vec::new();
        for (index, artifact) in artifacts.iter().enumerate() {
            let body = match &artifact.bytes {
                ArtifactBytes::Ready(bytes) => {
                    std::fs::write(directory.0.join(format!("slot-{index}.bin")), bytes)?;
                    serde_json::json!({ "bytes": bytes.len() })
                }
                ArtifactBytes::CsvOversize { maximum } => serde_json::json!({
                    "genuine_csv_refusal_maximum": maximum,
                }),
            };
            metadata.push(serde_json::json!({
                "target": artifact.target, "path": artifact.path,
                "records": artifact.records_written, "body": body,
            }));
        }
        std::fs::write(
            directory.0.join("rendered-slots.json"),
            serde_json::to_vec_pretty(&metadata)?,
        )?;
        let result = finalize(artifacts, limits);
        match &result {
            Ok(artifacts) => {
                for (index, artifact) in artifacts.iter().enumerate() {
                    std::fs::write(
                        directory.0.join(format!("returned-{index}.bin")),
                        &artifact.bytes,
                    )?;
                }
                std::fs::write(directory.0.join("outcome.json"), serde_json::to_vec_pretty(
                    &artifacts.iter().map(|artifact| serde_json::json!({
                        "target": artifact.target, "path": artifact.path,
                        "bytes": artifact.bytes.len(), "records": artifact.records_written,
                    })).collect::<Vec<_>>()
                )?)?;
            }
            Err(error) => directory.error(error)?,
        }
        result.map(|artifacts| (records, artifacts))
    }

    const SMALL: Limits = Limits {
        document: 16,
        run: 24,
    };

    #[test]
    fn later_csv_format_causes_precede_earlier_genuine_oversize() -> anyhow::Result<()> {
        for nil in [false, true] {
            let directory = Directory::new()?;
            let mut project = project(&"x".repeat(16), "a,b");
            if nil {
                project.graph.nodes.insert(
                    1,
                    Node::Const {
                        value: Value::XmlNil(XmlNil),
                    },
                );
            } else {
                project.extra_targets[0].options.csv_quote_disabled = true;
            }
            let error = run(&directory, &project, &["a.json"], SMALL).unwrap_err();
            let cause = error.downcast_ref::<format_csv::CsvFormatError>();
            if nil {
                assert!(matches!(cause, Some(format_csv::CsvFormatError::ValueType {
                    row: 0, field, expected: ScalarType::String, got: "xml nil",
                }) if field == "Value"));
            } else {
                assert!(
                    matches!(cause, Some(format_csv::CsvFormatError::UnquotedFieldBoundary {
                    row: 0, field,
                }) if field == "Value")
                );
            }
            assert!(
                error
                    .downcast_ref::<format_csv::CsvBoundedError>()
                    .is_none()
            );
            assert_eq!(
                error
                    .chain()
                    .take(3)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                vec![
                    "rendering extra target `later`".to_string(),
                    format!(
                        "rendering target payload {}",
                        directory.0.join("later.csv").display()
                    ),
                    "rendering CSV output payload".to_string(),
                ]
            );
            directory.1.set(true);
        }
        Ok(())
    }

    #[test]
    fn later_format_and_destination_errors_precede_earlier_oversize() -> anyhow::Result<()> {
        for case in 0..3 {
            let directory = Directory::new()?;
            let mut project = project(&"x".repeat(16), "ok");
            project.extra_targets[0].path = match case {
                0 => Some("later.unsupported".into()),
                1 => None,
                _ => Some(format!(
                    "{}.csv",
                    "x".repeat(super::super::MAX_PAYLOAD_PATH_BYTES)
                )),
            };
            let error = run(&directory, &project, &["a.json"], SMALL).unwrap_err();
            let text = format!("{error:#}");
            match case {
                0 => assert!(
                    text.contains("unsupported output payload extension: .unsupported"),
                    "{text}"
                ),
                1 => assert!(
                    text.contains("resolving extra target `later` payload path"),
                    "{text}"
                ),
                _ => assert!(text.contains("4096-byte UTF-8 limit"), "{text}"),
            }
            assert!(!text.contains("per-document limit"), "{text}");
            directory.1.set(true);
        }
        Ok(())
    }

    #[test]
    fn per_slot_document_then_total_then_global_paths_keep_their_order() -> anyhow::Result<()> {
        for (first, second, run_limit, expected) in [
            ("xxxxxxxxxxxxxxxx", "ok", 1, "per-document limit"),
            ("xxxxxxxxxxx", "yyyyyyyyyyy", 20, "total limit"),
            ("x", "y", 24, "multiple output artifacts use path"),
        ] {
            let directory = Directory::new()?;
            let mut project = project(first, second);
            project.extra_targets[0].path = project.target_path.clone();
            let error = run(
                &directory,
                &project,
                &["a.json"],
                Limits {
                    document: 16,
                    run: run_limit,
                },
            )
            .unwrap_err();
            assert!(error.to_string().contains(expected), "{error:#}");
            assert_eq!(
                error.chain().count(),
                1,
                "final budget/path failures have no render contexts"
            );
            directory.1.set(true);
        }
        let directory = Directory::new()?;
        let mut overlapping = project("x", "y");
        overlapping.extra_targets[0].path = Some("first.csv/child.csv".into());
        let error = run(&directory, &overlapping, &["a.json"], SMALL).unwrap_err();
        assert!(
            error.to_string().contains("overlap as file and directory"),
            "{error:#}"
        );
        assert_eq!(error.chain().count(), 1);
        directory.1.set(true);
        let directory = Directory::new()?;
        let mut project = project(&"x".repeat(16), &"y".repeat(16));
        project.target_options = FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        };
        project.target_path = Some("first.json".into());
        let error = run(&directory, &project, &["a.json"], SMALL).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "output artifact `{}` exceeds the 0 MiB per-document limit",
                directory.0.join("first.json").display(),
            )
        );
        directory.1.set(true);
        Ok(())
    }

    #[test]
    fn whole_dynamic_member_checks_and_joined_length_precede_deferred_budget() -> anyhow::Result<()>
    {
        for files in [
            vec!["same.json", "same.json"],
            vec!["a.json", "a.json/b.json"],
            vec!["../escape.json"],
        ] {
            let directory = Directory::new()?;
            let mut project = project(&"x".repeat(16), "ok");
            project.extra_targets[0].options = FormatOptions {
                json_document: true,
                ..FormatOptions::default()
            };
            project.extra_targets[0].path = None;
            project.extra_targets[0].root.iteration = ScopeIteration::DynamicDocuments {
                source: vec!["Rows".into()],
                output_path: 2,
            };
            let error = run(&directory, &project, &files, SMALL).unwrap_err();
            let text = format!("{error:#}");
            assert!(text.contains("dynamic output path"), "{text}");
            assert!(!text.contains("per-document limit"), "{text}");
            directory.1.set(true);
        }
        let directory = Directory::new()?;
        let mut project = project(&"x".repeat(16), "ok");
        project.extra_targets[0].options = FormatOptions {
            json_document: true,
            ..FormatOptions::default()
        };
        project.extra_targets[0].path = None;
        project.extra_targets[0].root.iteration = ScopeIteration::DynamicDocuments {
            source: vec!["Rows".into()],
            output_path: 2,
        };
        let long = format!("{}.json", "x".repeat(super::super::MAX_PAYLOAD_PATH_BYTES));
        let error = run(&directory, &project, &[&long], SMALL).unwrap_err();
        assert!(
            format!("{error:#}").contains("4096-byte UTF-8 limit"),
            "{error:#}"
        );
        directory.1.set(true);
        Ok(())
    }

    #[test]
    fn unbounded_preview_compatibility_is_separate_from_deferred_csv() -> anyhow::Result<()> {
        let directory = Directory::new()?;
        let schema =
            SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)]);
        let rows = Instance::Repeated(vec![Instance::Group(
            vec![(
                "Value".into(),
                Instance::Scalar(Value::String("x".repeat(16))),
            )]
            .into(),
        )]);
        let ordinary = render_payload(
            Path::new("preview.csv"),
            &schema,
            &rows,
            &csv_options(),
            "2026-01-02T03:04:05Z",
        );
        std::fs::write(
            directory.0.join("preview-outcome.txt"),
            format!(
                "{:?}",
                ordinary.as_ref().map(|(bytes, rows)| (bytes.len(), rows))
            ),
        )?;
        let (bytes, records) = ordinary?;
        std::fs::write(directory.0.join("preview.bin"), &bytes)?;
        assert_eq!(bytes, b"xxxxxxxxxxxxxxxx\n");
        assert_eq!(records, 1);
        directory.1.set(true);
        Ok(())
    }
}
