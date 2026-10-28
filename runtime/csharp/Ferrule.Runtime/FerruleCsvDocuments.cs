using System.Text;

namespace Ferrule.Runtime;

public sealed record FerruleCsvBytesDocument(string Path, byte[] Bytes);
public sealed record FerruleCsvTextDocument(string Path, string Text);

public enum FerruleCsvDocumentsError { RootShape, TooManyDocuments, PathTooLong, Document }

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
        var remaining = Math.Min(maximumTotalBytes, MaximumTotalBytes);
        var output = new List<FerruleCsvBytesDocument>(documents.Documents.Count);
        for (var index = 0; index < documents.Documents.Count; index++)
        {
            var document = documents.Documents[index];
            byte[] bytes;
            try { bytes = FerruleCsv.SerializeBytesBounded(document.Value, fields, options, remaining); }
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
}
