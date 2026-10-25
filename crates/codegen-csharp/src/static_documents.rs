//! Static JSON/X12/EDIFACT companions over the ordinary complete mapping.
//!
//! `ExecuteDocumentOutputs[WithHost]` and `ExecuteDocumentBytesOutputs[WithHost]`
//! admit every input once and execute the mapping once. The primary and true
//! named targets use their own admitted descriptors, in declaration order; no
//! bundle is returned until every physical writer succeeds. Host context is
//! shared unchanged by mapping and EDI completion (no implicit clock is added).
//! Selected-target companions retain their existing contract. XML, dynamic
//! sources/targets and document iteration remain outside this boundary policy.
//! Complete input/output bundles each allow 4096 documents and 256 MiB of strict
//! UTF-8, in addition to each codec's existing per-document/traversal limits.
//! These are admission counters, not streaming or peak-memory guarantees.

use std::fmt;

use codegen::{
    ArtifactSet, DocumentBoundaryDescriptor, DocumentBoundaryFormat, Program,
    StaticDocumentBoundaryError, StaticDocumentBoundaryPolicy, StaticDocumentBoundaryProfile,
};

use crate::Options;
use crate::composition::ClassPart;
use crate::{EmitError, literal};
use codegen::GenerationProfile;

/// Failures from complete static JSON/X12/EDIFACT document companions.
#[derive(Debug)]
pub enum StaticDocumentEmitError {
    Policy(StaticDocumentBoundaryError),
    Ordinary(EmitError),
    /// Retained for API compatibility; structural composition never produces it.
    ArtifactInvariant {
        path: &'static str,
    },
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
    options: &Options,
) -> Result<ArtifactSet, StaticDocumentEmitError> {
    let profile = codegen::prepare_static_document_boundary(program, policy)
        .map_err(StaticDocumentEmitError::Policy)?;
    let mut library = crate::library(program)?;
    library.add_companion("GeneratedMapping.Documents.cs", render(&profile));
    if has_edifact(&profile) {
        crate::edi_runtime::add_runtime(&mut library);
    } else {
        library.add_runtime(&crate::x12_output::X12_SOURCES);
    }
    library
        .into_artifacts_with(options, GenerationProfile::StaticDocuments)
        .map_err(StaticDocumentEmitError::from)
}

fn codec(format: DocumentBoundaryFormat) -> &'static str {
    match format {
        DocumentBoundaryFormat::Json => "FerruleJson",
        DocumentBoundaryFormat::X12 => "FerruleX12",
        DocumentBoundaryFormat::Edifact => "FerruleEdi",
    }
}

fn has_edifact(profile: &StaticDocumentBoundaryProfile) -> bool {
    std::iter::once(&profile.source)
        .chain(std::iter::once(&profile.target))
        .chain(profile.extra_sources.iter().map(|source| &source.boundary))
        .chain(profile.extra_targets.iter().map(|target| &target.boundary))
        .any(|boundary| boundary.format == DocumentBoundaryFormat::Edifact)
}

fn format_name(format: DocumentBoundaryFormat) -> &'static str {
    match format {
        DocumentBoundaryFormat::Json => "Json",
        DocumentBoundaryFormat::X12 => "X12",
        DocumentBoundaryFormat::Edifact => "Edifact",
    }
}

fn render(profile: &StaticDocumentBoundaryProfile) -> ClassPart {
    let mut types = String::from(crate::document_output_types::OUTPUT_TYPES);
    types.push_str(COMPLETE_OUTPUT_TYPES);
    let mut output = String::from(COMPLETE_OUTPUT_HELPERS_START);
    if has_edifact(profile) {
        output.push_str(" or global::Ferrule.Runtime.FerruleEdiException");
    }
    output.push_str(COMPLETE_OUTPUT_HELPERS_END);
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
        render_complete_api(bytes, &mut output);
        render_complete_parse(profile, bytes, &mut output);
        render_complete_serialize(profile, bytes, &mut output);
    }
    ClassPart {
        types,
        members: output,
    }
}

const COMPLETE_OUTPUT_TYPES: &str = r#"/// <summary>One complete physical run; Extras retain exact target declaration order.</summary>
public sealed record DocumentExecutionOutputs(DocumentBoundaryFormat PrimaryFormat, string Primary,
    global::System.Collections.Generic.IReadOnlyList<NamedDocumentOutput> Extras);
public sealed record DocumentBytesExecutionOutputs(DocumentBoundaryFormat PrimaryFormat, byte[] Primary,
    global::System.Collections.Generic.IReadOnlyList<NamedDocumentBytesOutput> Extras);

public abstract record DocumentBoundaryOwner
{
    private DocumentBoundaryOwner() { }
    public sealed record Source() : DocumentBoundaryOwner;
    public sealed record NamedSource(int DeclarationIndex, string Name) : DocumentBoundaryOwner;
    public sealed record Target() : DocumentBoundaryOwner;
    public sealed record NamedTarget(int DeclarationIndex, string Name) : DocumentBoundaryOwner;
}

/// <summary>Codec/encoding/resource failures retain their original typed inner cause.
/// Owner is null for whole-bundle count or output alignment failures. Mapping and
/// source-name errors keep the ordinary runtime exception contract.</summary>
public sealed class DocumentExecutionException : global::System.Exception
{
    internal DocumentExecutionException(DocumentBoundaryOwner? owner, global::System.Exception boundary)
        : base(owner is null ? boundary.Message : $"document endpoint {owner}: {boundary.Message}", boundary)
    { Owner = owner; Boundary = boundary; }
    public DocumentBoundaryOwner? Owner { get; }
    public global::System.Exception Boundary { get; }
}

public static class DocumentExecutionLimits
{
    public const int MaximumDocuments = 4096;
    public const long MaximumTotalUtf8Bytes = 256L * 1024 * 1024;
}

public sealed class DocumentBundleResourceException : global::System.Exception
{
    internal DocumentBundleResourceException(string resource, long observed, long limit)
        : base($"{resource} is {observed}; maximum is {limit}")
    { Resource = resource; Observed = observed; Limit = limit; }
    public string Resource { get; }
    public long Observed { get; }
    public long Limit { get; }
}

"#;

const COMPLETE_OUTPUT_HELPERS_START: &str = r#"    private static readonly global::System.Text.UTF8Encoding DocumentOutputsUtf8 = new(false, true);

    private static T DocumentBoundaryValue<T>(DocumentBoundaryOwner owner, global::System.Func<T> action)
    {
        try { return action(); }
        catch (global::System.Exception error) when (error is global::Ferrule.Runtime.FerruleRuntimeException
            or global::Ferrule.Runtime.FerruleX12Exception or global::System.Text.EncoderFallbackException
            or global::System.Text.DecoderFallbackException"#;

const COMPLETE_OUTPUT_HELPERS_END: &str = r#")
        { throw new DocumentExecutionException(owner, error); }
    }

    private sealed class DocumentBundleBudget
    {
        private long bytes;
        internal DocumentBundleBudget(long count)
        {
            if (count > DocumentExecutionLimits.MaximumDocuments)
                throw new DocumentExecutionException(null, new DocumentBundleResourceException(
                    "document_bundle_count", count, DocumentExecutionLimits.MaximumDocuments));
        }
        internal void Charge(DocumentBoundaryOwner owner, long count)
        {
            // All counts are measured nonnegative byte lengths, never caller counters.
            var observed = checked(bytes + count);
            if (observed > DocumentExecutionLimits.MaximumTotalUtf8Bytes)
                throw new DocumentExecutionException(owner, new DocumentBundleResourceException(
                    "document_bundle_utf8_bytes", observed, DocumentExecutionLimits.MaximumTotalUtf8Bytes));
            bytes = observed;
        }
    }

    private static DocumentExecutionException DocumentOutputsAlignment() => new(null,
        new global::System.InvalidOperationException("mapped document outputs do not match exact target declaration order"));
"#;

fn render_complete_api(bytes: bool, output: &mut String) {
    let (method, result, document, input, suffix) = if bytes {
        (
            "ExecuteDocumentBytesOutputs",
            "DocumentBytesExecutionOutputs",
            "byte[]",
            "NamedDocumentBytesInput",
            "Bytes",
        )
    } else {
        (
            "ExecuteDocumentOutputs",
            "DocumentExecutionOutputs",
            "string",
            "NamedDocumentInput",
            "",
        )
    };
    output.push_str(&format!(
        r#"
    public static {result} {method}({document} source)
        => {method}WithHost(source, global::System.Array.Empty<{input}>());

    public static {result} {method}WithHost(
        {document} source,
        global::System.Collections.Generic.IReadOnlyList<{input}> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext? executionContext = null)
    {{
        var parsed = ParseDocument{suffix}OutputsInputs(source, extraSources);
        var mapped = ExecuteOutputs(CreateContext(parsed.Primary, parsed.Inputs, executionContext));
        return SerializeDocument{suffix}Outputs(mapped, executionContext);
    }}
"#
    ));
}

fn render_complete_parse(
    profile: &StaticDocumentBoundaryProfile,
    bytes: bool,
    output: &mut String,
) {
    let (document, input, suffix) = if bytes {
        ("byte[]", "NamedDocumentBytesInput", "Bytes")
    } else {
        ("string", "NamedDocumentInput", "")
    };
    output.push_str(&format!(r#"
    private static (global::Ferrule.Runtime.FerruleInstance Primary,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> Inputs) ParseDocument{suffix}OutputsInputs(
        {document} source, global::System.Collections.Generic.IReadOnlyList<{input}> extraSources)
    {{
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(extraSources);
        var budget = new DocumentBundleBudget(1L + extraSources.Count);
        // Snapshot the named envelopes before validation and parsing. Caller documents
        // must not be mutated concurrently during this synchronous operation.
        var supplied = new {input}[extraSources.Count];
        for (var index = 0; index < supplied.Length; index++) supplied[index] = extraSources[index];
        ValidateNamedDocument{suffix}InputNames(supplied);
        var documents = new global::System.Collections.Generic.Dictionary<string, {document}>(global::System.StringComparer.Ordinal);
        foreach (var input in supplied)
        {{
            global::System.ArgumentNullException.ThrowIfNull(input.Document);
            documents.Add(input.Name, input.Document);
        }}
        var owner = new DocumentBoundaryOwner.Source();
"#));
    let primary_size = if bytes {
        "source.LongLength"
    } else {
        "DocumentBoundaryValue(owner, () => (long)DocumentOutputsUtf8.GetByteCount(source))"
    };
    output.push_str(&format!("        budget.Charge(owner, {primary_size});\n"));
    for (index, source) in profile.extra_sources.iter().enumerate() {
        let name = literal::string(&source.name);
        let size = if bytes {
            format!("document_{index}.LongLength")
        } else {
            format!(
                "DocumentBoundaryValue(owner_{index}, () => (long)DocumentOutputsUtf8.GetByteCount(document_{index}))"
            )
        };
        output.push_str(&format!(
            "        var owner_{index} = new DocumentBoundaryOwner.NamedSource({index}, {name});\n        var document_{index} = documents[{name}];\n        budget.Charge(owner_{index}, {size});\n"
        ));
    }
    let call = parsing(
        &profile.source,
        "DocumentSourceDescriptor",
        "source",
        suffix,
    );
    output.push_str(&format!("        var primary = DocumentBoundaryValue(owner, () => {call});\n        var parsed = new global::System.Collections.Generic.List<NamedInput>({});\n", profile.extra_sources.len()));
    for (index, source) in profile.extra_sources.iter().enumerate() {
        let name = literal::string(&source.name);
        let call = parsing(
            &source.boundary,
            &format!("ExtraDocumentSourceDescriptors[{index}]"),
            &format!("document_{index}"),
            suffix,
        );
        output.push_str(&format!("        parsed.Add(new NamedInput({name}, DocumentBoundaryValue(owner_{index}, () => {call})));\n"));
    }
    output.push_str("        return (primary, parsed);\n    }\n");
}

fn render_complete_serialize(
    profile: &StaticDocumentBoundaryProfile,
    bytes: bool,
    output: &mut String,
) {
    let (result, named_output, suffix) = if bytes {
        (
            "DocumentBytesExecutionOutputs",
            "NamedDocumentBytesOutput",
            "Bytes",
        )
    } else {
        ("DocumentExecutionOutputs", "NamedDocumentOutput", "")
    };
    let target_count = profile.extra_targets.len();
    output.push_str(&format!(
        r#"
    private static {result} SerializeDocument{suffix}Outputs(ExecutionOutputs mapped,
        global::Ferrule.Runtime.FerruleExecutionContext? executionContext)
    {{
        if (mapped.Extras.Count != {target_count}) throw DocumentOutputsAlignment();
"#
    ));
    for (index, target) in profile.extra_targets.iter().enumerate() {
        let name = literal::string(&target.name);
        output.push_str(&format!("        if (!global::System.String.Equals(mapped.Extras[{index}].Name, {name}, global::System.StringComparison.Ordinal))\n            throw DocumentOutputsAlignment();\n"));
    }
    output.push_str(&format!("        var budget = new DocumentBundleBudget(1L + {target_count});\n        var owner = new DocumentBoundaryOwner.Target();\n"));
    let call = serialization(
        &profile.target,
        "DocumentTargetDescriptor",
        "mapped.Primary",
        suffix,
    );
    output.push_str(&format!(
        "        var primary = DocumentBoundaryValue(owner, () => {call});\n"
    ));
    let primary_size = if bytes {
        "primary.LongLength"
    } else {
        "DocumentBoundaryValue(owner, () => (long)DocumentOutputsUtf8.GetByteCount(primary))"
    };
    output.push_str(&format!("        budget.Charge(owner, {primary_size});\n        var extras = new global::System.Collections.Generic.List<{named_output}>({target_count});\n"));
    for (index, target) in profile.extra_targets.iter().enumerate() {
        let name = literal::string(&target.name);
        let format = format_name(target.boundary.format);
        let descriptor = format!("ExtraDocumentTargetDescriptors[{index}]");
        let instance = format!("mapped.Extras[{index}].Instance");
        let call = serialization(&target.boundary, &descriptor, &instance, suffix);
        let size = if bytes {
            format!("document_{index}.LongLength")
        } else {
            format!(
                "DocumentBoundaryValue(owner_{index}, () => (long)DocumentOutputsUtf8.GetByteCount(document_{index}))"
            )
        };
        output.push_str(&format!("        var owner_{index} = new DocumentBoundaryOwner.NamedTarget({index}, {name});\n        var document_{index} = DocumentBoundaryValue(owner_{index}, () => {call});\n        budget.Charge(owner_{index}, {size});\n        extras.Add(new {named_output}({name}, DocumentBoundaryFormat.{format}, document_{index}));\n"));
    }
    let primary_format = format_name(profile.target.format);
    output.push_str(&format!("        return new {result}(DocumentBoundaryFormat.{primary_format}, primary, extras.AsReadOnly());\n    }}\n"));
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
    let source_call = parsing(
        &profile.source,
        "DocumentSourceDescriptor",
        "source",
        suffix,
    );
    let parse = if has_edifact(profile) {
        format!(
            "var parsed = ParseDocument{suffix}OutputsInputs(source, extraSources);\n        var parsedSource = parsed.Primary;\n        var parsedInputs = parsed.Inputs;"
        )
    } else {
        format!(
            "ValidateNamedDocument{suffix}InputNames(extraSources);\n        var parsedSource = {source_call};\n        var parsedInputs = ParseNamedDocument{suffix}Inputs(extraSources);"
        )
    };
    output.push_str(&format!(
        r#"
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
        {parse}
        var outcome = ExecuteSelectedTargetWithHost(
            parsedSource, selection, parsedInputs, executionContext, dynamicSourceLoader: null);
        return Serialize{result}(outcome, selected, executionContext);
    }}
"#
    ));
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
        let call = parsing(
            &source.boundary,
            &format!("ExtraDocumentSourceDescriptors[{index}]"),
            "extraSource.Document",
            suffix,
        );
        output.push_str(&format!("                {name} => {call},\n"));
    }
    output.push_str("                _ => throw new global::System.InvalidOperationException(\"Preflighted document source is invalid.\"),\n            };\n            parsed.Add(new NamedInput(extraSource.Name, instance));\n        }\n        return parsed;\n    }\n");
}

fn serialization(
    boundary: &DocumentBoundaryDescriptor,
    descriptor: &str,
    instance: &str,
    suffix: &str,
) -> String {
    if boundary.format == DocumentBoundaryFormat::Edifact {
        let options = literal::string(&boundary.options_descriptor);
        let call = format!(
            "new global::Ferrule.Runtime.FerruleEdi({descriptor}, {options}, \"Edifact\", strictEnvelope: true).Serialize({instance}, executionContext?.CurrentDateTime)"
        );
        return if suffix == "Bytes" {
            call
        } else {
            format!("DocumentOutputsUtf8.GetString({call})")
        };
    }
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

fn parsing(
    boundary: &DocumentBoundaryDescriptor,
    descriptor: &str,
    document: &str,
    suffix: &str,
) -> String {
    if boundary.format == DocumentBoundaryFormat::Edifact {
        let options = literal::string(&boundary.options_descriptor);
        let bytes = if suffix == "Bytes" {
            document.to_owned()
        } else {
            format!("DocumentOutputsUtf8.GetBytes({document})")
        };
        format!(
            "new global::Ferrule.Runtime.FerruleEdi({descriptor}, {options}, \"Edifact\", input: true, strictEnvelope: true).Parse({bytes})"
        )
    } else {
        format!(
            "global::Ferrule.Runtime.{}.ParseEmbedded{suffix}({descriptor}, {document})",
            codec(boundary.format)
        )
    }
}

fn render_selected_serialize(
    profile: &StaticDocumentBoundaryProfile,
    bytes: bool,
    output: &mut String,
) {
    let wrap_boundary = has_edifact(profile);
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
    let primary_call = if wrap_boundary {
        format!("DocumentBoundaryValue(new DocumentBoundaryOwner.Target(), () => {primary_call})")
    } else {
        primary_call
    };
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
        let call = if wrap_boundary {
            format!(
                "DocumentBoundaryValue(new DocumentBoundaryOwner.NamedTarget({index}, {name}), () => {call})"
            )
        } else {
            call
        };
        output.push_str(&format!(r#"            case {selected} when outcome is SelectedTargetOutput.Named named_{selected} &&
                global::System.String.Equals(named_{selected}.Output.Name, {name}, global::System.StringComparison.Ordinal):
                return new {result}.Named(new {named_output}(
                    named_{selected}.Output.Name, DocumentBoundaryFormat.{target_format}, {call}));
"#));
    }
    output.push_str("            default:\n                throw new global::Ferrule.Runtime.FerruleRuntimeException(\n                    global::Ferrule.Runtime.FerruleRuntimeError.JsonBoundary,\n                    \"generated mapping returned a target that does not match the resolved selection\");\n        }\n    }\n");
}

#[cfg(test)]
mod tests;
