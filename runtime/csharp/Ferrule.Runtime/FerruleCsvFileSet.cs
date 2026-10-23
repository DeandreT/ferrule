namespace Ferrule.Runtime;

/// <summary>Complete physical-file acquisition, retaining empty files and row provenance.
/// This supplies an execution view only; it neither maps nor publishes output.</summary>
public static class FerruleCsvFileSet
{
    public static FerruleInstance Acquire(
        Func<string> filename, Func<string, IEnumerable<string>> select,
        Func<string, FerruleCsvSourceData> load, IReadOnlyList<FerruleCsvReadField> fields,
        string rowName, FerruleCsvReadOptions options, FerruleCsvByteDecodingPolicy decoding, int maximum)
    {
        ArgumentNullException.ThrowIfNull(filename);
        ArgumentNullException.ThrowIfNull(select);
        ArgumentNullException.ThrowIfNull(load);
        ArgumentNullException.ThrowIfNull(fields);
        if (string.IsNullOrEmpty(rowName) || rowName == "Path")
            throw new FerruleCsvAcquisitionException(FerruleCsvAcquisitionError.InvalidFileSetSchema);
        if (maximum is < 0 or > FerruleCsvSourceRows.MaximumDocuments)
            throw new FerruleCsvAcquisitionException(FerruleCsvAcquisitionError.InvalidLimit, maximum: maximum);
        static T At<T>(FerruleCsvAcquisitionError error, Func<T> action, string? path = null)
        {
            try { return action(); }
            catch (Exception source) { throw new FerruleCsvAcquisitionException(error, path, source: source); }
        }
        var pattern = At(FerruleCsvAcquisitionError.Filename, filename);
        var selected = At(FerruleCsvAcquisitionError.Selection, () => select(pattern)
            ?? throw new InvalidOperationException("CSV selector returned no sequence."));
        using var requests = At(FerruleCsvAcquisitionError.Request, selected.GetEnumerator);
        var files = new List<FerruleInstance>();
        long totalBytes = 0, totalRows = 0;
        while (At(FerruleCsvAcquisitionError.Request, requests.MoveNext))
        {
            string path = At(FerruleCsvAcquisitionError.Request, () => requests.Current);
            if (files.Count == maximum)
                throw new FerruleCsvAcquisitionException(FerruleCsvAcquisitionError.Documents, maximum: maximum);
            At(FerruleCsvAcquisitionError.Input, () => { FerruleCsvSourceDocument.ValidatePath(path, nameof(path)); return true; }, path);
            var data = At(FerruleCsvAcquisitionError.Load, () => load(path)
                ?? throw new InvalidOperationException("CSV loader returned no bytes."), path);
            var bytes = At(FerruleCsvAcquisitionError.Input, () => data.Bytes
                ?? throw new ArgumentNullException(nameof(data.Bytes)), path);
            totalBytes += bytes.LongLength + System.Text.Encoding.UTF8.GetByteCount(path) +
                (data.ResolvedPath is null ? 0 : System.Text.Encoding.UTF8.GetByteCount(data.ResolvedPath));
            if (totalBytes > 64 * 1024 * 1024)
                throw new FerruleCsvAcquisitionException(FerruleCsvAcquisitionError.FileSetBytes);
            var document = At(FerruleCsvAcquisitionError.Input, () => FerruleCsvSourceDocument.ParseBytes(
                path, data.ResolvedPath, bytes, fields, decoding, options), path);
            totalRows += document.Count;
            if (totalRows > 1_000_000)
                throw new FerruleCsvAcquisitionException(FerruleCsvAcquisitionError.FileSetRows);
            files.Add(new FerruleGroup(new FerruleField[] {
                new("Path", new FerruleScalar(FerruleValue.FromString(document.SourcePath))),
                new(rowName, new FerruleRepeated(document.Rows().Select(row => row.Value))),
            }));
        }
        return new FerruleGroup(new FerruleField[] { new("Files", new FerruleRepeated(files)) });
    }
}
