pub(crate) const OUTPUT_TYPES: &str = r#"public enum DocumentBoundaryFormat { Json, X12 }

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
