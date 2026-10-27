using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public sealed class FerruleJsonGroupingException(string reason) : Exception("JSON grouping preparation refused: " + reason);

/// <summary>A pure per-document fan-out view; original members and key occurrences are retained.</summary>
public sealed class FerruleJsonGroupingPreparation
{
    public const string KeyField = "__group_key";
    public const string MemberField = "__group_member";
    private readonly string[] collection;
    private readonly string[] keys;
    internal IReadOnlyList<string> CollectionPath => collection;
    internal IReadOnlyList<string> KeyPath => keys;
    private FerruleJsonGroupingPreparation(string[] collection, string[] keys) { this.collection = collection; this.keys = keys; }

    public static FerruleJsonGroupingPreparation Parse(string descriptor)
    {
        if (descriptor is null || Encoding.UTF8.GetByteCount(descriptor) > 1024 * 1024) throw new FerruleJsonGroupingException("Descriptor");
        try
        {
            using var document = JsonDocument.Parse(descriptor);
            var root = document.RootElement;
            var names = root.EnumerateObject().Select(p => p.Name).ToArray();
            if (names.Length != 2 || names.Distinct(StringComparer.Ordinal).Count() != 2 || names.Any(n => n is not ("collection" or "keys"))) throw new FerruleJsonGroupingException("Descriptor");
            static string[] Path(JsonElement element)
            {
                var path = element.EnumerateArray().Select(v => v.GetString()!).ToArray();
                if (path.Length is < 1 or > 64 || path.Any(s => string.IsNullOrEmpty(s) || s.Contains('\0')) || path.Sum(s => (long)Encoding.UTF8.GetByteCount(s)) > 65536) throw new FerruleJsonGroupingException("Plan");
                return path;
            }
            var collection = Path(root.GetProperty("collection"));
            var keys = Path(root.GetProperty("keys"));
            if (collection[0] != "Root") throw new FerruleJsonGroupingException("Plan");
            return new(collection, keys);
        }
        catch (Exception error) when (error is JsonException or InvalidOperationException or KeyNotFoundException)
        { throw new FerruleJsonGroupingException("Descriptor"); }
    }

    public FerruleInstance Prepare(FerruleInstance document, FerruleJsonProjectionLimits? limits = null)
    {
        ArgumentNullException.ThrowIfNull(document); limits ??= new();
        if (limits.Values is < 0 or > 1_000_000 || limits.NameBytes is < 0 or > 64 * 1024 * 1024) throw new FerruleJsonGroupingException("InvalidLimits");
        var budget = new Budget(limits.Values, limits.NameBytes);
        budget.Measure(document, 0);
        return Rewrite(document, 0, budget);
    }

    private FerruleInstance Rewrite(FerruleInstance value, int index, Budget budget)
    {
        if (index == collection.Length)
        {
            if (value is not FerruleRepeated members) throw new FerruleJsonGroupingException("Shape");
            var result = new List<FerruleInstance>();
            foreach (var member in members.Items)
            {
                if (member is not FerruleGroup) throw new FerruleJsonGroupingException("Shape");
                var selected = new List<string>(); SelectKeys(member, 0, selected);
                var seen = new HashSet<string>(StringComparer.Ordinal);
                foreach (var key in selected)
                {
                    if (!seen.Add(key)) continue;
                    budget.Value(); budget.Value(); budget.Name(KeyField); budget.Name(MemberField); budget.Text(key); budget.Measure(member, 0);
                    result.Add(new FerruleGroup([new(KeyField, new FerruleScalar(FerruleValue.FromString(key))), new(MemberField, member)]));
                }
            }
            return new FerruleRepeated(result);
        }
        if (value is FerruleRepeated repeated) return new FerruleRepeated(repeated.Items.Select(item => Rewrite(item, index, budget)).ToArray());
        if (value is not FerruleGroup group) throw new FerruleJsonGroupingException("Shape");
        _ = Field(group, collection[index]);
        return new FerruleGroup(group.Fields.Select(field => field.Name == collection[index] ? new FerruleField(field.Name, Rewrite(field.Value, index + 1, budget)) : field).ToArray());
    }

    private void SelectKeys(FerruleInstance value, int index, List<string> selected)
    {
        if (value is FerruleRepeated repeated) { foreach (var item in repeated.Items) SelectKeys(item, index, selected); return; }
        if (value is not FerruleGroup group) throw new FerruleJsonGroupingException("Shape");
        if (index == keys.Length)
        {
            if (group.Fields.Count != 1) throw new FerruleJsonGroupingException("Shape");
            if (Field(group, "Value") is not FerruleScalar { Value.Kind: FerruleValueKind.String } scalar) throw new FerruleJsonGroupingException("Shape");
            selected.Add(scalar.Value.StringValue); return;
        }
        SelectKeys(Field(group, keys[index]), index + 1, selected);
    }

    private static FerruleInstance Field(FerruleGroup group, string name)
    {
        var fields = group.Fields.Where(field => field.Name == name).ToArray();
        if (fields.Length != 1) throw new FerruleJsonGroupingException("Shape");
        return fields[0].Value;
    }

    private sealed class Budget(int values, int names)
    {
        private int text = 64 * 1024 * 1024;
        public void Value() { if (values-- == 0) throw new FerruleJsonGroupingException("Values"); }
        public void Name(string value) { names -= Encoding.UTF8.GetByteCount(value); if (names < 0) throw new FerruleJsonGroupingException("Names"); }
        public void Text(string value) { text -= Encoding.UTF8.GetByteCount(value); if (text < 0) throw new FerruleJsonGroupingException("Text"); }
        public void Measure(FerruleInstance value, int depth)
        {
            if (depth > 128) throw new FerruleJsonGroupingException("Shape"); Value();
            switch (value)
            {
                case FerruleGroup group: foreach (var field in group.Fields) { Name(field.Name); Measure(field.Value, depth + 1); } break;
                case FerruleRepeated repeated: foreach (var item in repeated.Items) Measure(item, depth + 1); break;
                case FerruleScalar { Value.Kind: FerruleValueKind.String } scalar: Text(scalar.Value.StringValue); break;
                case FerruleScalar: break;
                default: throw new FerruleJsonGroupingException("Shape");
            }
        }
    }
}
