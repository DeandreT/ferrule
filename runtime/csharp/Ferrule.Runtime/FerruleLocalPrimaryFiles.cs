namespace Ferrule.Runtime;

public enum FerruleLocalFileError
{
    Root, Uri, Rooted, Parent, InvalidName, WildcardDirectory, NoMatches, Missing, Symlink, NotRegular, TooLarge, Io,
}

/// <summary>
/// A local file refusal. Subject is the URI scheme for Uri, the filename for filename
/// refusals, and the normalized request or directory path for file refusals.
/// </summary>
public sealed class FerruleLocalFileException : Exception
{
    public FerruleLocalFileException(FerruleLocalFileError error, string subject, long? limit = null,
        Exception? source = null)
        : base($"local primary file {error}: {subject}" + (limit is null ? "" : $" ({limit} bytes)"), source)
    {
        Error = error;
        Subject = subject;
        Limit = limit;
    }

    public FerruleLocalFileError Error { get; }
    public string Subject { get; }
    public long? Limit { get; }
}

/// <summary>
/// An explicit local filesystem host for primary documents. Filenames are relative paths
/// beneath one root: '/' and '\' both separate components, and empty and '.' components
/// are ignored. URI schemes, rooted or drive paths, parent components and colons are
/// refused, so no network or other volume is reached. Every component beneath the root
/// must be a real directory or file, not a symbolic link; these checks are not atomic
/// against concurrent changes. Wildcard matches are case-sensitive and sorted by
/// Unicode scalar values.
/// </summary>
public sealed class FerruleLocalPrimaryFiles : IFerrulePrimarySourceHost
{
    public const long DefaultMaximumDocumentBytes = 8 * 1024 * 1024;

    private readonly string _root;
    private readonly long _maximumDocumentBytes;

    public FerruleLocalPrimaryFiles(string root, long maximumDocumentBytes = DefaultMaximumDocumentBytes)
    {
        ArgumentNullException.ThrowIfNull(root);
        ArgumentOutOfRangeException.ThrowIfNegative(maximumDocumentBytes);
        string full;
        try { full = Path.GetFullPath(root); }
        catch (Exception source) when (source is ArgumentException or IOException or NotSupportedException)
        { throw new FerruleLocalFileException(FerruleLocalFileError.Root, root, source: source); }
        if (!Directory.Exists(full)) throw new FerruleLocalFileException(FerruleLocalFileError.Root, root);
        _root = full;
        _maximumDocumentBytes = maximumDocumentBytes;
    }

    public string Root => _root;

    internal static string[] Components(string filename)
    {
        ArgumentNullException.ThrowIfNull(filename);
        if (filename.Length == 0 || filename.Contains('\0'))
            throw new FerruleLocalFileException(FerruleLocalFileError.InvalidName, filename);
        int colon = filename.IndexOf(':');
        if (colon >= 0)
        {
            string prefix = filename[..colon];
            bool scheme = prefix.Length > 0 && char.IsAsciiLetter(prefix[0]) &&
                prefix.Skip(1).All(c => char.IsAsciiLetterOrDigit(c) || c is '+' or '-' or '.');
            throw scheme && prefix.Length == 1
                ? new FerruleLocalFileException(FerruleLocalFileError.Rooted, filename)
                : scheme
                    ? new FerruleLocalFileException(FerruleLocalFileError.Uri, prefix)
                    : new FerruleLocalFileException(FerruleLocalFileError.InvalidName, filename);
        }
        if (filename[0] is '/' or '\\') throw new FerruleLocalFileException(FerruleLocalFileError.Rooted, filename);
        var parts = filename.Split('/', '\\').Where(part => part is not ("" or ".")).ToArray();
        if (parts.Contains("..")) throw new FerruleLocalFileException(FerruleLocalFileError.Parent, filename);
        if (parts.Length == 0) throw new FerruleLocalFileException(FerruleLocalFileError.InvalidName, filename);
        return parts;
    }

    private static int[] Scalars(string text) => text.EnumerateRunes().Select(rune => rune.Value).ToArray();

    internal static bool WildcardMatches(string pattern, string name)
    {
        int[] value = Scalars(name);
        var previous = new bool[value.Length + 1];
        previous[0] = true;
        foreach (int token in Scalars(pattern))
        {
            var current = new bool[value.Length + 1];
            current[0] = token == '*' && previous[0];
            for (int index = 1; index <= value.Length; index++)
                current[index] = token switch
                {
                    '*' => previous[index] || current[index - 1],
                    '?' => previous[index - 1],
                    _ => previous[index - 1] && token == value[index - 1],
                };
            previous = current;
        }
        return previous[value.Length];
    }

    private static int CompareScalars(string left, string right)
    {
        int[] a = Scalars(left), b = Scalars(right);
        for (int index = 0; index < Math.Min(a.Length, b.Length); index++)
            if (a[index] != b[index]) return a[index].CompareTo(b[index]);
        return a.Length.CompareTo(b.Length);
    }

    private static bool IsUnicode(string name)
    {
        for (int index = 0; index < name.Length; index++)
        {
            if (char.IsHighSurrogate(name[index]) && index + 1 < name.Length && char.IsLowSurrogate(name[index + 1]))
                index++;
            else if (char.IsSurrogate(name[index]))
                return false;
        }
        return true;
    }

    private static bool Missing(Exception error) => error is FileNotFoundException or DirectoryNotFoundException;
    private static bool HostFailure(Exception error) => error is IOException or UnauthorizedAccessException;

    // Check every component beneath the root without following links. The final
    // component must be a directory when `directory`, else a file.
    private string Check(IReadOnlyList<string> components, bool directory)
    {
        string request = string.Join('/', components);
        string path = _root;
        for (int index = 0; index < components.Count; index++)
        {
            path = Path.Join(path, components[index]);
            FileAttributes attributes;
            try { attributes = File.GetAttributes(path); }
            catch (Exception error) when (Missing(error))
            { throw new FerruleLocalFileException(FerruleLocalFileError.Missing, request, source: error); }
            catch (Exception error) when (HostFailure(error))
            { throw new FerruleLocalFileException(FerruleLocalFileError.Io, request, source: error); }
            if (attributes.HasFlag(FileAttributes.ReparsePoint))
                throw new FerruleLocalFileException(FerruleLocalFileError.Symlink, request);
            bool expectDirectory = directory || index + 1 < components.Count;
            if (expectDirectory != attributes.HasFlag(FileAttributes.Directory))
                throw new FerruleLocalFileException(FerruleLocalFileError.NotRegular, request);
        }
        return path;
    }

    public IReadOnlyList<string> Select(string filename, FerrulePrimaryFileSelection selection)
    {
        string[] parts = Components(filename);
        if (!selection.IsFileNameWildcard) return [string.Join('/', parts)];
        string pattern = parts[^1];
        string[] directories = parts[..^1];
        if (directories.Any(part => part.Contains('*') || part.Contains('?')))
            throw new FerruleLocalFileException(FerruleLocalFileError.WildcardDirectory, filename);
        if (!pattern.Contains('*') && !pattern.Contains('?')) return [string.Join('/', parts)];
        string directory = Check(directories, true);
        string location = string.Join('/', directories);
        var names = new List<string>();
        var options = new EnumerationOptions
        {
            AttributesToSkip = 0,
            IgnoreInaccessible = false,
            MatchType = MatchType.Simple,
            RecurseSubdirectories = false,
            ReturnSpecialDirectories = false,
        };
        try
        {
            foreach (var entry in new DirectoryInfo(directory).EnumerateFileSystemInfos("*", options))
            {
                string name = entry.Name;
                if (!IsUnicode(name) || !WildcardMatches(pattern, name)) continue;
                var attributes = entry.Attributes;
                if (attributes.HasFlag(FileAttributes.ReparsePoint))
                    throw new FerruleLocalFileException(FerruleLocalFileError.Symlink,
                        location.Length == 0 ? name : location + "/" + name);
                if (!attributes.HasFlag(FileAttributes.Directory)) names.Add(name);
            }
        }
        catch (Exception error) when (HostFailure(error))
        { throw new FerruleLocalFileException(FerruleLocalFileError.Io, location, source: error); }
        if (names.Count == 0 && selection.Empty == FerruleEmptyFileSelection.Refuse)
            throw new FerruleLocalFileException(FerruleLocalFileError.NoMatches, filename);
        names.Sort(CompareScalars);
        return names.Select(name => location.Length == 0 ? name : location + "/" + name).ToArray();
    }

    public FerruleCsvSourceData Load(string request)
    {
        string[] parts = Components(request);
        string path = Check(parts, false);
        string normalized = string.Join('/', parts);
        var bytes = new MemoryStream();
        try
        {
            using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read);
            var buffer = new byte[81920];
            int read;
            while ((read = stream.Read(buffer, 0, buffer.Length)) > 0)
            {
                bytes.Write(buffer, 0, read);
                if (bytes.Length > _maximumDocumentBytes)
                    throw new FerruleLocalFileException(FerruleLocalFileError.TooLarge, normalized,
                        _maximumDocumentBytes);
            }
        }
        catch (Exception error) when (HostFailure(error))
        { throw new FerruleLocalFileException(FerruleLocalFileError.Io, normalized, source: error); }
        return new FerruleCsvSourceData(bytes.ToArray(), path);
    }
}
