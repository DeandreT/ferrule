using System.Buffers;
using System.Text.Encodings.Web;
using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleJson
{
    private const int MaximumJsonDependentSchemas = 32;

    private static IReadOnlyList<JsonDependentSchema> ReadJsonDependentSchemas(
        string name,
        JsonElement element,
        bool isObject,
        NodeBudget schemaBudget,
        JsonPatternSchemaContext patternContext,
        int depth)
    {
        JsonElement? declared = null;
        foreach (var property in element.EnumerateObject())
        {
            if (!string.Equals(
                    property.Name,
                    "json_dependent_schemas",
                    StringComparison.Ordinal))
            {
                continue;
            }
            if (declared is not null)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has duplicate dependent-schema metadata.");
            }
            declared = property.Value;
        }
        if (declared is null || declared.Value.ValueKind == JsonValueKind.Null)
        {
            return Array.Empty<JsonDependentSchema>();
        }
        if (!isObject)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has dependent schemas without an object.");
        }

        RequireKind(
            declared.Value,
            JsonValueKind.Array,
            $"schema node '{name}' dependent schemas",
            "array");
        var rules = new List<JsonDependentSchema>();
        var canonicalRules = new HashSet<byte[]>(UniqueItemKeyComparer.Instance);
        var canonicalBudget = new UniqueItemBudget();
        var triggerBytes = 0;
        foreach (var ruleElement in declared.Value.EnumerateArray())
        {
            if (rules.Count == MaximumJsonDependentSchemas)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has more than {MaximumJsonDependentSchemas} dependent schemas.");
            }
            RequireKind(
                ruleElement,
                JsonValueKind.Object,
                $"schema node '{name}' dependent-schema rule",
                "object");
            var fields = new HashSet<string>(StringComparer.Ordinal);
            foreach (var property in ruleElement.EnumerateObject())
            {
                if (property.Name is not ("trigger" or "predicate") ||
                    !fields.Add(property.Name))
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' dependent-schema rule has an unknown or duplicate field '{property.Name}'.");
                }
            }

            var trigger = RequiredString(ruleElement, "trigger");
            triggerBytes = checked(
                triggerBytes +
                StrictUtf8.GetByteCount(trigger));
            if (triggerBytes > MaximumJsonPropertyDependencyNameBytes)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' dependent-schema triggers exceed the {MaximumJsonPropertyDependencyNameBytes}-byte limit.");
            }
            var predicate = ReadJsonSchemaPredicate(
                name,
                RequiredProperty(ruleElement, "predicate"),
                schemaBudget,
                patternContext,
                depth + 1,
                "dependent-schema");
            if (predicate.Schema is { JsonAny: true, Repeating: false })
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has a tautological dependent schema for trigger '{trigger}'.");
            }

            var canonical = CreateUniqueItemKey(ruleElement, canonicalBudget);
            if (!canonicalRules.Add(canonical))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has a duplicate dependent schema.");
            }
            rules.Add(new JsonDependentSchema(trigger, predicate));
        }
        if (rules.Count == 0)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has empty dependent-schema metadata.");
        }
        return rules;
    }

    private static void ValidateDependentSchemas(
        JsonSchemaNode schema,
        IReadOnlyList<JsonProperty> properties,
        JsonElement value,
        NodeBudget budget,
        int depth)
    {
        if (schema.DependentSchemas.Count == 0)
        {
            return;
        }
        var matcher = budget.Matcher();
        foreach (var rule in schema.DependentSchemas)
        {
            if (!properties.Any(property =>
                    string.Equals(property.Name, rule.Trigger, StringComparison.Ordinal)))
            {
                continue;
            }
            if (!MatchesJsonSchemaPredicate(
                    rule.Predicate,
                    value,
                    matcher,
                    depth))
            {
                throw Boundary(
                    $"JSON object '{schema.Name}' triggered a dependent schema for property '{rule.Trigger}' that did not match.");
            }
        }
    }

    private static void WriteDependentObject(
        Utf8JsonWriter writer,
        JsonSchemaNode schema,
        FerruleGroup group,
        NodeBudget budget,
        int depth)
    {
        var buffer = new BoundedJsonBufferWriter(
            MaximumDocumentBytes,
            $"Normalized JSON object '{schema.Name}'");
        using (var objectWriter = new Utf8JsonWriter(
                   buffer,
                   new JsonWriterOptions
                   {
                       Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
                       MaxDepth = MaximumDepth,
                       SkipValidation = false,
                   }))
        {
            WriteObject(objectWriter, schema, group, budget, depth);
        }
        using var document = JsonDocument.Parse(
            buffer.WrittenMemory,
            new JsonDocumentOptions
            {
                MaxDepth = MaximumDepth,
                CommentHandling = JsonCommentHandling.Disallow,
                AllowTrailingCommas = false,
            });
        var properties = OrderedProperties(document.RootElement);
        ValidateDependentSchemas(
            schema,
            properties,
            document.RootElement,
            budget,
            depth);
        document.RootElement.WriteTo(writer);
    }

    private sealed record JsonDependentSchema(
        string Trigger,
        JsonSchemaPredicate Predicate);

    private sealed class BoundedJsonBufferWriter : IBufferWriter<byte>
    {
        private readonly ArrayBufferWriter<byte> _buffer = new(256);
        private readonly int _maximumBytes;
        private readonly string _label;

        public BoundedJsonBufferWriter(int maximumBytes, string label)
        {
            _maximumBytes = maximumBytes;
            _label = label;
        }

        public ReadOnlyMemory<byte> WrittenMemory => _buffer.WrittenMemory;

        public void Advance(int count)
        {
            if (count < 0 || count > _maximumBytes - _buffer.WrittenCount)
            {
                throw Boundary(
                    $"{_label} exceeds the {_maximumBytes}-byte limit.");
            }
            _buffer.Advance(count);
        }

        public Memory<byte> GetMemory(int sizeHint = 0)
        {
            var requested = RequestedCapacity(sizeHint);
            var memory = _buffer.GetMemory(requested);
            return memory[..Math.Min(
                memory.Length,
                _maximumBytes - _buffer.WrittenCount)];
        }

        public Span<byte> GetSpan(int sizeHint = 0)
        {
            var requested = RequestedCapacity(sizeHint);
            var span = _buffer.GetSpan(requested);
            return span[..Math.Min(
                span.Length,
                _maximumBytes - _buffer.WrittenCount)];
        }

        private int RequestedCapacity(int sizeHint)
        {
            if (sizeHint < 0)
            {
                throw new ArgumentOutOfRangeException(nameof(sizeHint));
            }
            var requested = Math.Max(1, sizeHint);
            if (requested > _maximumBytes - _buffer.WrittenCount)
            {
                throw Boundary(
                    $"{_label} exceeds the {_maximumBytes}-byte limit.");
            }
            return requested;
        }
    }
}
