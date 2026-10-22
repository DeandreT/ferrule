use std::fmt;

use codegen::{ArtifactSet, CsvDocumentBoundaryError, CsvDocumentBoundaryPolicy, Program};
use ir::{ScalarType, SchemaKind};

use crate::{EmitError, file, literal};

#[derive(Debug)]
pub enum CsvDocumentEmitError {
    Boundary(Box<CsvDocumentBoundaryError>),
    Emit(EmitError),
    Visitor(codegen::DocumentVisitorEmitError<EmitError>),
    ArtifactInvariant,
}

impl fmt::Display for CsvDocumentEmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boundary(e) => e.fmt(f),
            Self::Emit(e) => e.fmt(f),
            Self::Visitor(e) => e.fmt(f),
            Self::ArtifactInvariant => {
                f.write_str("CSV document project artifact is missing its compile entry")
            }
        }
    }
}

impl std::error::Error for CsvDocumentEmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Boundary(e) => Some(e.as_ref()),
            Self::Emit(e) => Some(e),
            Self::Visitor(e) => Some(e),
            Self::ArtifactInvariant => None,
        }
    }
}

/// Add complete CSV text/byte entry points using the ordinary generated mapper.
pub fn emit_with_csv_document_adapters(
    program: &Program,
    policy: &CsvDocumentBoundaryPolicy,
) -> Result<ArtifactSet, CsvDocumentEmitError> {
    emit_document(program, policy, false)
}

pub fn emit_with_csv_document_set_boundary(
    program: &Program,
    policy: &CsvDocumentBoundaryPolicy,
) -> Result<ArtifactSet, CsvDocumentEmitError> {
    emit_document(program, policy, true)
}

/// Complete document metadata and flat CSV parsing/serialization, with explicit
/// host-controlled acquisition, clocks and publication.
pub fn emit_document_with_csv_document_set_boundary(
    document: &codegen::DocumentProgram,
    policy: &CsvDocumentBoundaryPolicy,
) -> Result<ArtifactSet, CsvDocumentEmitError> {
    let artifacts = emit_with_csv_document_set_boundary(document.program(), policy)?;
    crate::primary_filename::append(artifacts, document).map_err(CsvDocumentEmitError::Emit)
}

fn emit_document(
    program: &Program,
    policy: &CsvDocumentBoundaryPolicy,
    document_set: bool,
) -> Result<ArtifactSet, CsvDocumentEmitError> {
    (if document_set {
        codegen::validate_csv_document_set_boundary(program, policy)
    } else {
        codegen::validate_csv_document_boundary(program, policy)
    })
    .map_err(|e| CsvDocumentEmitError::Boundary(Box::new(e)))?;
    let artifacts = if document_set {
        crate::emit_with_csv_document_set_visitor(program, &policy.target)
            .map_err(CsvDocumentEmitError::Visitor)?
    } else {
        crate::emit_with_csv_output(program, &policy.target).map_err(CsvDocumentEmitError::Emit)?
    };
    let mut files = artifacts.into_files();
    let project = files
        .iter_mut()
        .find(|f| f.path.as_str() == "Ferrule.Generated.csproj")
        .ok_or(CsvDocumentEmitError::ArtifactInvariant)?;
    let content = std::str::from_utf8(&project.contents)
        .map_err(|_| CsvDocumentEmitError::ArtifactInvariant)?;
    let entry = "    <Compile Include=\"GeneratedMapping.cs\" />\n";
    if content.matches(entry).count() != 1 {
        return Err(CsvDocumentEmitError::ArtifactInvariant);
    }
    project.contents = content
        .replacen(
            entry,
            &format!("{entry}    <Compile Include=\"GeneratedMapping.CsvDocument.cs\" />\n"),
            1,
        )
        .into_bytes();
    files.push(
        file(
            "GeneratedMapping.CsvDocument.cs",
            render(program, policy, document_set),
        )
        .map_err(CsvDocumentEmitError::Emit)?,
    );
    files.push(
        file(
            "Runtime/FerruleCsvInput.cs",
            include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsvInput.cs"),
        )
        .map_err(CsvDocumentEmitError::Emit)?,
    );
    if !document_set {
        files.push(
            file(
                "Runtime/FerruleCsvSource.cs",
                include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleCsvSource.cs"),
            )
            .map_err(CsvDocumentEmitError::Emit)?,
        );
    }
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(CsvDocumentEmitError::Emit)
}

fn render(program: &Program, policy: &CsvDocumentBoundaryPolicy, document_set: bool) -> String {
    let SchemaKind::Group { children, .. } = &program.source.kind else {
        unreachable!("admitted CSV source")
    };
    let mut s = String::from(
        "namespace Ferrule.Generated;\n\npublic static partial class GeneratedMapping\n{\n    private static readonly global::Ferrule.Runtime.FerruleCsvReadField[] CsvInputFields =\n    [\n",
    );
    for field in children {
        let SchemaKind::Scalar { ty } = field.kind else {
            unreachable!("admitted CSV field")
        };
        let ty = match ty {
            ScalarType::String => "String",
            ScalarType::Int => "Int64",
            ScalarType::Float => "Double",
            ScalarType::Bool => "Bool",
        };
        s.push_str(&format!(
            "        new({}, global::Ferrule.Runtime.FerruleScalarType.{ty}),\n",
            literal::string(&field.name)
        ));
    }
    let p = &policy.source;
    let character =
        |c: Option<char>| c.map_or_else(|| "null".into(), |c| format!("(char){}", u32::from(c)));
    s.push_str(&format!("    ];\n    private static readonly global::Ferrule.Runtime.FerruleCsvReadOptions CsvInputOptions = new()\n    {{\n        Delimiter = {},\n        Quote = {},\n        QuoteDisabled = {},\n        HasHeaders = {},\n        PreserveEmptyStrings = {},\n    }};\n", character(p.delimiter), character(p.quote), p.quote_disabled, p.has_headers, p.preserve_empty_strings));
    for bytes in [false, true] {
        let suffix = if bytes { "Bytes" } else { "" };
        let ty = if bytes { "byte[]" } else { "string" };
        s.push_str(&format!("\n    public static global::Ferrule.Runtime.FerruleInstance ParseCsv{suffix}({ty} source) =>\n        global::Ferrule.Runtime.FerruleCsvInput.Parse{suffix}(source, CsvInputFields, CsvInputOptions);\n"));
        if document_set {
            continue;
        }
        for contextual in [false, true] {
            let arg = if contextual {
                ", global::Ferrule.Runtime.FerruleExecutionContext executionContext"
            } else {
                ""
            };
            let pass = if contextual { ", executionContext" } else { "" };
            s.push_str(&format!(
                "\n    public static {ty} ExecuteCsvDocument{suffix}({ty} source{arg})\n    {{\n"
            ));
            if contextual {
                s.push_str(
                    "        global::System.ArgumentNullException.ThrowIfNull(executionContext);\n",
                );
            }
            s.push_str(&format!("        var input = ParseCsv{suffix}(source);\n        return ExecuteCsv{suffix}(input{pass});\n    }}\n"));
        }
    }
    s.push_str("\n    public static global::Ferrule.Runtime.FerruleInstance ParseCsvBytesWithDecodingPolicy(byte[] source, global::Ferrule.Runtime.FerruleCsvByteDecodingPolicy policy) =>\n        global::Ferrule.Runtime.FerruleCsvInput.ParseBytesWithDecodingPolicy(source, CsvInputFields, policy, CsvInputOptions);\n");
    for contextual in [false, true].into_iter().filter(|_| !document_set) {
        let argument = if contextual {
            ", global::Ferrule.Runtime.FerruleExecutionContext execution"
        } else {
            ""
        };
        let pass = if contextual { ", execution" } else { "" };
        s.push_str(&format!("\n    public static byte[] ExecuteCsvDocumentBytesWithDecodingPolicy(byte[] source, global::Ferrule.Runtime.FerruleCsvByteDecodingPolicy policy{argument})\n    {{\n"));
        if contextual {
            s.push_str("        global::System.ArgumentNullException.ThrowIfNull(execution);\n");
        }
        s.push_str(&format!("        var input = ParseCsvBytesWithDecodingPolicy(source, policy);\n        return ExecuteCsvBytes(input{pass});\n    }}\n"));
    }
    s.push_str("}\n");
    s
}
