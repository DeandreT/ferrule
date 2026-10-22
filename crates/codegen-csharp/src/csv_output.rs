use codegen::{ArtifactSet, CsvOutputPolicy, Program};
use ir::{ScalarType, SchemaKind};

use crate::{EmitError, file, literal};

const CSV_RUNTIME_SOURCE: &str =
    include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsv.cs");

pub(crate) fn emit(program: &Program, policy: &CsvOutputPolicy) -> Result<ArtifactSet, EmitError> {
    codegen::validate_csv_output(program, policy)?;
    emit_adapter(program, render(program, policy))
}

pub(crate) fn emit_document_set(
    program: &Program,
    policy: &CsvOutputPolicy,
) -> Result<ArtifactSet, EmitError> {
    codegen::validate_csv_document_set_output(program, policy)?;
    emit_adapter(program, render_document_set(program, policy))
}

fn emit_adapter(program: &Program, adapter: String) -> Result<ArtifactSet, EmitError> {
    add_adapter(crate::emit(program)?, adapter)
}

pub(crate) fn append_document_set(
    program: &Program,
    policy: &CsvOutputPolicy,
    artifacts: ArtifactSet,
) -> Result<ArtifactSet, EmitError> {
    codegen::validate_csv_document_set_output(program, policy)?;
    add_adapter(artifacts, render_document_set(program, policy))
}

fn add_adapter(artifacts: ArtifactSet, adapter: String) -> Result<ArtifactSet, EmitError> {
    // Keep ordinary emission and its runtime registration untouched. CSV adds
    // one partial class and its runtime only after the shared boundary proof.
    let mut files = artifacts.into_files();
    for generated in &mut files {
        match generated.path.as_str() {
            "GeneratedMapping.cs" => {
                let source = std::str::from_utf8(&generated.contents)
                    .expect("ordinary generated mapping is UTF-8");
                generated.contents = source
                    .replacen(
                        "public static class GeneratedMapping\n{",
                        "public static partial class GeneratedMapping\n{",
                        1,
                    )
                    .into_bytes();
            }
            "Ferrule.Generated.csproj" => {
                let project = std::str::from_utf8(&generated.contents)
                    .expect("ordinary generated project is UTF-8");
                generated.contents = project
                    .replacen(
                        "    <Compile Include=\"GeneratedMapping.cs\" />\n",
                        "    <Compile Include=\"GeneratedMapping.cs\" />\n    <Compile Include=\"GeneratedMapping.Csv.cs\" />\n",
                        1,
                    )
                    .into_bytes();
            }
            _ => {}
        }
    }
    files.push(file("GeneratedMapping.Csv.cs", adapter)?);
    files.push(file("Runtime/FerruleCsv.cs", CSV_RUNTIME_SOURCE)?);
    Ok(ArtifactSet::new(files)?)
}

fn render_policy(program: &Program, policy: &CsvOutputPolicy) -> String {
    let SchemaKind::Group { children, .. } = &program.target.kind else {
        unreachable!("shared CSV validation proves one flat group target")
    };
    let mut output = String::from(
        "namespace Ferrule.Generated;\n\npublic static partial class GeneratedMapping\n{\n    private static readonly global::Ferrule.Runtime.FerruleCsvField[] CsvOutputFields =\n        new global::Ferrule.Runtime.FerruleCsvField[]\n        {\n",
    );
    for field in children {
        let SchemaKind::Scalar { ty } = &field.kind else {
            unreachable!("shared CSV validation proves single scalar columns")
        };
        output.push_str("            new(");
        output.push_str(&literal::string(&field.name));
        output.push_str(", global::Ferrule.Runtime.FerruleScalarType.");
        output.push_str(match ty {
            ScalarType::String => "String",
            ScalarType::Int => "Int64",
            ScalarType::Float => "Double",
            ScalarType::Bool => "Bool",
        });
        output.push_str("),\n");
    }
    output.push_str(
        "        };\n\n    private static readonly global::Ferrule.Runtime.FerruleCsvWriteOptions CsvOutputPolicy =\n        new global::Ferrule.Runtime.FerruleCsvWriteOptions\n        {\n            Delimiter = ",
    );
    output.push_str(&character(policy.delimiter));
    output.push_str(",\n            Quote = ");
    output.push_str(&character(policy.quote));
    output.push_str(",\n            QuoteDisabled = ");
    output.push_str(boolean(policy.quote_disabled));
    output.push_str(",\n            HasHeaders = ");
    output.push_str(boolean(policy.has_headers));
    output.push_str(",\n            Utf8Bom = ");
    output.push_str(boolean(policy.utf8_bom));
    output.push_str(",\n            RawDelimitedCrLf = ");
    output.push_str(boolean(policy.raw_delimited_crlf));
    output.push_str(",\n        };\n");
    output
}

fn render(program: &Program, policy: &CsvOutputPolicy) -> String {
    let mut output = render_policy(program, policy);
    output.push_str(ADAPTERS);
    if program
        .extra_sources
        .iter()
        .any(|source| source.dynamic.is_none())
    {
        output.truncate(output.len() - 2); // Retain every existing method before the class close.
        output.push_str(NAMED_ADAPTERS);
        output.push_str("}\n");
    }
    if program
        .extra_sources
        .iter()
        .any(|source| source.dynamic.is_some())
    {
        output.truncate(output.len() - 2);
        output.push_str(DYNAMIC_ADAPTERS);
        output.push_str("}\n");
    }
    output
}

fn render_document_set(program: &Program, policy: &CsvOutputPolicy) -> String {
    let mut output = render_policy(program, policy);
    output.push_str(r#"
    public sealed record CsvOutputDocument(string Path, string Document);
    public sealed record CsvOutputBytesDocument(string Path, byte[] Document);

    /// <summary>Serialize one completed row document; this does not run the mapping or write a file.</summary>
    public static string SerializeCsvDocument(global::Ferrule.Runtime.FerruleInstance value) =>
        global::Ferrule.Runtime.FerruleCsv.Serialize(value, CsvOutputFields, CsvOutputPolicy);

    public static byte[] SerializeCsvDocumentBytes(global::Ferrule.Runtime.FerruleInstance value) =>
        global::Ferrule.Runtime.FerruleCsv.SerializeBytes(value, CsvOutputFields, CsvOutputPolicy);

    /// <summary>Materialize the mapping, then serialize each complete CSV document.</summary>
    public static global::System.Collections.Generic.IReadOnlyList<CsvOutputDocument> ExecuteCsvDocuments(
        global::Ferrule.Runtime.FerruleInstance source,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        if (Execute(source, executionContext) is not global::Ferrule.Runtime.FerruleDocumentSet documents)
            throw new global::System.InvalidOperationException("Expected validated document-row output.");
        var output = new global::System.Collections.Generic.List<CsvOutputDocument>(documents.Documents.Count);
        foreach (var document in documents.Documents)
            output.Add(new(document.Path, global::Ferrule.Runtime.FerruleCsv.Serialize(document.Value, CsvOutputFields, CsvOutputPolicy)));
        return output.AsReadOnly();
    }

    public static global::System.Collections.Generic.IReadOnlyList<CsvOutputBytesDocument> ExecuteCsvDocumentsBytes(
        global::Ferrule.Runtime.FerruleInstance source,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        if (Execute(source, executionContext) is not global::Ferrule.Runtime.FerruleDocumentSet documents)
            throw new global::System.InvalidOperationException("Expected validated document-row output.");
        var output = new global::System.Collections.Generic.List<CsvOutputBytesDocument>(documents.Documents.Count);
        foreach (var document in documents.Documents)
            output.Add(new(document.Path, global::Ferrule.Runtime.FerruleCsv.SerializeBytes(document.Value, CsvOutputFields, CsvOutputPolicy)));
        return output.AsReadOnly();
    }
}
"#);
    output
}

const ADAPTERS: &str = r#"
    public static string ExecuteCsv(
        global::Ferrule.Runtime.FerruleInstance source)
    {
        var primary = Execute(source);
        return global::Ferrule.Runtime.FerruleCsv.Serialize(primary, CsvOutputFields, CsvOutputPolicy);
    }

    public static string ExecuteCsv(
        global::Ferrule.Runtime.FerruleInstance source,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        var primary = Execute(source, executionContext);
        return global::Ferrule.Runtime.FerruleCsv.Serialize(primary, CsvOutputFields, CsvOutputPolicy);
    }

    public static byte[] ExecuteCsvBytes(
        global::Ferrule.Runtime.FerruleInstance source)
    {
        var primary = Execute(source);
        return global::Ferrule.Runtime.FerruleCsv.SerializeBytes(primary, CsvOutputFields, CsvOutputPolicy);
    }

    public static byte[] ExecuteCsvBytes(
        global::Ferrule.Runtime.FerruleInstance source,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        var primary = Execute(source, executionContext);
        return global::Ferrule.Runtime.FerruleCsv.SerializeBytes(primary, CsvOutputFields, CsvOutputPolicy);
    }
}
"#;

fn character(value: Option<char>) -> String {
    match value {
        Some(value) => format!("(char){}", u32::from(value)),
        None => "null".into(),
    }
}

const fn boolean(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

const NAMED_ADAPTERS: &str = r#"
    public static string ExecuteCsvWithSources(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources)
    {
        var primary = ExecuteWithSources(source, extraSources);
        return global::Ferrule.Runtime.FerruleCsv.Serialize(primary, CsvOutputFields, CsvOutputPolicy);
    }

    public static string ExecuteCsvWithSources(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        var primary = ExecuteWithSources(source, extraSources, executionContext);
        return global::Ferrule.Runtime.FerruleCsv.Serialize(primary, CsvOutputFields, CsvOutputPolicy);
    }

    public static byte[] ExecuteCsvBytesWithSources(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources)
    {
        var primary = ExecuteWithSources(source, extraSources);
        return global::Ferrule.Runtime.FerruleCsv.SerializeBytes(primary, CsvOutputFields, CsvOutputPolicy);
    }

    public static byte[] ExecuteCsvBytesWithSources(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext)
    {
        var primary = ExecuteWithSources(source, extraSources, executionContext);
        return global::Ferrule.Runtime.FerruleCsv.SerializeBytes(primary, CsvOutputFields, CsvOutputPolicy);
    }
"#;

const DYNAMIC_ADAPTERS: &str = r#"
    public static string ExecuteCsvWithDynamicSourceLoader(
        global::Ferrule.Runtime.FerruleInstance source,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)
    {
        var primary = ExecuteWithDynamicSourceLoader(source, loader);
        return global::Ferrule.Runtime.FerruleCsv.Serialize(primary, CsvOutputFields, CsvOutputPolicy);
    }

    public static string ExecuteCsvWithSourcesAndDynamicSourceLoader(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)
    {
        var primary = ExecuteWithSourcesAndDynamicSourceLoader(source, extraSources, loader);
        return global::Ferrule.Runtime.FerruleCsv.Serialize(primary, CsvOutputFields, CsvOutputPolicy);
    }

    public static string ExecuteCsvWithSourcesContextAndDynamicSourceLoader(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)
    {
        var primary = ExecuteWithSourcesContextAndDynamicSourceLoader(source, extraSources, executionContext, loader);
        return global::Ferrule.Runtime.FerruleCsv.Serialize(primary, CsvOutputFields, CsvOutputPolicy);
    }

    public static byte[] ExecuteCsvBytesWithDynamicSourceLoader(
        global::Ferrule.Runtime.FerruleInstance source,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)
    {
        var primary = ExecuteWithDynamicSourceLoader(source, loader);
        return global::Ferrule.Runtime.FerruleCsv.SerializeBytes(primary, CsvOutputFields, CsvOutputPolicy);
    }

    public static byte[] ExecuteCsvBytesWithSourcesAndDynamicSourceLoader(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)
    {
        var primary = ExecuteWithSourcesAndDynamicSourceLoader(source, extraSources, loader);
        return global::Ferrule.Runtime.FerruleCsv.SerializeBytes(primary, CsvOutputFields, CsvOutputPolicy);
    }

    public static byte[] ExecuteCsvBytesWithSourcesContextAndDynamicSourceLoader(
        global::Ferrule.Runtime.FerruleInstance source,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext executionContext,
        global::Ferrule.Runtime.IFerruleDynamicSourceLoader loader)
    {
        var primary = ExecuteWithSourcesContextAndDynamicSourceLoader(source, extraSources, executionContext, loader);
        return global::Ferrule.Runtime.FerruleCsv.SerializeBytes(primary, CsvOutputFields, CsvOutputPolicy);
    }
"#;
