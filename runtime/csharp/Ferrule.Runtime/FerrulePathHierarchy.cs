namespace Ferrule.Runtime;

/// <summary>Bounded recursive directory-tree construction from scalar paths.</summary>
public static class FerrulePathHierarchy
{
    public const int MaximumDepth = 256;
    public const int MaximumItems = 1_000_000;

    public static FerruleInstance Build(
        ScopeContext context,
        IReadOnlyList<string> collection,
        string separator,
        string directoriesField,
        string filesField,
        string nameField)
    {
        ArgumentNullException.ThrowIfNull(context);
        ArgumentNullException.ThrowIfNull(collection);
        ArgumentException.ThrowIfNullOrEmpty(separator);
        ArgumentException.ThrowIfNullOrEmpty(directoriesField);
        ArgumentException.ThrowIfNullOrEmpty(filesField);
        ArgumentException.ThrowIfNullOrEmpty(nameField);

        return BuildFrom(
            LegacyItems(context, collection),
            separator,
            directoriesField,
            filesField,
            nameField);
    }

    private static IEnumerable<FerruleInstance> LegacyItems(
        ScopeContext context,
        IReadOnlyList<string> collection)
    {
        foreach (var candidate in context.IterateSource(collection))
        {
            if (candidate.Frames.Count != 0)
            {
                yield return candidate.Frames[^1];
            }
        }
    }

    /// <summary>
    /// Builds the same tree from one exact owner's collection. A same-owner frame
    /// pins its prefix; other owners and legacy frames never supply the items.
    /// The directory, file, and name fields describe output structure only.
    /// </summary>
    public static FerruleInstance BuildOwned(
        FerruleBasicScopeContext context,
        FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> collection,
        string separator,
        string directoriesField,
        string filesField,
        string nameField)
    {
        ArgumentNullException.ThrowIfNull(context);
        ArgumentNullException.ThrowIfNull(collection);
        ArgumentException.ThrowIfNullOrEmpty(separator);
        ArgumentException.ThrowIfNullOrEmpty(directoriesField);
        ArgumentException.ThrowIfNullOrEmpty(filesField);
        ArgumentException.ThrowIfNullOrEmpty(nameField);

        var cohort = context.WalkOwnedSourceCohort(owner, collection);
        var items = new List<FerruleInstance>(cohort.Count);
        foreach (var candidate in cohort)
        {
            var frames = candidate.Context.LegacyView.Frames;
            if (frames.Count != 0)
            {
                items.Add(frames[^1]);
            }
        }
        GC.KeepAlive(cohort);
        return BuildFrom(items, separator, directoriesField, filesField, nameField);
    }

    private static FerruleInstance BuildFrom(
        IEnumerable<FerruleInstance> items,
        string separator,
        string directoriesField,
        string filesField,
        string nameField)
    {
        var roots = new List<Directory>();
        var materialized = 0;
        foreach (var item in items)
        {
            if (item is not FerruleScalar scalar)
            {
                throw ValueType(InstanceKind(item));
            }
            var value = scalar.Value.Kind switch
            {
                FerruleValueKind.String => scalar.Value.StringValue,
                FerruleValueKind.Null or FerruleValueKind.JsonNull or FerruleValueKind.XmlNil =>
                    string.Empty,
                _ => throw ValueType(scalar.Value.Kind.ToString()),
            };
            if (value.Length == 0)
            {
                continue;
            }
            var separatorIndex = value.IndexOf(separator, StringComparison.Ordinal);
            if (separatorIndex < 0)
            {
                continue;
            }
            var rootName = value[..separatorIndex];
            var remainder = value[(separatorIndex + separator.Length)..];
            var root = roots.Find(candidate =>
                string.Equals(candidate.Name, rootName, StringComparison.Ordinal));
            if (root is null)
            {
                ReserveItem(ref materialized);
                root = new Directory(rootName);
                roots.Add(root);
            }
            root.Insert(remainder, separator, 1, ref materialized);
        }

        if (roots.Count != 1)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.PathHierarchyRootCount,
                $"path hierarchy requires exactly one root directory, found {roots.Count}",
                detail: roots.Count.ToString(System.Globalization.CultureInfo.InvariantCulture));
        }
        return roots[0].IntoInstance(directoriesField, filesField, nameField);
    }

    private static void ReserveItem(ref int materialized)
    {
        if (materialized >= MaximumItems)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.PathHierarchyTooLarge,
                $"path hierarchy materializes more than {MaximumItems} directory and file items",
                maximumItems: (UInt128)MaximumItems);
        }
        materialized++;
    }

    private static FerruleRuntimeException ValueType(string found) =>
        new(
            FerruleRuntimeError.PathHierarchyValueType,
            $"path-hierarchy input values must be strings, got {found}",
            foundInstance: found);

    private static string InstanceKind(FerruleInstance instance) => instance switch
    {
        FerruleScalar => "scalar",
        FerruleGroup => "group",
        FerruleRepeated => "repeated collection",
        FerruleMappedSequence => "mapped sequence",
        FerruleDocumentSet => "document set",
        _ => "unknown instance",
    };

    private sealed class Directory
    {
        private readonly List<string> _files = new();
        private readonly List<Directory> _directories = new();

        internal Directory(string name)
        {
            Name = name;
        }

        internal string Name { get; }

        internal void Insert(
            string path,
            string separator,
            int depth,
            ref int materialized)
        {
            if (depth >= MaximumDepth)
            {
                throw new FerruleRuntimeException(
                    FerruleRuntimeError.PathHierarchyDepth,
                    $"path hierarchy exceeds the {MaximumDepth}-directory depth limit",
                    maximumDepth: MaximumDepth);
            }
            var separatorIndex = path.IndexOf(separator, StringComparison.Ordinal);
            if (separatorIndex < 0)
            {
                ReserveItem(ref materialized);
                _files.Add(path);
                return;
            }

            var directoryName = path[..separatorIndex];
            var remainder = path[(separatorIndex + separator.Length)..];
            var child = _directories.Find(candidate =>
                string.Equals(candidate.Name, directoryName, StringComparison.Ordinal));
            if (child is null)
            {
                ReserveItem(ref materialized);
                child = new Directory(directoryName);
                _directories.Add(child);
            }
            child.Insert(remainder, separator, depth + 1, ref materialized);
        }

        internal FerruleGroup IntoInstance(
            string directoriesField,
            string filesField,
            string nameField)
        {
            var files = _files.Select(file =>
                (FerruleInstance)new FerruleGroup(new[]
                {
                    new FerruleField(
                        nameField,
                        new FerruleScalar(FerruleValue.FromString(file))),
                }));
            var directories = _directories.Select(directory =>
                (FerruleInstance)directory.IntoInstance(
                    directoriesField,
                    filesField,
                    nameField));
            return new FerruleGroup(new FerruleField[]
            {
                new(filesField, new FerruleRepeated(files)),
                new(directoriesField, new FerruleRepeated(directories)),
                new(nameField, new FerruleScalar(FerruleValue.FromString(Name))),
            });
        }
    }
}
