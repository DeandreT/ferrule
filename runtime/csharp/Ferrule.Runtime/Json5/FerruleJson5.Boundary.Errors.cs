namespace Ferrule.Runtime;

/// <summary>The stage that refused an explicit JSON5 companion call.</summary>
public enum FerruleJson5Stage { Encoding, Schema, Syntax, Input, Mapping, Output }
public enum FerruleJson5SchemaSide { Source, Target }
public enum FerruleJson5Resource
{
    OriginalDocumentBytes, NormalizedDocumentBytes, SyntaxDepth, SyntaxWork,
    EmbeddedSchemaBytes, OutputDocumentBytes,
}
public enum FerruleJson5SchemaKind
{
    RootObjectRequired, UnsupportedMetadata, DuplicateChildName,
    InvalidRequiredName, UnknownDescriptorField, DuplicateDescriptorField,
    InvalidDescriptorShape, Limit,
}
public enum FerruleJson5SchemaResource
{
    DescriptorBytes, SchemaNodes, LogicalLevels, NameLength, NameBytes,
}

/// <summary>Optional-profile refusal; ordinary JSON codec errors are unchanged.</summary>
public sealed class FerruleJson5SchemaException : Exception
{
    internal FerruleJson5SchemaException(
        FerruleJson5SchemaKind kind, string? field = null, string? name = null,
        FerruleJson5SchemaResource? resource = null, long? requested = null,
        long? maximum = null)
        : base($"JSON5 schema profile refused {kind}.")
    {
        Kind = kind; Field = field; Name = name; Resource = resource;
        Requested = requested; Maximum = maximum;
    }
    public FerruleJson5SchemaKind Kind { get; }
    public string? Field { get; }
    public string? Name { get; }
    public FerruleJson5SchemaResource? Resource { get; }
    public long? Requested { get; }
    public long? Maximum { get; }
}

public sealed class FerruleJson5ResourceException : Exception
{
    internal FerruleJson5ResourceException(FerruleJson5Resource resource, long requested, long maximum)
        : base($"JSON5 {resource} requested {requested}, maximum {maximum}.")
    { Resource = resource; Requested = requested; Maximum = maximum; }
    public FerruleJson5Resource Resource { get; }
    public long Requested { get; }
    public long Maximum { get; }
}

/// <summary>Stage identity plus the original, unclassified exception chain.</summary>
public sealed class FerruleJson5BoundaryException : Exception
{
    internal FerruleJson5BoundaryException(
        FerruleJson5Stage stage, Exception cause, FerruleJson5SchemaSide? side = null)
        : base($"JSON5 {stage} boundary failed.", cause)
    {
        Stage = stage; SchemaSide = side;
        if (cause is FerruleJson5ResourceException limit)
        { Resource = limit.Resource; Requested = limit.Requested; Maximum = limit.Maximum; }
        if (cause is FerruleJson5SyntaxException syntax)
        {
            SyntaxKind = syntax.Kind; Utf8Offset = syntax.Offset; Utf16Index = syntax.Utf16Index;
            Requested = syntax.Requested; Maximum = syntax.Maximum;
            Resource = syntax.Resource switch
            {
                FerruleJson5SyntaxResource.OriginalDocumentBytes => FerruleJson5Resource.OriginalDocumentBytes,
                FerruleJson5SyntaxResource.NormalizedDocumentBytes => FerruleJson5Resource.NormalizedDocumentBytes,
                FerruleJson5SyntaxResource.ContainerDepth => FerruleJson5Resource.SyntaxDepth,
                FerruleJson5SyntaxResource.SyntaxWork => FerruleJson5Resource.SyntaxWork,
                _ => null,
            };
        }
        if (cause is System.Text.DecoderFallbackException decoder) { Utf8Offset = decoder.Index; }
        if (cause is FerruleJson5SchemaException schema &&
            schema.Resource == FerruleJson5SchemaResource.DescriptorBytes)
        { Resource = FerruleJson5Resource.EmbeddedSchemaBytes; Requested = schema.Requested; Maximum = schema.Maximum; }
    }
    public FerruleJson5Stage Stage { get; }
    public FerruleJson5SchemaSide? SchemaSide { get; }
    public FerruleJson5Resource? Resource { get; }
    public FerruleJson5SyntaxKind? SyntaxKind { get; }
    public long? Requested { get; }
    public long? Maximum { get; }
    /// <summary>Absolute offset in the original UTF-8 input; not an inner encoder span index.</summary>
    public long? Utf8Offset { get; }
    /// <summary>Absolute input-text code-unit index; an inner encoder can own a different span index.</summary>
    public int? Utf16Index { get; }
}
