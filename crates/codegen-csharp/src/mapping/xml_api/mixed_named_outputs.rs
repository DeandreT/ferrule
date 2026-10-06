use super::output_arguments;
use crate::{EmitError, literal};
use codegen::{Program, ProgramValidationError, XmlOutputMode};

pub(super) const TYPES: &str = r#"public sealed record XmlMixedDocumentOutput(string Path, string Document);
public sealed record XmlMixedBytesDocumentOutput(string Path, byte[] Document);
public abstract record NamedXmlMixedOutput
{
    private NamedXmlMixedOutput() { }
    public sealed record SingleDocument(int DeclarationIndex, string Name, string Document) : NamedXmlMixedOutput;
    public sealed record DocumentList(int DeclarationIndex, string Name, global::System.Collections.Generic.IReadOnlyList<XmlMixedDocumentOutput> Documents) : NamedXmlMixedOutput;
}
public abstract record NamedXmlMixedBytesOutput
{
    private NamedXmlMixedBytesOutput() { }
    public sealed record SingleDocument(int DeclarationIndex, string Name, byte[] Document) : NamedXmlMixedBytesOutput;
    public sealed record DocumentList(int DeclarationIndex, string Name, global::System.Collections.Generic.IReadOnlyList<XmlMixedBytesDocumentOutput> Documents) : NamedXmlMixedBytesOutput;
}
public sealed record XmlMixedExecutionOutputs(string Primary, global::System.Collections.Generic.IReadOnlyList<NamedXmlMixedOutput> Extras);
public sealed record XmlMixedBytesExecutionOutputs(byte[] Primary, global::System.Collections.Generic.IReadOnlyList<NamedXmlMixedBytesOutput> Extras);
public abstract record XmlMixedOutputOwner
{
    private XmlMixedOutputOwner() { }
    public sealed record Primary() : XmlMixedOutputOwner;
    public sealed record Named(int DeclarationIndex, string Name) : XmlMixedOutputOwner;
    public sealed record Member(int DeclarationIndex, string Name, int MemberIndex, string Path) : XmlMixedOutputOwner;
}
public sealed class XmlMixedExecutionException : global::System.Exception
{
    public XmlMixedExecutionException(XmlMixedOutputOwner? owner, global::Ferrule.Runtime.FerruleXmlBoundaryException boundary)
        : base(MessageFor(owner, boundary), boundary)
    { Owner = owner; Boundary = boundary; }
    public XmlMixedOutputOwner? Owner { get; }
    public global::Ferrule.Runtime.FerruleXmlBoundaryException Boundary { get; }
    private static string MessageFor(XmlMixedOutputOwner? owner, global::Ferrule.Runtime.FerruleXmlBoundaryException boundary)
    {
        global::System.ArgumentNullException.ThrowIfNull(boundary);
        return owner switch
        {
            XmlMixedOutputOwner.Primary => $"primary XML output: {boundary.Message}",
            XmlMixedOutputOwner.Named named => $"named XML output {named.DeclarationIndex} `{named.Name}`: {boundary.Message}",
            XmlMixedOutputOwner.Member member => $"named XML output {member.DeclarationIndex} `{member.Name}` member {member.MemberIndex} `{member.Path}`: {boundary.Message}",
            null => boundary.Message,
            _ => throw new global::System.ArgumentException("Unknown mixed XML output owner.", nameof(owner)),
        };
    }
}

"#;

pub(super) fn render(program: &Program, output: &mut String) -> Result<(), EmitError> {
    if program.xml_output_mode()? != Some(XmlOutputMode::StaticPrimaryMixedNamedXmlOutputs) {
        return Err(ProgramValidationError::InvalidXmlBoundary {
            reason: "mixed named XML renderer requires its exact validated mode".into(),
        }
        .into());
    }
    let policy = program.xml_boundary.as_ref().ok_or_else(|| {
        ProgramValidationError::InvalidXmlBoundary {
            reason: "missing mixed XML boundary".into(),
        }
    })?;
    let source = codegen::serialize_embedded_schema(
        &program.source,
        codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
    )?;
    let target = codegen::serialize_embedded_schema(
        &program.target,
        codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
    )?;
    output.push_str(&format!(
        "\n    private const string SourceXmlSchema = {};\n    private const string TargetXmlSchema = {};\n",
        literal::string(&source), literal::string(&target),
    ));
    for (index, named) in program.extra_targets.iter().enumerate() {
        let descriptor = codegen::serialize_embedded_schema(
            &named.target,
            codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
        )?;
        output.push_str(&format!(
            "    private const string MixedXmlSchema{index} = {};\n    private const string MixedXmlName{index} = {};\n",
            literal::string(&descriptor), literal::string(&named.name),
        ));
    }
    output.push_str(r#"    private static readonly global::System.Text.UTF8Encoding MixedXmlUtf8 = new(false, true);
    private static XmlMixedExecutionException MixedXmlAlignment(string detail) =>
        new(null, global::Ferrule.Runtime.FerruleXmlOutputSetBudget.Alignment(detail).Boundary);
    private static XmlMixedExecutionException MixedXmlError(XmlMixedOutputOwner owner, global::Ferrule.Runtime.FerruleXmlBoundaryException boundary) =>
        new(boundary.Kind == global::Ferrule.Runtime.FerruleXmlBoundaryErrorKind.Schema ? null : owner, boundary);
"#);
    let dynamic_index = program
        .extra_targets
        .iter()
        .position(|target| {
            target
                .root
                .iteration
                .as_ref()
                .is_some_and(|iteration| iteration.dynamic_document_iteration().is_some())
        })
        .ok_or_else(|| ProgramValidationError::InvalidXmlBoundary {
            reason: "missing mixed XML document list".into(),
        })?;
    let primary_arguments = output_arguments(&policy.output)?;
    for bytes in [false, true] {
        let (name, source_type, parser, result, named_dto, member_dto, helper, conversion) =
            if bytes {
                (
                    "ExecuteXmlBytesMixedOutputs",
                    "byte[]",
                    "ParseStructuredEmbeddedBytes",
                    "XmlMixedBytesExecutionOutputs",
                    "NamedXmlMixedBytesOutput",
                    "XmlMixedBytesDocumentOutput",
                    "SerializeMixedXmlBytesOutputs",
                    "MixedXmlUtf8.GetBytes(xml)",
                )
            } else {
                (
                    "ExecuteXmlMixedOutputs",
                    "string",
                    "ParseStructuredEmbedded",
                    "XmlMixedExecutionOutputs",
                    "NamedXmlMixedOutput",
                    "XmlMixedDocumentOutput",
                    "SerializeMixedXmlOutputs",
                    "xml",
                )
            };
        for context in [false, true] {
            let context_argument = if context {
                ", global::Ferrule.Runtime.FerruleExecutionContext executionContext"
            } else {
                ""
            };
            let context_call = if context { ", executionContext" } else { "" };
            output.push_str(&format!(r#"
    public static {result} {name}({source_type} source{context_argument})
    {{
        global::Ferrule.Runtime.FerruleInstance parsed;
        ExecutionOutputs mapped;
        try
        {{
            parsed = global::Ferrule.Runtime.FerruleXml.{parser}(SourceXmlSchema, source);
            try {{ mapped = ExecuteOutputs(parsed{context_call}); }}
            catch (global::Ferrule.Runtime.FerruleRuntimeException error)
            {{
                throw new global::Ferrule.Runtime.FerruleXmlBoundaryException(
                    global::Ferrule.Runtime.FerruleXmlBoundaryErrorKind.Mapping, error.Message, error);
            }}
        }}
        catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)
        {{ throw new XmlMixedExecutionException(null, error); }}
        return {helper}(mapped);
    }}
"#));
        }
        output.push_str(&format!(
            r#"
    private static {result} {helper}(ExecutionOutputs mapped)
    {{
        if (mapped.Primary is not global::Ferrule.Runtime.FerruleGroup)
            throw MixedXmlAlignment("mixed XML outputs require a primary Group");
        if (mapped.Extras.Count != 2)
            throw MixedXmlAlignment("mixed XML outputs require every declared named target");
"#
        ));
        // Align all declaration envelopes before count arithmetic or serialization.
        for index in 0..2 {
            output.push_str(&format!(
                r#"        if (mapped.Extras[{index}].Name != MixedXmlName{index})
            throw MixedXmlAlignment("mixed XML outputs do not match exact declaration order");
"#
            ));
            if index == dynamic_index {
                output.push_str(&format!(r#"        if (mapped.Extras[{index}].Instance is not global::Ferrule.Runtime.FerruleDocumentSet members{index})
            throw MixedXmlAlignment("mixed XML document-list output requires a DocumentSet");
"#));
            } else {
                output.push_str(&format!(r#"        if (mapped.Extras[{index}].Instance is not global::Ferrule.Runtime.FerruleGroup)
            throw MixedXmlAlignment("mixed XML single-document output requires a Group");
"#));
            }
        }
        output.push_str(&format!(r#"        if (members{dynamic_index}.Documents.Count > global::System.Int32.MaxValue - 2)
            throw MixedXmlAlignment("mixed XML output count exceeds the host index range");
        var artifactCount = checked(members{dynamic_index}.Documents.Count + 2);
        global::Ferrule.Runtime.FerruleXmlOutputSetBudget budget;
        try {{ budget = new global::Ferrule.Runtime.FerruleXmlOutputSetBudget(artifactCount); }}
        catch (global::Ferrule.Runtime.FerruleXmlOutputSetException error)
        {{ throw new XmlMixedExecutionException(null, error.Boundary); }}
        string xml;
        try {{ xml = global::Ferrule.Runtime.FerruleXml.SerializeDocumentEmbedded(TargetXmlSchema, mapped.Primary, {primary_arguments}); }}
        catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)
        {{ throw MixedXmlError(new XmlMixedOutputOwner.Primary(), error); }}
        try {{ budget.Charge(global::Ferrule.Runtime.FerruleXmlOutputTarget.Primary, MixedXmlUtf8.GetByteCount(xml)); }}
        catch (global::Ferrule.Runtime.FerruleXmlOutputSetException error)
        {{ throw MixedXmlError(new XmlMixedOutputOwner.Primary(), error.Boundary); }}
        var primary = {conversion};
        var extras = new global::System.Collections.Generic.List<{named_dto}>(2);
"#));
        for (index, named_policy) in policy.extra_outputs.iter().enumerate() {
            let arguments = output_arguments(&named_policy.output)?;
            if index == dynamic_index {
                output.push_str(&format!(r#"        var documents = new global::System.Collections.Generic.List<{member_dto}>(members{index}.Documents.Count);
        for (var memberIndex = 0; memberIndex < members{index}.Documents.Count; memberIndex++)
        {{
            var member = members{index}.Documents[memberIndex];
            try {{ xml = global::Ferrule.Runtime.FerruleXml.SerializeDocumentEmbedded(MixedXmlSchema{index}, member.Value, {arguments}); }}
            catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)
            {{ throw MixedXmlError(new XmlMixedOutputOwner.Member({index}, MixedXmlName{index}, memberIndex, member.Path), error); }}
            try {{ budget.Charge(global::Ferrule.Runtime.FerruleXmlOutputTarget.Named({index}, MixedXmlName{index}), MixedXmlUtf8.GetByteCount(xml)); }}
            catch (global::Ferrule.Runtime.FerruleXmlOutputSetException error)
            {{ throw MixedXmlError(new XmlMixedOutputOwner.Member({index}, MixedXmlName{index}, memberIndex, member.Path), error.Boundary); }}
            documents.Add(new {member_dto}(member.Path, {conversion}));
        }}
        extras.Add(new {named_dto}.DocumentList({index}, MixedXmlName{index}, documents.AsReadOnly()));
"#));
            } else {
                output.push_str(&format!(r#"        try {{ xml = global::Ferrule.Runtime.FerruleXml.SerializeDocumentEmbedded(MixedXmlSchema{index}, mapped.Extras[{index}].Instance, {arguments}); }}
        catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)
        {{ throw MixedXmlError(new XmlMixedOutputOwner.Named({index}, MixedXmlName{index}), error); }}
        try {{ budget.Charge(global::Ferrule.Runtime.FerruleXmlOutputTarget.Named({index}, MixedXmlName{index}), MixedXmlUtf8.GetByteCount(xml)); }}
        catch (global::Ferrule.Runtime.FerruleXmlOutputSetException error)
        {{ throw MixedXmlError(new XmlMixedOutputOwner.Named({index}, MixedXmlName{index}), error.Boundary); }}
        extras.Add(new {named_dto}.SingleDocument({index}, MixedXmlName{index}, {conversion}));
"#));
            }
        }
        output.push_str(&format!(
            "        return new {result}(primary, extras.AsReadOnly());\n    }}\n"
        ));
    }
    Ok(())
}
