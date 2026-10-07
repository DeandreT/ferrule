use codegen::{ArtifactSet, CsvOutputPolicy, Program};
use ir::{ScalarType, SchemaKind};

use crate::{EmitError, file, literal};

const CSV_RUNTIME_SOURCE: &str =
    include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsv.cs");

pub(crate) fn emit(program: &Program, policy: &CsvOutputPolicy) -> Result<ArtifactSet, EmitError> {
    codegen::validate_csv_output(program, policy)?;
    // Keep ordinary emission and its runtime registration untouched. CSV adds
    // one partial class and its runtime only after the shared boundary proof.
    let mut files = crate::emit(program)?.into_files();
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
    files.push(file("GeneratedMapping.Csv.cs", render(program, policy))?);
    files.push(file("Runtime/FerruleCsv.cs", CSV_RUNTIME_SOURCE)?);
    Ok(ArtifactSet::new(files)?)
}

fn render(program: &Program, policy: &CsvOutputPolicy) -> String {
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
    output.push_str(
        ",\n        };\n\n    public static string ExecuteCsv(\n        global::Ferrule.Runtime.FerruleInstance source)\n    {\n        var primary = Execute(source);\n        return global::Ferrule.Runtime.FerruleCsv.Serialize(primary, CsvOutputFields, CsvOutputPolicy);\n    }\n\n    public static string ExecuteCsv(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        var primary = Execute(source, executionContext);\n        return global::Ferrule.Runtime.FerruleCsv.Serialize(primary, CsvOutputFields, CsvOutputPolicy);\n    }\n\n    public static byte[] ExecuteCsvBytes(\n        global::Ferrule.Runtime.FerruleInstance source)\n    {\n        var primary = Execute(source);\n        return global::Ferrule.Runtime.FerruleCsv.SerializeBytes(primary, CsvOutputFields, CsvOutputPolicy);\n    }\n\n    public static byte[] ExecuteCsvBytes(\n        global::Ferrule.Runtime.FerruleInstance source,\n        global::Ferrule.Runtime.FerruleExecutionContext executionContext)\n    {\n        var primary = Execute(source, executionContext);\n        return global::Ferrule.Runtime.FerruleCsv.SerializeBytes(primary, CsvOutputFields, CsvOutputPolicy);\n    }\n}\n",
    );
    if program
        .extra_sources
        .iter()
        .any(|source| source.dynamic.is_none())
    {
        output.truncate(output.len() - 2); // Retain every existing method before the class close.
        output.push_str(NAMED_ADAPTERS);
        output.push_str("}\n");
    }
    output
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
