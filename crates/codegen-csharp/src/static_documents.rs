use std::fmt;

use codegen::{
    ArtifactSet, DocumentBoundaryDescriptor, DocumentBoundaryFormat, Program,
    StaticDocumentBoundaryError, StaticDocumentBoundaryPolicy, StaticDocumentBoundaryProfile,
};

use crate::{EmitError, file, literal};

/// Failures from complete static JSON/X12 document companions.
#[derive(Debug)]
pub enum StaticDocumentEmitError {
    Policy(StaticDocumentBoundaryError),
    Ordinary(EmitError),
    ArtifactInvariant { path: &'static str },
}

impl fmt::Display for StaticDocumentEmitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(error) => write!(formatter, "static document boundary: {error}"),
            Self::Ordinary(error) => write!(formatter, "static document emission: {error}"),
            Self::ArtifactInvariant { path } => {
                write!(
                    formatter,
                    "static document emission requires the ordinary artifact {path}"
                )
            }
        }
    }
}

impl std::error::Error for StaticDocumentEmitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Policy(error) => Some(error),
            Self::Ordinary(error) => Some(error),
            Self::ArtifactInvariant { .. } => None,
        }
    }
}

impl From<EmitError> for StaticDocumentEmitError {
    fn from(error: EmitError) -> Self {
        Self::Ordinary(error)
    }
}

pub(crate) fn emit(
    program: &Program,
    policy: &StaticDocumentBoundaryPolicy,
) -> Result<ArtifactSet, StaticDocumentEmitError> {
    let profile = codegen::prepare_static_document_boundary(program, policy)
        .map_err(StaticDocumentEmitError::Policy)?;
    let mut files = crate::emit(program)?.into_files();
    for (path, before, after) in [
        (
            "GeneratedMapping.cs",
            "public static class GeneratedMapping\n{",
            "public static partial class GeneratedMapping\n{",
        ),
        (
            "Ferrule.Generated.csproj",
            "    <Compile Include=\"GeneratedMapping.cs\" />\n",
            "    <Compile Include=\"GeneratedMapping.cs\" />\n    <Compile Include=\"GeneratedMapping.Documents.cs\" />\n",
        ),
    ] {
        let generated = files
            .iter_mut()
            .find(|generated| generated.path.as_str() == path)
            .ok_or(StaticDocumentEmitError::ArtifactInvariant { path })?;
        let source = std::str::from_utf8(&generated.contents)
            .map_err(|_| StaticDocumentEmitError::ArtifactInvariant { path })?;
        if source.matches(before).count() != 1 {
            return Err(StaticDocumentEmitError::ArtifactInvariant { path });
        }
        generated.contents = source.replacen(before, after, 1).into_bytes();
    }
    files.push(file("GeneratedMapping.Documents.cs", render(&profile))?);
    for (path, source) in crate::x12_output::X12_SOURCES {
        files.push(file(path, source)?);
    }
    ArtifactSet::new(files)
        .map_err(EmitError::from)
        .map_err(StaticDocumentEmitError::from)
}

fn codec(format: DocumentBoundaryFormat) -> &'static str {
    match format {
        DocumentBoundaryFormat::Json => "FerruleJson",
        DocumentBoundaryFormat::X12 => "FerruleX12",
    }
}

fn format_name(format: DocumentBoundaryFormat) -> &'static str {
    match format {
        DocumentBoundaryFormat::Json => "Json",
        DocumentBoundaryFormat::X12 => "X12",
    }
}

fn render(profile: &StaticDocumentBoundaryProfile) -> String {
    let mut output = String::from("namespace Ferrule.Generated;\n\n");
    output.push_str(OUTPUT_TYPES);
    output.push_str("public static partial class GeneratedMapping\n{\n");
    for (name, descriptor) in [
        ("DocumentSourceDescriptor", &profile.source.descriptor),
        ("DocumentTargetDescriptor", &profile.target.descriptor),
    ] {
        output.push_str(&format!(
            "    private const string {name} = {};\n",
            literal::string(descriptor)
        ));
    }
    for (name, descriptors) in [
        ("ExtraDocumentSourceDescriptors", &profile.extra_sources),
        ("ExtraDocumentTargetDescriptors", &profile.extra_targets),
    ] {
        if descriptors.is_empty() {
            continue;
        }
        output.push_str(&format!(
            "    private static readonly string[] {name} =\n    [\n"
        ));
        for descriptor in descriptors {
            output.push_str(&format!(
                "        {},\n",
                literal::string(&descriptor.boundary.descriptor)
            ));
        }
        output.push_str("    ];\n");
    }
    for bytes in [false, true] {
        render_api(profile, bytes, &mut output);
        render_name_validation(profile, bytes, &mut output);
        render_named_parse(profile, bytes, &mut output);
        render_selected_serialize(profile, bytes, &mut output);
    }
    output.push_str("}\n");
    output
}

fn render_api(profile: &StaticDocumentBoundaryProfile, bytes: bool, output: &mut String) {
    let (method, result, document, input, suffix) = if bytes {
        (
            "ExecuteDocumentBytesSelectedTarget",
            "SelectedDocumentBytesTargetOutput",
            "byte[]",
            "NamedDocumentBytesInput",
            "Bytes",
        )
    } else {
        (
            "ExecuteDocumentSelectedTarget",
            "SelectedDocumentTargetOutput",
            "string",
            "NamedDocumentInput",
            "",
        )
    };
    let source_codec = codec(profile.source.format);
    output.push_str(&format!(r#"
    public static {result} {method}(
        {document} source,
        global::Ferrule.Runtime.FerruleTargetSelection selection)
    {{
        return {method}WithHost(source, selection, global::System.Array.Empty<{input}>());
    }}

    public static {result} {method}WithHost(
        {document} source,
        global::Ferrule.Runtime.FerruleTargetSelection selection,
        global::System.Collections.Generic.IReadOnlyList<{input}> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext? executionContext = null)
    {{
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(selection);
        var selected = ResolveTarget(selection);
        global::System.ArgumentNullException.ThrowIfNull(extraSources);
        ValidateNamedDocument{suffix}InputNames(extraSources);
        var parsedSource = global::Ferrule.Runtime.{source_codec}.ParseEmbedded{suffix}(DocumentSourceDescriptor, source);
        var parsedInputs = ParseNamedDocument{suffix}Inputs(extraSources);
        var outcome = ExecuteSelectedTargetWithHost(
            parsedSource, selection, parsedInputs, executionContext, dynamicSourceLoader: null);
        return Serialize{result}(outcome, selected, executionContext);
    }}
"#));
}

fn render_name_validation(
    profile: &StaticDocumentBoundaryProfile,
    bytes: bool,
    output: &mut String,
) {
    let (input, suffix) = if bytes {
        ("NamedDocumentBytesInput", "Bytes")
    } else {
        ("NamedDocumentInput", "")
    };
    output.push_str(&format!(
        "\n    private static void ValidateNamedDocument{suffix}InputNames(\n        global::System.Collections.Generic.IReadOnlyList<{input}> extraSources)\n    {{\n"
    ));
    if !profile.extra_sources.is_empty() {
        output.push_str("        var matched = new global::System.Collections.Generic.HashSet<string>(global::System.StringComparer.Ordinal);\n");
    }
    output.push_str("        foreach (var extraSource in extraSources)\n        {\n            global::System.ArgumentNullException.ThrowIfNull(extraSource);\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Name);\n");
    if profile.extra_sources.is_empty() {
        unexpected_named_source(output, "            ");
    } else {
        if !bytes {
            output.push_str("            global::System.ArgumentNullException.ThrowIfNull(extraSource.Document);\n");
        }
        let names = profile
            .extra_sources
            .iter()
            .map(|source| literal::string(&source.name))
            .collect::<Vec<_>>()
            .join(" or ");
        output.push_str(&format!(
            "            if (extraSource.Name is not ({names}))\n            {{\n"
        ));
        unexpected_named_source(output, "                ");
        output.push_str("            }\n            if (!matched.Add(extraSource.Name))\n            {\n                throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                    global::Ferrule.Runtime.FerruleRuntimeError.DuplicateNamedSource,\n                    $\"named source '{extraSource.Name}' was supplied more than once\",\n                    detail: extraSource.Name);\n            }\n");
    }
    output.push_str("        }\n");
    for source in &profile.extra_sources {
        let name = literal::string(&source.name);
        output.push_str(&format!(
            "        if (!matched.Contains({name}))\n        {{\n            throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                global::Ferrule.Runtime.FerruleRuntimeError.MissingNamedSource,\n                \"named source \" + {name} + \" is required by this mapping\",\n                detail: {name});\n        }}\n"
        ));
    }
    output.push_str("    }\n");
}

fn unexpected_named_source(output: &mut String, indent: &str) {
    output.push_str(&format!(
        "{indent}throw new global::Ferrule.Runtime.FerruleRuntimeException(\n{indent}    global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,\n{indent}    $\"named source '{{extraSource.Name}}' is not declared by this mapping\",\n{indent}    detail: extraSource.Name);\n"
    ));
}

fn render_named_parse(profile: &StaticDocumentBoundaryProfile, bytes: bool, output: &mut String) {
    let (input, suffix) = if bytes {
        ("NamedDocumentBytesInput", "Bytes")
    } else {
        ("NamedDocumentInput", "")
    };
    output.push_str(&format!(
        "\n    private static global::System.Collections.Generic.IReadOnlyList<NamedInput> ParseNamedDocument{suffix}Inputs(\n        global::System.Collections.Generic.IReadOnlyList<{input}> extraSources)\n    {{\n"
    ));
    if profile.extra_sources.is_empty() {
        output.push_str("        return global::System.Array.Empty<NamedInput>();\n    }\n");
        return;
    }
    output.push_str("        var parsed = new global::System.Collections.Generic.List<NamedInput>(extraSources.Count);\n        foreach (var extraSource in extraSources)\n        {\n            global::System.ArgumentNullException.ThrowIfNull(extraSource.Document);\n            var instance = extraSource.Name switch\n            {\n");
    for (index, source) in profile.extra_sources.iter().enumerate() {
        let name = literal::string(&source.name);
        let source_codec = codec(source.boundary.format);
        output.push_str(&format!(
            "                {name} => global::Ferrule.Runtime.{source_codec}.ParseEmbedded{suffix}(ExtraDocumentSourceDescriptors[{index}], extraSource.Document),\n"
        ));
    }
    output.push_str("                _ => throw new global::System.InvalidOperationException(\"Preflighted document source is invalid.\"),\n            };\n            parsed.Add(new NamedInput(extraSource.Name, instance));\n        }\n        return parsed;\n    }\n");
}

fn serialization(
    boundary: &DocumentBoundaryDescriptor,
    descriptor: &str,
    instance: &str,
    suffix: &str,
) -> String {
    let selected_codec = codec(boundary.format);
    let call = format!(
        "global::Ferrule.Runtime.{selected_codec}.SerializeEmbedded{suffix}({descriptor}, {instance}"
    );
    if boundary.format == DocumentBoundaryFormat::X12 {
        format!(
            "executionContext is null\n                        ? {call})\n                        : {call}, executionContext)"
        )
    } else {
        format!("{call})")
    }
}

fn render_selected_serialize(
    profile: &StaticDocumentBoundaryProfile,
    bytes: bool,
    output: &mut String,
) {
    let (result, named_output, suffix) = if bytes {
        (
            "SelectedDocumentBytesTargetOutput",
            "NamedDocumentBytesOutput",
            "Bytes",
        )
    } else {
        ("SelectedDocumentTargetOutput", "NamedDocumentOutput", "")
    };
    let primary_format = format_name(profile.target.format);
    let primary_call = serialization(
        &profile.target,
        "DocumentTargetDescriptor",
        "primary.Instance",
        suffix,
    );
    output.push_str(&format!(
        r#"
    private static {result} Serialize{result}(
        SelectedTargetOutput outcome, int selected,
        global::Ferrule.Runtime.FerruleExecutionContext? executionContext)
    {{
        switch (selected)
        {{
            case 0 when outcome is SelectedTargetOutput.Primary primary:
                return new {result}.Primary(
                    DocumentBoundaryFormat.{primary_format}, {primary_call});
"#
    ));
    for (index, target) in profile.extra_targets.iter().enumerate() {
        let selected = index + 1;
        let name = literal::string(&target.name);
        let target_format = format_name(target.boundary.format);
        let descriptor = format!("ExtraDocumentTargetDescriptors[{index}]");
        let instance = format!("named_{selected}.Output.Instance");
        let call = serialization(&target.boundary, &descriptor, &instance, suffix);
        output.push_str(&format!(r#"            case {selected} when outcome is SelectedTargetOutput.Named named_{selected} &&
                global::System.String.Equals(named_{selected}.Output.Name, {name}, global::System.StringComparison.Ordinal):
                return new {result}.Named(new {named_output}(
                    named_{selected}.Output.Name, DocumentBoundaryFormat.{target_format}, {call}));
"#));
    }
    output.push_str("            default:\n                throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                    global::Ferrule.Runtime.FerruleRuntimeError.JsonBoundary,\n                    \"generated mapping returned a target that does not match the resolved selection\");\n        }\n    }\n");
}

const OUTPUT_TYPES: &str = r#"public enum DocumentBoundaryFormat { Json, X12 }

public sealed record NamedDocumentInput(string Name, string Document);
public sealed record NamedDocumentBytesInput(string Name, byte[] Document);
public sealed record NamedDocumentOutput(string Name, DocumentBoundaryFormat Format, string Document);
public sealed record NamedDocumentBytesOutput(string Name, DocumentBoundaryFormat Format, byte[] Document);

public abstract class SelectedDocumentTargetOutput
{
    private protected SelectedDocumentTargetOutput() { }

    public sealed class Primary : SelectedDocumentTargetOutput
    {
        internal Primary(DocumentBoundaryFormat format, string document) { Format = format; Document = document; }
        public DocumentBoundaryFormat Format { get; }
        public string Document { get; }
    }

    public sealed class Named : SelectedDocumentTargetOutput
    {
        internal Named(NamedDocumentOutput output) { Output = output; }
        public NamedDocumentOutput Output { get; }
    }
}

public abstract class SelectedDocumentBytesTargetOutput
{
    private protected SelectedDocumentBytesTargetOutput() { }

    public sealed class Primary : SelectedDocumentBytesTargetOutput
    {
        internal Primary(DocumentBoundaryFormat format, byte[] document) { Format = format; Document = document; }
        public DocumentBoundaryFormat Format { get; }
        public byte[] Document { get; }
    }

    public sealed class Named : SelectedDocumentBytesTargetOutput
    {
        internal Named(NamedDocumentBytesOutput output) { Output = output; }
        public NamedDocumentBytesOutput Output { get; }
    }
}

"#;

#[cfg(test)]
mod tests;
