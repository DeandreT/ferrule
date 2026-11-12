using System.Text;

namespace Ferrule.Runtime;

public sealed record FerruleCsvBytesDocument(string Path, byte[] Bytes);
public sealed record FerruleCsvTextDocument(string Path, string Text);

public enum FerruleCsvDocumentsError { RootShape, TooManyDocuments, PathTooLong, Document, InvalidRowsField, DocumentShape, MappedConfiguration }

public sealed class FerruleCsvDocumentsException : Exception
{
    internal FerruleCsvDocumentsException(FerruleCsvDocumentsError error, string message,
        int? document = null, string? path = null, Exception? inner = null) : base(message, inner)
    { Error = error; Document = document; Path = path; }

    public FerruleCsvDocumentsError Error { get; }
    public int? Document { get; }
    public string? Path { get; }
}

/// <summary>One bounded, all-or-nothing list of path-bearing CSV documents.</summary>
public static class FerruleCsvDocuments
{
    public const int MaximumDocuments = 4096;
    public const int MaximumPathBytes = 4096;
    public const int MaximumTotalBytes = 256 * 1024 * 1024;
    private static readonly UTF8Encoding Utf8 = new(false, true);

    /// <summary>Lower budgets are allowed; callers cannot raise the global limit.</summary>
    public static IReadOnlyList<FerruleCsvBytesDocument> SerializeBytes(FerruleInstance value,
        IReadOnlyList<FerruleCsvField> fields, FerruleCsvWriteOptions? options = null,
        int maximumTotalBytes = MaximumTotalBytes)
        => SerializeBytesCore(value, fields, options, maximumTotalBytes, null);

    /// <summary>Borrow the sole repeated field of every constructed wrapper.
    /// Logical paths are preserved; this API does not publish files.</summary>
    public static IReadOnlyList<FerruleCsvBytesDocument> SerializeMappedBytes(FerruleInstance value,
        string rowsField, IReadOnlyList<FerruleCsvField> fields, FerruleCsvWriteOptions? options = null,
        int maximumTotalBytes = MaximumTotalBytes)
    {
        if (string.IsNullOrEmpty(rowsField) || rowsField.Contains('\0'))
            throw new FerruleCsvDocumentsException(FerruleCsvDocumentsError.InvalidRowsField,
                "CSV mapped document row field must be nonempty and NUL-free.");
        return SerializeBytesCore(value, fields, options, maximumTotalBytes, rowsField);
    }

    private static IReadOnlyList<FerruleCsvBytesDocument> SerializeBytesCore(FerruleInstance value,
        IReadOnlyList<FerruleCsvField> fields, FerruleCsvWriteOptions? options,
        int maximumTotalBytes, string? rowsField)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(maximumTotalBytes);
        if (value is not FerruleDocumentSet documents)
            throw new FerruleCsvDocumentsException(FerruleCsvDocumentsError.RootShape,
                "CSV document output requires a document set.");
        if (documents.Documents.Count > MaximumDocuments)
            throw new FerruleCsvDocumentsException(FerruleCsvDocumentsError.TooManyDocuments,
                "CSV document output exceeds the document-count limit.");
        for (var index = 0; index < documents.Documents.Count; index++)
        {
            if (Utf8.GetByteCount(documents.Documents[index].Path) > MaximumPathBytes)
                throw new FerruleCsvDocumentsException(FerruleCsvDocumentsError.PathTooLong,
                    "CSV document path exceeds the UTF-8 byte limit.", index, documents.Documents[index].Path);
        }
        if (rowsField is not null)
        {
            // Descriptors carry only scalar types, never constraints. Generated
            // admission proves canonical metadata. Validate public descriptors
            // and dialect even for zero documents, without header precedence.
            try
            {
                if (fields.Count is < 1 or > 256)
                    throw new FerruleCsvException(FerruleCsvError.UnsupportedSchema,
                        "Mapped CSV output requires between 1 and 256 columns.");
                FerruleCsv.ValidateFields(fields);
                if (fields.Any(field => string.IsNullOrEmpty(field.Name)) ||
                    fields.Sum(field => (long)Utf8.GetByteCount(field.Name)) > 1024 * 1024 ||
                    options?.RawDelimitedCrLf == true)
                    throw new FerruleCsvException(FerruleCsvError.UnsupportedSchema,
                        "Mapped CSV output requires canonical scalar fields and ordinary CSV settings.");
                FerruleCsv.SerializeBytesBounded(new FerruleRepeated(Array.Empty<FerruleInstance>()), fields,
                    (options ?? new FerruleCsvWriteOptions()) with { HasHeaders = false, Utf8Bom = false }, 0);
            }
            catch (FerruleCsvException error)
            {
                throw new FerruleCsvDocumentsException(FerruleCsvDocumentsError.MappedConfiguration,
                    error.Message, inner: error);
            }
        }
        var remaining = Math.Min(maximumTotalBytes, MaximumTotalBytes);
        var output = new List<FerruleCsvBytesDocument>(documents.Documents.Count);
        for (var index = 0; index < documents.Documents.Count; index++)
        {
            var document = documents.Documents[index];
            var selected = document.Value;
            if (rowsField is not null)
            {
                if (selected is not FerruleGroup wrapper || wrapper.Fields.Count != 1 ||
                    wrapper.Fields[0].Name != rowsField || wrapper.Fields[0].Value is not FerruleRepeated)
                    throw new FerruleCsvDocumentsException(FerruleCsvDocumentsError.DocumentShape,
                        $"CSV document {index} at {document.Path} requires exactly one repeated field {rowsField}.",
                        index, document.Path);
                selected = wrapper.Fields[0].Value;
            }
            byte[] bytes;
            try { bytes = FerruleCsv.SerializeBytesBounded(selected, fields, options, remaining); }
            catch (FerruleCsvException error)
            {
                throw new FerruleCsvDocumentsException(FerruleCsvDocumentsError.Document,
                    $"CSV document {index} at {document.Path}: {error.Message}", index, document.Path, error);
            }
            remaining -= bytes.Length;
            output.Add(new FerruleCsvBytesDocument(document.Path, bytes));
        }
        return output.AsReadOnly();
    }

    public static IReadOnlyList<FerruleCsvTextDocument> Serialize(FerruleInstance value,
        IReadOnlyList<FerruleCsvField> fields, FerruleCsvWriteOptions? options = null,
        int maximumTotalBytes = MaximumTotalBytes) => SerializeBytes(value, fields, options, maximumTotalBytes)
            .Select(document => new FerruleCsvTextDocument(document.Path, Utf8.GetString(document.Bytes)))
            .ToArray();

    public static IReadOnlyList<FerruleCsvTextDocument> SerializeMapped(FerruleInstance value,
        string rowsField, IReadOnlyList<FerruleCsvField> fields, FerruleCsvWriteOptions? options = null,
        int maximumTotalBytes = MaximumTotalBytes) => SerializeMappedBytes(value, rowsField, fields, options, maximumTotalBytes)
            .Select(document => new FerruleCsvTextDocument(document.Path, Utf8.GetString(document.Bytes)))
            .ToArray();
}
