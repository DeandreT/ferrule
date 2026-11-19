use codegen::ArtifactSet;
use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{FormatOptions, Project, TabularBoundaryKind};

/// Generate a single JSON row -> new flat workbook byte entry point. Never infer
/// worksheet settings from paths, or silently discard unrelated boundary options.
pub fn emit_with_flat_xlsx_output(project: &Project) -> Result<ArtifactSet, String> {
    let target = &project.target_options;
    let sheet = target.xlsx_sheet.as_deref().unwrap_or("Sheet1");
    if project.source_options
        != (FormatOptions {
            json_document: true,
            ..Default::default()
        })
        || *target
            != (FormatOptions {
                tabular_kind: Some(TabularBoundaryKind::Xlsx),
                xlsx_sheet: target.xlsx_sheet.clone(),
                has_header_row: Some(true),
                ..Default::default()
            })
        || sheet.eq_ignore_ascii_case("History")
        || sheet.is_empty()
        || sheet.len() > 31
        || !sheet
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
        || !project.extra_sources.is_empty()
        || !project.extra_targets.is_empty()
        || project.root.output_path().is_some()
    {
        return Err(
            "XLSX output requires one JSON row and a new flat worksheet with headers".into(),
        );
    }
    let mut names = Vec::new();
    let mut kinds = Vec::new();
    for (schema, is_target) in [(&project.source, false), (&project.target, true)] {
        let SchemaKind::Group { children, .. } = &schema.kind else {
            return Err("expected flat row".into());
        };
        if children.is_empty()
            || children.len() > 26
            || *schema != SchemaNode::group(&schema.name, children.clone())
        {
            return Err("unsupported worksheet schema".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for child in children {
            let SchemaKind::Scalar { ty } = child.kind else {
                return Err("expected scalar column".into());
            };
            if child.name.is_empty()
                || child.name.len() > 64
                || !child
                    .name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_')
                || !seen.insert(&child.name)
                || *child != SchemaNode::scalar(&child.name, ty)
            {
                return Err("unsupported worksheet column".into());
            }
            let kind = match ty {
                ScalarType::String => "String",
                ScalarType::Int => "Int64",
                ScalarType::Bool => "Bool",
                _ => return Err("unsupported worksheet scalar".into()),
            };
            if is_target {
                names.push(crate::literal::string(&child.name));
                kinds.push(format!("global::Ferrule.Runtime.FerruleValueKind.{kind}"));
            }
        }
    }
    let program = codegen::lower(project).map_err(|e| e.to_string())?;
    let mut library = crate::library(&program).map_err(|e| e.to_string())?;
    library.extend_mapping(format!(r#"
    public static byte[] ExecuteJsonToXlsxBytes(byte[] source, global::Ferrule.Runtime.FerruleExecutionContext execution)
    {{
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(execution);
        if (source.Length > 65536) throw new global::System.IO.InvalidDataException("XLSX input limit");
        var row = Execute(global::Ferrule.Runtime.FerruleJson.ParseEmbeddedBytes(SourceJsonSchema, source), execution);
        return global::Ferrule.Runtime.FlatWorkbook.Write(row, {}, [{}], [{}]);
    }}"#, crate::literal::string(sheet), names.join(","), kinds.join(",")));
    library.add_runtime(&[("Runtime/FlatWorkbook.cs", include_str!("FlatWorkbook.cs"))]);
    library.into_artifacts().map_err(|e| e.to_string())
}
