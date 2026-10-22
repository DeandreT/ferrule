use codegen::{ArtifactSet, CsvOutputPolicy, Program};
use ir::{ScalarType, SchemaKind};

use crate::composition::{ClassPart, Library};
use crate::{EmitError, literal};

const CSV_RUNTIME_SOURCE: &str =
    include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsv.cs");

pub(crate) fn emit(program: &Program, policy: &CsvOutputPolicy) -> Result<ArtifactSet, EmitError> {
    library(program, policy)?.into_artifacts()
}

/// The ordinary library plus one partial class and its runtime, added only
/// after the shared boundary proof.
pub(crate) fn library(program: &Program, policy: &CsvOutputPolicy) -> Result<Library, EmitError> {
    codegen::validate_csv_output(program, policy)?;
    let mut library = crate::library(program)?;
    library.add_companion("GeneratedMapping.Csv.cs", render(program, policy));
    library.add_runtime(&[("Runtime/FerruleCsv.cs", CSV_RUNTIME_SOURCE)]);
    Ok(library)
}

fn render(program: &Program, policy: &CsvOutputPolicy) -> ClassPart {
    let SchemaKind::Group { children, .. } = &program.target.kind else {
        unreachable!("shared CSV validation proves one flat group target")
    };
    let mut output = String::from(
        "    private static readonly global::Ferrule.Runtime.FerruleCsvField[] CsvOutputFields =\n        new global::Ferrule.Runtime.FerruleCsvField[]\n        {\n",
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
    output.push_str(
        ",\n        };\n\n    public static string ExecuteCsv(\n        global::Ferrule.Runtime.FerruleInstance source)\n    {\n        var primary = Execute(source);\n        return global::Ferrule.Runtime.FerruleCsv.Serialize(primary, CsvOutputFields, CsvOutputPolicy);\n    }\n\n    public static string ExecuteCsv(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        var primary = Execute(source, executionContext);\n        return global::Ferrule.Runtime.FerruleCsv.Serialize(primary, CsvOutputFields, CsvOutputPolicy);\n    }\n\n    public static byte[] ExecuteCsvBytes(\n        global::Ferrule.Runtime.FerruleInstance source)\n    {\n        var primary = Execute(source);\n        return global::Ferrule.Runtime.FerruleCsv.SerializeBytes(primary, CsvOutputFields, CsvOutputPolicy);\n    }\n\n    public static byte[] ExecuteCsvBytes(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        var primary = Execute(source, executionContext);\n        return global::Ferrule.Runtime.FerruleCsv.SerializeBytes(primary, CsvOutputFields, CsvOutputPolicy);\n    }\n",
    );
    if program
        .extra_sources
        .iter()
        .any(|source| source.dynamic.is_none())
    {
        output.push_str(NAMED_ADAPTERS);
    }
    if program
        .extra_sources
        .iter()
        .any(|source| source.dynamic.is_some())
    {
        output.push_str(DYNAMIC_ADAPTERS);
    }
    ClassPart::members(output)
}

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
