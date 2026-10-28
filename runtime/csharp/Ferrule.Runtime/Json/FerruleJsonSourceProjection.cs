using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public sealed record FerruleJsonProjectionLimits
{
    public int Values { get; init; } = 1_000_000;
    public int NameBytes { get; init; } = 64 * 1024 * 1024;
}

public enum FerruleJsonProjectionError
{
    Plan, InvalidLimits, Values, Names,
}

public sealed class FerruleJsonProjectionException : Exception
{
    public FerruleJsonProjectionError Error { get; }
    internal FerruleJsonProjectionException(FerruleJsonProjectionError error)
        : base($"JSON source projection: {error}") => Error = error;
}

/// <summary>
/// A bounded typed-selection plan, not a validating JSON schema. Every matching
/// property/array value remains an ordered occurrence; numbers stay lexical.
/// </summary>
public sealed class FerruleJsonSourceProjection
{
    private sealed record Property(string Name, Shape Value);
    private sealed record Shape(FerruleJsonSourceKind Kind, Property[] Properties, Shape? Item);
    private readonly Shape root;
    private FerruleJsonSourceProjection(Shape root) => this.root = root;

    public static FerruleJsonSourceProjection Parse(string descriptor)
    {
        if (descriptor is null || Encoding.UTF8.GetByteCount(descriptor) > 1024 * 1024)
            throw new FerruleJsonProjectionException(FerruleJsonProjectionError.Plan);
        try
        {
            using var json = JsonDocument.Parse(descriptor, new JsonDocumentOptions { MaxDepth = 128 });
            var nodes = 0;
            var names = 0;
            return new(ReadShape(json.RootElement, 0, ref nodes, ref names));
        }
        catch (Exception error) when (error is JsonException or InvalidOperationException or KeyNotFoundException)
        {
            throw new FerruleJsonProjectionException(FerruleJsonProjectionError.Plan);
        }
    }

    private static Shape ReadShape(JsonElement value, int depth, ref int nodes, ref int names)
    {
        if (depth > 24 || ++nodes > 4096 || value.ValueKind != JsonValueKind.Object)
            throw new FerruleJsonProjectionException(FerruleJsonProjectionError.Plan);
        var kind = value.GetProperty("kind").GetString();
        string[] allowed = kind switch
        {
            "object" => ["kind", "properties"],
            "array" => ["kind", "item"],
            "string" or "number" or "boolean" or "null" => ["kind"],
            _ => throw new FerruleJsonProjectionException(FerruleJsonProjectionError.Plan),
        };
        RequireFields(value, allowed);
        var properties = new List<Property>();
        Shape? item = null;
        if (kind == "object")
        {
            var seen = new HashSet<string>(StringComparer.Ordinal);
            foreach (var property in value.GetProperty("properties").EnumerateArray())
            {
                RequireFields(property, ["name", "shape"]);
                var name = property.GetProperty("name").GetString();
                if (string.IsNullOrEmpty(name) || name.Contains('\0') || !FerruleUnicode.IsWellFormed(name) || !seen.Add(name))
                    throw new FerruleJsonProjectionException(FerruleJsonProjectionError.Plan);
                names = checked(names + Encoding.UTF8.GetByteCount(name));
                if (names > 65536) throw new FerruleJsonProjectionException(FerruleJsonProjectionError.Plan);
                properties.Add(new(name, ReadShape(property.GetProperty("shape"), depth + 1, ref nodes, ref names)));
            }
        }
        if (kind == "array") item = ReadShape(value.GetProperty("item"), depth + 1, ref nodes, ref names);
        var type = kind switch
        {
            "object" => FerruleJsonSourceKind.Object,
            "array" => FerruleJsonSourceKind.Array,
            "string" => FerruleJsonSourceKind.String,
            "number" => FerruleJsonSourceKind.Number,
            "boolean" => FerruleJsonSourceKind.Boolean,
            _ => FerruleJsonSourceKind.Null,
        };
        return new(type, properties.ToArray(), item);
    }

    private static void RequireFields(JsonElement value, string[] expected)
    {
        var names = value.EnumerateObject().Select(p => p.Name).ToArray();
        if (names.Length != expected.Length || names.Distinct(StringComparer.Ordinal).Count() != names.Length || names.Any(n => !expected.Contains(n)))
            throw new FerruleJsonProjectionException(FerruleJsonProjectionError.Plan);
    }

    /// <summary>No I/O, schema validation or numeric conversion. Wrong-kind
    /// roots leave an empty Root collection but keep the physical Path.</summary>
    public FerruleInstance ProjectDocument(FerruleJsonSourceDocument document, FerruleJsonProjectionLimits? limits = null)
    {
        ArgumentNullException.ThrowIfNull(document);
        limits ??= new();
        if (limits.Values is < 0 or > 1_000_000 || limits.NameBytes is < 0 or > 64 * 1024 * 1024)
            throw new FerruleJsonProjectionException(FerruleJsonProjectionError.InvalidLimits);
        var budget = new Budget(limits.Values, limits.NameBytes);
        budget.Value(); budget.Value(); budget.Value();
        budget.Name("Path"); budget.Name("Root");
        var value = Project(root, document.Root, budget);
        return new FerruleGroup([
            new("Path", new FerruleScalar(FerruleValue.FromString(document.Identity))),
            new("Root", new FerruleRepeated(value is null ? [] : new[] { value })),
        ]);
    }

    /// <summary>
    /// Lazily evaluate the filename, select requests and load each complete
    /// document before projecting it. Host exceptions and enumerator cleanup
    /// keep their ordinary .NET propagation. There is no file access here.
    /// </summary>
    public IEnumerable<FerruleInstance> Acquire(Func<string> filename,
        Func<string, IEnumerable<string>> select, Func<string, FerruleJsonSourceDocument> load,
        int maximum = FerruleJsonSources.MaxDocuments, FerruleJsonProjectionLimits? limits = null,
        FerruleJsonGroupingPreparation? grouping = null)
    {
        ArgumentNullException.ThrowIfNull(filename);
        ArgumentNullException.ThrowIfNull(select);
        ArgumentNullException.ThrowIfNull(load);
        if (grouping is not null) ValidateGrouping(grouping);
        if (maximum is < 0 or > FerruleJsonSources.MaxDocuments)
            throw new ArgumentOutOfRangeException(nameof(maximum));
        limits ??= new();
        if (limits.Values is < 0 or > 1_000_000 || limits.NameBytes is < 0 or > 64 * 1024 * 1024)
            throw new FerruleJsonProjectionException(FerruleJsonProjectionError.InvalidLimits);
        return Enumerate();

        IEnumerable<FerruleInstance> Enumerate()
        {
            var count = 0;
            foreach (var request in select(filename()))
            {
                if (count == maximum) throw new FerruleJsonSourceLoadException(maximum);
                var result = ProjectDocument(load(request), limits);
                if (grouping is not null) result = grouping.Prepare(result, limits);
                count++;
                yield return result;
            }
        }
    }

    private void ValidateGrouping(FerruleJsonGroupingPreparation grouping)
    {
        static Shape Select(Shape shape, IEnumerable<string> path)
        {
            foreach (var name in path)
            {
                shape = shape.Kind switch
                {
                    FerruleJsonSourceKind.Object => shape.Properties.FirstOrDefault(property => property.Name == name)?.Value
                        ?? throw new FerruleJsonGroupingException("Plan"),
                    FerruleJsonSourceKind.Array when name == "Items" => shape.Item!,
                    _ => throw new FerruleJsonGroupingException("Plan"),
                };
            }
            return shape;
        }
        var member = Select(root, grouping.CollectionPath.Skip(1));
        if (member.Kind is not (FerruleJsonSourceKind.Object or FerruleJsonSourceKind.Array) || Select(member, grouping.KeyPath).Kind != FerruleJsonSourceKind.String)
            throw new FerruleJsonGroupingException("Plan");
    }

    private sealed class Budget(int values, int names)
    {
        public void Value()
        {
            if (values-- == 0) throw new FerruleJsonProjectionException(FerruleJsonProjectionError.Values);
        }
        public void Name(string name)
        {
            names -= Encoding.UTF8.GetByteCount(name);
            if (names < 0) throw new FerruleJsonProjectionException(FerruleJsonProjectionError.Names);
        }
    }

    private static FerruleInstance? Project(Shape shape, FerruleJsonSourceValue value, Budget budget)
    {
        if (shape.Kind != value.Kind) return null;
        budget.Value();
        var fields = new List<FerruleField>();
        if (shape.Kind == FerruleJsonSourceKind.Object)
        {
            var candidates = shape.Properties.ToDictionary(property => property.Name,
                _ => new List<FerruleJsonSourceValue>(), StringComparer.Ordinal);
            foreach (var member in value.Members)
                if (candidates.TryGetValue(member.Key, out var memberValues)) memberValues.Add(member.Value);
            foreach (var property in shape.Properties)
            {
                budget.Name(property.Name); budget.Value();
                var selected = new List<FerruleInstance>();
                foreach (var candidate in candidates[property.Name])
                {
                    var item = Project(property.Value, candidate, budget);
                    if (item is not null) selected.Add(item);
                }
                fields.Add(new(property.Name, new FerruleRepeated(selected)));
            }
        }
        else if (shape.Kind == FerruleJsonSourceKind.Array)
        {
            budget.Name("Items"); budget.Value();
            var selected = new List<FerruleInstance>();
            foreach (var candidate in value.Items)
            {
                var item = Project(shape.Item!, candidate, budget);
                if (item is not null) selected.Add(item);
            }
            fields.Add(new("Items", new FerruleRepeated(selected)));
        }
        else if (shape.Kind != FerruleJsonSourceKind.Null)
        {
            budget.Name("Value"); budget.Value();
            var scalar = shape.Kind switch
            {
                FerruleJsonSourceKind.String => FerruleValue.FromString(value.StringValue!),
                FerruleJsonSourceKind.Number => FerruleValue.FromString(value.NumberText!),
                _ => FerruleValue.FromBoolean(value.BooleanValue!.Value),
            };
            fields.Add(new("Value", new FerruleScalar(scalar)));
        }
        return new FerruleGroup(fields);
    }
}
