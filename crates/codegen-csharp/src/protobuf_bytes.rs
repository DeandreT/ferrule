//! Closed scalar-message profile; generate descriptors from the project's embedded files only.
use crate::{composition::ClassPart, literal};
use format_protobuf::{Cardinality, FieldType, Layout, ScalarType};
use ir::SchemaNode;
use mapping::{FormatOptions, Project};

fn descriptor(schema: &SchemaNode, options: &FormatOptions) -> Result<String, String> {
    let pb = options
        .protobuf
        .as_ref()
        .ok_or("missing saved protobuf boundary")?;
    if options
        != &(FormatOptions {
            protobuf: Some(pb.clone()),
            ..Default::default()
        })
    {
        return Err("mixed protobuf boundary settings".into());
    }
    let layout = Layout::parse_files(
        pb.schema_path.as_deref().unwrap_or("root.proto"),
        &pb.schema,
        pb.imports
            .iter()
            .map(|file| (file.path.as_str(), file.source.as_str())),
    )
    .map_err(|e| e.to_string())?;
    let root = layout
        .resolve_message(&pb.root_message)
        .map_err(|e| e.to_string())?;
    let message = layout.message(root).ok_or("invalid protobuf root")?;
    if message.fields().len() > 256 {
        return Err("protobuf field limit".into());
    }
    let mut fields = Vec::new();
    for f in message.fields() {
        let kind = match f.ty() {
            FieldType::Scalar(ScalarType::String | ScalarType::Bytes) => "Text",
            FieldType::Scalar(ScalarType::Int64) => "Int64",
            FieldType::Scalar(ScalarType::Bool) => "Bool",
            _ => return Err("unsupported protobuf field type".into()),
        };
        if f.oneof().is_some()
            || !matches!(
                f.cardinality(),
                Cardinality::Implicit | Cardinality::Required
            )
        {
            return Err("unsupported protobuf presence or collection".into());
        }
        fields.push(format!(
            "new({}, {}, PbKind.{kind}, {})",
            literal::string(f.name()),
            f.number(),
            f.cardinality() == Cardinality::Required
        ));
    }
    if format_protobuf::to_ir_schema(&layout, &pb.root_message).map_err(|e| e.to_string())?
        != *schema
    {
        return Err("saved protobuf root does not describe mapping schema".into());
    }
    Ok(format!("[{}]", fields.join(", ")))
}

/// Emits bounded protobuf byte entry points for static primary and named scalar messages.
/// No descriptor, filename, or root message is supplied at execution time.
pub fn emit_with_protobuf_bytes(project: &Project) -> Result<codegen::ArtifactSet, String> {
    if project.extra_sources.len() > 63
        || project.extra_targets.len() > 63
        || project.root.output_path().is_some()
        || project
            .extra_sources
            .iter()
            .any(|s| s.dynamic_path.is_some())
        || project
            .extra_targets
            .iter()
            .any(|t| t.root.output_path().is_some())
    {
        return Err("protobuf requires a bounded static endpoint set".into());
    }
    let sources = std::iter::once((&project.source, &project.source_options)).chain(
        project
            .extra_sources
            .iter()
            .map(|s| (&s.schema, &s.options)),
    );
    let targets = std::iter::once((&project.target, &project.target_options)).chain(
        project
            .extra_targets
            .iter()
            .map(|t| (&t.schema, &t.options)),
    );
    let descriptors = |pairs: Vec<(&SchemaNode, &FormatOptions)>| {
        pairs
            .into_iter()
            .map(|(s, o)| descriptor(s, o))
            .collect::<Result<Vec<_>, _>>()
            .map(|v| v.join(",\n"))
    };
    let mut members = format!(
        "private static readonly PbField[][] PbInputs = [{}];\nprivate static readonly PbField[][] PbOutputs = [{}];\nprivate static readonly string[] PbNames = [{}];\n",
        descriptors(sources.collect())?,
        descriptors(targets.collect())?,
        project
            .extra_sources
            .iter()
            .map(|s| literal::string(&s.name))
            .collect::<Vec<_>>()
            .join(",")
    );
    members.push_str(include_str!("protobuf_bytes.cs.inc"));
    let program = codegen::lower(project).map_err(|e| e.to_string())?;
    let mut library = crate::library(&program).map_err(|e| e.to_string())?;
    library.add_companion("GeneratedMapping.Protobuf.cs", ClassPart::members(members));
    library.into_artifacts().map_err(|e| e.to_string())
}
