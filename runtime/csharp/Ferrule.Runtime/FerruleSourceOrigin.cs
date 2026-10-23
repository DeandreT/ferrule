using System.Text;

namespace Ferrule.Runtime;

public enum FerruleSourceRowPathError { InactiveFrame, GroupedFrame, MissingOrigin }

public sealed class FerruleSourceRowPathException : Exception
{
    public FerruleSourceRowPathException(FerruleSourceRowPathError error, IReadOnlyList<string> collection)
        : base($"Source row path for [{string.Join('/', collection)}]: {error}")
    {
        Error = error;
        Collection = Array.AsReadOnly(collection.ToArray());
    }
    public FerruleSourceRowPathError Error { get; }
    public IReadOnlyList<string> Collection { get; }
}

/// <summary>One reader/host-created physical source identity, not inferred from a path.</summary>
public sealed class FerruleSourceDocumentOrigin
{
    public const int MaximumPathBytes = 4096;

    public FerruleSourceDocumentOrigin(string path, string? resolvedPath = null)
    {
        Validate(path, nameof(path));
        if (resolvedPath is not null) Validate(resolvedPath, nameof(resolvedPath));
        Path = path;
        ResolvedPath = resolvedPath;
    }
    public string Path { get; }
    public string? ResolvedPath { get; }
    public string SourcePath => ResolvedPath ?? Path;
    public bool SameDocument(FerruleSourceDocumentOrigin? other) => ReferenceEquals(this, other);
    public FerruleSourceRowOrigin Row(ulong position)
    {
        if (position == 0) throw new ArgumentOutOfRangeException(nameof(position));
        return new(this, position);
    }
    private static void Validate(string path, string argument)
    {
        if (string.IsNullOrEmpty(path) || path.Contains('\0') || !FerruleUnicode.IsWellFormed(path)
            || Encoding.UTF8.GetByteCount(path) > MaximumPathBytes)
            throw new ArgumentException("Source paths must be nonempty, NUL-free, valid Unicode and at most 4096 UTF-8 bytes.", argument);
    }
}

/// <summary>Runtime-only original row provenance; not schema admission or endpoint ownership.</summary>
public sealed class FerruleSourceRowOrigin : IEquatable<FerruleSourceRowOrigin>
{
    internal FerruleSourceRowOrigin(FerruleSourceDocumentOrigin document, ulong position)
    {
        Document = document;
        Position = position;
    }
    public FerruleSourceDocumentOrigin Document { get; }
    public ulong Position { get; }
    public bool SameOccurrence(FerruleSourceRowOrigin? other) => other is not null
        && Position == other.Position && Document.SameDocument(other.Document);
    public bool Equals(FerruleSourceRowOrigin? other) => SameOccurrence(other);
    public override bool Equals(object? value) => value is FerruleSourceRowOrigin other && SameOccurrence(other);
    public override int GetHashCode() => HashCode.Combine(System.Runtime.CompilerServices.RuntimeHelpers.GetHashCode(Document), Position);
}
