using System.Buffers;
using System.Globalization;
using System.Numerics;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;

namespace Ferrule.Runtime;

/// <summary>Bounded schema-shaped JSON parsing and serialization for generated mappings.</summary>
public static class FerruleJson
{
    public const int MaximumSchemaBytes = 1024 * 1024;
    public const int MaximumDocumentBytes = 64 * 1024 * 1024;
    public const int MaximumDepth = 256;
    public const int MaximumNodes = 1_000_000;

    private static readonly JsonSerializerOptions CanonicalJsonOptions = new()
    {
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
    };

    public static FerruleInstance Parse(string schemaJson, string document)
    {
        ArgumentNullException.ThrowIfNull(schemaJson);
        ArgumentNullException.ThrowIfNull(document);
        RequireUtf8Limit(schemaJson, MaximumSchemaBytes, "embedded JSON schema");
        RequireUtf8Limit(document, MaximumDocumentBytes, "JSON input");
        var schema = ParseSchema(schemaJson);
        try
        {
            var input = document.Length > 0 && document[0] == '\uFEFF'
                ? document[1..]
                : document;
            using var parsed = JsonDocument.Parse(
                input,
                new JsonDocumentOptions
                {
                    MaxDepth = MaximumDepth,
                    CommentHandling = JsonCommentHandling.Disallow,
                    AllowTrailingCommas = false,
                });
            var budget = new NodeBudget();
            return ReadNode(schema, parsed.RootElement, budget, 0);
        }
        catch (FerruleRuntimeException)
        {
            throw;
        }
        catch (Exception error) when (error is JsonException or FormatException or OverflowException)
        {
            throw Boundary("JSON input is invalid.", error);
        }
    }

    internal static void ValidateSchema(string schemaJson)
    {
        ArgumentNullException.ThrowIfNull(schemaJson);
        RequireUtf8Limit(schemaJson, MaximumSchemaBytes, "embedded JSON schema");
        _ = ParseSchema(schemaJson);
    }

    public static string Serialize(string schemaJson, FerruleInstance instance)
    {
        ArgumentNullException.ThrowIfNull(schemaJson);
        ArgumentNullException.ThrowIfNull(instance);
        RequireUtf8Limit(schemaJson, MaximumSchemaBytes, "embedded JSON schema");
        var schema = ParseSchema(schemaJson);
        try
        {
            var buffer = new ArrayBufferWriter<byte>();
            using (var writer = new Utf8JsonWriter(
                       buffer,
                       new JsonWriterOptions
                       {
                           Indented = true,
                           Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
                           MaxDepth = MaximumDepth,
                           SkipValidation = false,
                       }))
            {
                var budget = new NodeBudget();
                if (instance is FerruleRepeated repeated && !schema.Repeating)
                {
                    budget.Visit(0);
                    writer.WriteStartArray();
                    foreach (var item in repeated.Items)
                    {
                        WriteSingleNode(writer, schema, item, budget, 1);
                    }

                    writer.WriteEndArray();
                }
                else
                {
                    WriteNode(writer, schema, instance, budget, 0);
                }
            }

            var outputBytes = checked(buffer.WrittenCount + 1);
            if (outputBytes > MaximumDocumentBytes)
            {
                throw Boundary(
                    $"JSON output is {outputBytes} bytes; maximum is {MaximumDocumentBytes}.");
            }

            return Encoding.UTF8.GetString(buffer.WrittenSpan) + "\n";
        }
        catch (FerruleRuntimeException)
        {
            throw;
        }
        catch (Exception error) when (error is JsonException or FormatException or OverflowException)
        {
            throw Boundary("JSON output is invalid.", error);
        }
    }

    private static JsonSchemaNode ParseSchema(string schemaJson)
    {
        try
        {
            using var parsed = JsonDocument.Parse(
                schemaJson,
                new JsonDocumentOptions
                {
                    MaxDepth = MaximumDepth,
                    CommentHandling = JsonCommentHandling.Disallow,
                    AllowTrailingCommas = false,
                });
            var budget = new NodeBudget();
            return ReadSchemaNode(parsed.RootElement, budget, 0);
        }
        catch (FerruleRuntimeException)
        {
            throw;
        }
        catch (Exception error) when (error is JsonException or FormatException or OverflowException)
        {
            throw Boundary("Embedded JSON schema is invalid.", error);
        }
    }

    private static JsonSchemaNode ReadSchemaNode(
        JsonElement element,
        NodeBudget budget,
        int depth)
    {
        budget.Visit(depth);
        RequireKind(element, JsonValueKind.Object, "schema node", "object");
        var name = RequiredString(element, "name");
        var kindElement = RequiredProperty(element, "kind");
        RequireKind(kindElement, JsonValueKind.Object, $"schema node '{name}' kind", "object");
        var kind = RequiredString(kindElement, "kind");
        var scalarDomain = kind switch
        {
            "scalar" => ScalarDomain(
                name,
                RequiredString(kindElement, "ty")),
            "scalar_union" => ReadScalarDomain(name, kindElement),
            "group" => JsonScalarDomain.None,
            _ => throw Boundary(
                $"Embedded JSON schema node '{name}' has unknown kind '{kind}'."),
        };
        var children = new List<JsonSchemaNode>();
        JsonSchemaNode? dynamic = null;
        var alternatives = new List<JsonAlternative>();
        if (scalarDomain == JsonScalarDomain.None)
        {
            if (kindElement.TryGetProperty("children", out var childElements))
            {
                RequireKind(childElements, JsonValueKind.Array, $"schema node '{name}' children", "array");
                foreach (var child in childElements.EnumerateArray())
                {
                    children.Add(ReadSchemaNode(child, budget, depth + 1));
                }
            }

            if (kindElement.TryGetProperty("dynamic", out var dynamicElement) &&
                dynamicElement.ValueKind != JsonValueKind.Null)
            {
                dynamic = ReadSchemaNode(dynamicElement, budget, depth + 1);
            }

            if (kindElement.TryGetProperty("alternatives", out var alternativeElements))
            {
                RequireKind(
                    alternativeElements,
                    JsonValueKind.Array,
                    $"schema node '{name}' alternatives",
                    "array");
                foreach (var alternative in alternativeElements.EnumerateArray())
                {
                    alternatives.Add(ReadAlternative(alternative));
                }
            }
            if (dynamic is not null && alternatives.Count != 0)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' combines an open object with closed alternatives.");
            }
        }

        return new JsonSchemaNode(
            name,
            OptionalBoolean(element, "repeating"),
            OptionalBoolean(element, "nullable"),
            OptionalBoolean(element, "container_nullable"),
            OptionalBoolean(element, "json_any"),
            scalarDomain,
            children,
            dynamic,
            alternatives,
            element.TryGetProperty("alternative_mode", out var mode) &&
            mode.ValueKind == JsonValueKind.String &&
            string.Equals(mode.GetString(), "inclusive", StringComparison.Ordinal));
    }

    private static JsonScalarDomain ReadScalarDomain(
        string nodeName,
        JsonElement kindElement)
    {
        var typeElements = RequiredProperty(kindElement, "types");
        RequireKind(
            typeElements,
            JsonValueKind.Array,
            $"schema node '{nodeName}' scalar union types",
            "array");
        var domain = JsonScalarDomain.None;
        var previousOrder = -1;
        var count = 0;
        foreach (var typeElement in typeElements.EnumerateArray())
        {
            if (typeElement.ValueKind != JsonValueKind.String)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{nodeName}' scalar union types must contain strings.");
            }

            var typeName = typeElement.GetString() ?? string.Empty;
            var scalar = ScalarDomain(nodeName, typeName);
            var order = ScalarOrder(scalar);
            if (domain.HasFlag(scalar))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{nodeName}' scalar union contains duplicate type '{typeName}'.");
            }
            if (order <= previousOrder)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{nodeName}' scalar union types are not in canonical order.");
            }

            domain |= scalar;
            previousOrder = order;
            count = checked(count + 1);
        }

        if (count < 2)
        {
            throw Boundary(
                $"Embedded JSON schema node '{nodeName}' scalar union must contain at least two distinct types.");
        }

        return domain;
    }

    private static JsonScalarDomain ScalarDomain(string nodeName, string typeName) =>
        typeName switch
        {
            "string" => JsonScalarDomain.String,
            "int" => JsonScalarDomain.Int64,
            "float" => JsonScalarDomain.Double,
            "bool" => JsonScalarDomain.Bool,
            _ => throw Boundary(
                $"Embedded JSON schema node '{nodeName}' has unknown scalar type '{typeName}'."),
        };

    private static int ScalarOrder(JsonScalarDomain scalar) => scalar switch
    {
        JsonScalarDomain.String => 0,
        JsonScalarDomain.Int64 => 1,
        JsonScalarDomain.Double => 2,
        JsonScalarDomain.Bool => 3,
        _ => throw Boundary("Embedded JSON schema scalar domain is invalid."),
    };

    private static JsonAlternative ReadAlternative(JsonElement element)
    {
        RequireKind(element, JsonValueKind.Object, "schema alternative", "object");
        var members = RequiredStrings(element, "members");
        var required = element.TryGetProperty("required", out _)
            ? RequiredStrings(element, "required")
            : Array.Empty<string>();
        var constraints = new List<JsonConstraint>();
        if (element.TryGetProperty("constraints", out var constraintElements))
        {
            RequireKind(constraintElements, JsonValueKind.Array, "schema constraints", "array");
            foreach (var constraint in constraintElements.EnumerateArray())
            {
                var member = RequiredString(constraint, "member");
                var value = RequiredProperty(constraint, "value");
                var type = RequiredString(value, "type");
                var expected = value.TryGetProperty("value", out var expectedValue)
                    ? expectedValue.Clone()
                    : default;
                constraints.Add(new JsonConstraint(member, type, expected));
            }
        }

        return new JsonAlternative(members, required, constraints);
    }

    private static FerruleInstance ReadNode(
        JsonSchemaNode schema,
        JsonElement element,
        NodeBudget budget,
        int depth)
    {
        budget.Visit(depth);
        if (schema.ContainerNullable && element.ValueKind == JsonValueKind.Null)
        {
            return new FerruleScalar(FerruleValue.JsonNull);
        }

        if (schema.Repeating)
        {
            RequireKind(element, JsonValueKind.Array, schema.Name, "array");
            var items = new List<FerruleInstance>();
            foreach (var item in element.EnumerateArray())
            {
                items.Add(ReadSingleNode(schema, item, budget, depth + 1));
            }

            return new FerruleRepeated(items);
        }

        return ReadSingleNode(schema, element, budget, depth);
    }

    private static FerruleInstance ReadSingleNode(
        JsonSchemaNode schema,
        JsonElement element,
        NodeBudget budget,
        int depth)
    {
        if (schema.JsonAny)
        {
            return new FerruleScalar(
                FerruleValue.FromString(JsonSerializer.Serialize(element, CanonicalJsonOptions)));
        }

        if (schema.ContainerNullable && element.ValueKind == JsonValueKind.Null)
        {
            return new FerruleScalar(FerruleValue.JsonNull);
        }

        if (schema.IsScalar)
        {
            return new FerruleScalar(ReadScalar(schema, element));
        }

        RequireKind(element, JsonValueKind.Object, schema.Name, "object");
        var properties = OrderedProperties(element);
        ValidateAlternatives(schema, properties);
        var fields = new List<FerruleField>();
        if (schema.Dynamic is { } dynamic)
        {
            foreach (var property in properties)
            {
                var child = schema.Child(property.Name) ?? dynamic;
                fields.Add(
                    new FerruleField(
                        property.Name,
                        ReadNode(child, property.Value, budget, depth + 1)));
            }

            foreach (var child in schema.Children)
            {
                if (!properties.Any(property =>
                        string.Equals(property.Name, child.Name, StringComparison.Ordinal)))
                {
                    fields.Add(new FerruleField(child.Name, Missing(child)));
                }
            }
        }
        else
        {
            foreach (var child in schema.Children)
            {
                var property = properties.Find(candidate =>
                    string.Equals(candidate.Name, child.Name, StringComparison.Ordinal));
                fields.Add(
                    new FerruleField(
                        child.Name,
                        property is null
                            ? Missing(child)
                            : ReadNode(child, property.Value, budget, depth + 1)));
            }
        }

        return new FerruleGroup(fields);
    }

    private static FerruleValue ReadScalar(
        JsonSchemaNode schema,
        JsonElement element)
    {
        if (element.ValueKind == JsonValueKind.Null && schema.Nullable)
        {
            return FerruleValue.JsonNull;
        }

        var domain = schema.ScalarDomain;
        if (element.ValueKind == JsonValueKind.String &&
            domain.HasFlag(JsonScalarDomain.String))
        {
            return FerruleValue.FromString(element.GetString() ?? string.Empty);
        }
        if (element.ValueKind == JsonValueKind.Number)
        {
            if (domain.HasFlag(JsonScalarDomain.Int64) &&
                element.TryGetInt64(out var integer))
            {
                return FerruleValue.FromInt64(integer);
            }
            if (domain.HasFlag(JsonScalarDomain.Double))
            {
                return ReadDouble(schema.Name, element);
            }
        }
        if (element.ValueKind is JsonValueKind.True or JsonValueKind.False &&
            domain.HasFlag(JsonScalarDomain.Bool))
        {
            return FerruleValue.FromBoolean(element.GetBoolean());
        }

        throw Shape(
            schema.Name,
            ScalarName(domain),
            element.ValueKind.ToString());
    }

    private static FerruleValue ReadDouble(string name, JsonElement element)
    {
        if (element.TryGetInt64(out var integer))
        {
            if (TryExactDouble(integer, out var converted))
            {
                return FerruleValue.FromDouble(converted);
            }

            throw Shape(name, "number", "integer outside the exact double range");
        }
        if (element.TryGetUInt64(out var unsignedInteger))
        {
            if (TryExactDouble(unsignedInteger, out var converted))
            {
                return FerruleValue.FromDouble(converted);
            }

            throw Shape(name, "number", "integer outside the exact double range");
        }

        var value = element.GetDouble();
        if (!double.IsFinite(value))
        {
            throw Shape(name, "finite number", "non-finite number");
        }

        return FerruleValue.FromDouble(value);
    }

    private static FerruleInstance Missing(JsonSchemaNode schema)
    {
        if (schema.ContainerNullable)
        {
            return new FerruleScalar(FerruleValue.Null);
        }

        if (schema.Repeating)
        {
            return new FerruleRepeated(Array.Empty<FerruleInstance>());
        }

        return schema.IsScalar
            ? new FerruleScalar(FerruleValue.Null)
            : new FerruleGroup(Array.Empty<FerruleField>());
    }

    private static void WriteNode(
        Utf8JsonWriter writer,
        JsonSchemaNode schema,
        FerruleInstance instance,
        NodeBudget budget,
        int depth)
    {
        budget.Visit(depth);
        if (schema.ContainerNullable &&
            instance is FerruleScalar { Value.Kind: FerruleValueKind.JsonNull })
        {
            writer.WriteNullValue();
            return;
        }

        if (schema.Repeating)
        {
            if (instance is not FerruleRepeated repeated)
            {
                throw Shape(schema.Name, "array", InstanceKind(instance));
            }

            writer.WriteStartArray();
            foreach (var item in repeated.Items)
            {
                WriteSingleNode(writer, schema, item, budget, depth + 1);
            }

            writer.WriteEndArray();
            return;
        }

        WriteSingleNode(writer, schema, instance, budget, depth);
    }

    private static void WriteSingleNode(
        Utf8JsonWriter writer,
        JsonSchemaNode schema,
        FerruleInstance instance,
        NodeBudget budget,
        int depth)
    {
        if (schema.JsonAny)
        {
            WriteAny(writer, schema, instance);
            return;
        }

        if (schema.ContainerNullable &&
            instance is FerruleScalar { Value.Kind: FerruleValueKind.JsonNull })
        {
            writer.WriteNullValue();
            return;
        }

        if (schema.IsScalar)
        {
            if (instance is not FerruleScalar value)
            {
                throw Shape(
                    schema.Name,
                    ScalarName(schema.ScalarDomain),
                    InstanceKind(instance));
            }

            if (schema.IsScalarUnion)
            {
                WriteScalarUnion(writer, schema, value.Value);
            }
            else
            {
                WriteScalar(writer, schema, SingleScalar(schema.ScalarDomain), value.Value);
            }
            return;
        }

        if (instance is not FerruleGroup group)
        {
            throw Shape(schema.Name, "object", InstanceKind(instance));
        }

        ValidateOutputAlternatives(schema, group);
        writer.WriteStartObject();
        if (schema.Dynamic is { } dynamic)
        {
            foreach (var field in group.Fields)
            {
                var child = schema.Child(field.Name) ?? dynamic;
                if (BoundaryAbsence(child, field.Value))
                {
                    continue;
                }

                writer.WritePropertyName(field.Name);
                WriteNode(writer, child, field.Value, budget, depth + 1);
            }
        }
        else
        {
            foreach (var child in schema.Children)
            {
                if (!group.TryGetField(child.Name, out var value) ||
                    BoundaryAbsence(child, value))
                {
                    continue;
                }

                writer.WritePropertyName(child.Name);
                WriteNode(writer, child, value, budget, depth + 1);
            }
        }

        writer.WriteEndObject();
    }

    private static void WriteAny(
        Utf8JsonWriter writer,
        JsonSchemaNode schema,
        FerruleInstance instance)
    {
        if (instance is not FerruleScalar scalar)
        {
            throw Shape(schema.Name, "arbitrary JSON scalar encoding", InstanceKind(instance));
        }

        switch (scalar.Value.Kind)
        {
            case FerruleValueKind.String:
                try
                {
                    using var document = JsonDocument.Parse(
                        scalar.Value.StringValue,
                        new JsonDocumentOptions
                        {
                            MaxDepth = MaximumDepth,
                            CommentHandling = JsonCommentHandling.Disallow,
                            AllowTrailingCommas = false,
                        });
                    document.RootElement.WriteTo(writer);
                }
                catch (JsonException)
                {
                    writer.WriteStringValue(scalar.Value.StringValue);
                }

                break;
            case FerruleValueKind.Bool:
                writer.WriteBooleanValue(scalar.Value.BooleanValue);
                break;
            case FerruleValueKind.Int64:
                writer.WriteNumberValue(scalar.Value.Int64Value);
                break;
            case FerruleValueKind.Double when double.IsFinite(scalar.Value.DoubleValue):
                WriteFiniteDouble(writer, scalar.Value.DoubleValue);
                break;
            case FerruleValueKind.JsonNull:
                writer.WriteNullValue();
                break;
            default:
                throw Shape(
                    schema.Name,
                    "arbitrary JSON value",
                    scalar.Value.Kind.ToString());
        }
    }

    private static void WriteScalar(
        Utf8JsonWriter writer,
        JsonSchemaNode schema,
        JsonScalarType scalar,
        FerruleValue value)
    {
        if (value.Kind == FerruleValueKind.JsonNull && schema.Nullable)
        {
            writer.WriteNullValue();
            return;
        }

        switch (scalar, value.Kind)
        {
            case (JsonScalarType.String, FerruleValueKind.String):
                writer.WriteStringValue(value.StringValue);
                return;
            case (JsonScalarType.String, FerruleValueKind.Bool):
                writer.WriteStringValue(value.BooleanValue ? "true" : "false");
                return;
            case (JsonScalarType.String, FerruleValueKind.Int64):
                writer.WriteStringValue(value.Int64Value.ToString(CultureInfo.InvariantCulture));
                return;
            case (JsonScalarType.String, FerruleValueKind.Double)
                when double.IsFinite(value.DoubleValue):
                writer.WriteStringValue(value.DoubleValue.ToString("R", CultureInfo.InvariantCulture));
                return;
            case (JsonScalarType.Int64, FerruleValueKind.Int64):
                writer.WriteNumberValue(value.Int64Value);
                return;
            case (JsonScalarType.Int64, FerruleValueKind.String)
                when long.TryParse(
                    value.StringValue.Trim(),
                    NumberStyles.AllowLeadingSign,
                    CultureInfo.InvariantCulture,
                    out var integer):
                writer.WriteNumberValue(integer);
                return;
            case (JsonScalarType.Double, FerruleValueKind.Int64)
                when TryExactDouble(value.Int64Value, out _):
                writer.WriteNumberValue(value.Int64Value);
                return;
            case (JsonScalarType.Double, FerruleValueKind.Double)
                when double.IsFinite(value.DoubleValue):
                WriteFiniteDouble(writer, value.DoubleValue);
                return;
            case (JsonScalarType.Double, FerruleValueKind.String)
                when double.TryParse(
                         value.StringValue.Trim(),
                         NumberStyles.Float,
                         CultureInfo.InvariantCulture,
                         out var number) &&
                     double.IsFinite(number):
                WriteFiniteDouble(writer, number);
                return;
            case (JsonScalarType.Bool, FerruleValueKind.Bool):
                writer.WriteBooleanValue(value.BooleanValue);
                return;
            case (JsonScalarType.Bool, FerruleValueKind.String)
                when string.Equals(value.StringValue.Trim(), "true", StringComparison.Ordinal):
                writer.WriteBooleanValue(true);
                return;
            case (JsonScalarType.Bool, FerruleValueKind.String)
                when string.Equals(value.StringValue.Trim(), "false", StringComparison.Ordinal):
                writer.WriteBooleanValue(false);
                return;
            default:
                throw Shape(schema.Name, ScalarName(scalar), value.Kind.ToString());
        }
    }

    private static void WriteScalarUnion(
        Utf8JsonWriter writer,
        JsonSchemaNode schema,
        FerruleValue value)
    {
        if (value.Kind == FerruleValueKind.Int64 &&
            !schema.ScalarDomain.HasFlag(JsonScalarDomain.Int64) &&
            schema.ScalarDomain.HasFlag(JsonScalarDomain.Double) &&
            TryExactDouble(value.Int64Value, out _))
        {
            writer.WriteNumberValue(value.Int64Value);
            return;
        }

        var normalized = NormalizeScalarUnion(schema, value);
        if (normalized.Kind == FerruleValueKind.JsonNull)
        {
            writer.WriteNullValue();
            return;
        }

        WriteAdmittedScalar(writer, schema, normalized);
    }

    private static FerruleValue NormalizeScalarUnion(
        JsonSchemaNode schema,
        FerruleValue value)
    {
        var domain = schema.ScalarDomain;
        if (value.Kind == FerruleValueKind.JsonNull && schema.Nullable ||
            ValueDomain(value.Kind) is { } valueDomain && domain.HasFlag(valueDomain))
        {
            return value;
        }
        if (value.Kind == FerruleValueKind.Int64 &&
            domain.HasFlag(JsonScalarDomain.Double))
        {
            if (TryExactDouble(value.Int64Value, out var converted))
            {
                return FerruleValue.FromDouble(converted);
            }

            throw Shape(
                schema.Name,
                "number",
                "int outside the exact double range");
        }

        if (value.Kind == FerruleValueKind.String &&
            !domain.HasFlag(JsonScalarDomain.String))
        {
            var converted = FerruleValue.Null;
            var conversions = 0;
            var text = value.StringValue.Trim();
            if (domain.HasFlag(JsonScalarDomain.Int64) &&
                long.TryParse(
                    text,
                    NumberStyles.AllowLeadingSign,
                    CultureInfo.InvariantCulture,
                    out var integer))
            {
                converted = FerruleValue.FromInt64(integer);
                conversions++;
            }
            if (domain.HasFlag(JsonScalarDomain.Double) &&
                double.TryParse(
                    text,
                    NumberStyles.Float,
                    CultureInfo.InvariantCulture,
                    out var number) &&
                double.IsFinite(number))
            {
                converted = FerruleValue.FromDouble(number);
                conversions++;
            }
            if (domain.HasFlag(JsonScalarDomain.Bool) &&
                string.Equals(text, "true", StringComparison.Ordinal))
            {
                converted = FerruleValue.FromBoolean(true);
                conversions++;
            }
            else if (domain.HasFlag(JsonScalarDomain.Bool) &&
                     string.Equals(text, "false", StringComparison.Ordinal))
            {
                converted = FerruleValue.FromBoolean(false);
                conversions++;
            }

            if (conversions == 1)
            {
                return converted;
            }
            if (conversions > 1)
            {
                throw Shape(
                    schema.Name,
                    "unambiguous declared scalar union",
                    "String");
            }
        }

        throw Shape(
            schema.Name,
            "declared scalar union",
            value.Kind.ToString());
    }

    private static void WriteAdmittedScalar(
        Utf8JsonWriter writer,
        JsonSchemaNode schema,
        FerruleValue value)
    {
        switch (value.Kind)
        {
            case FerruleValueKind.String:
                writer.WriteStringValue(value.StringValue);
                return;
            case FerruleValueKind.Int64:
                writer.WriteNumberValue(value.Int64Value);
                return;
            case FerruleValueKind.Double when double.IsFinite(value.DoubleValue):
                WriteFiniteDouble(writer, value.DoubleValue);
                return;
            case FerruleValueKind.Bool:
                writer.WriteBooleanValue(value.BooleanValue);
                return;
            default:
                throw Shape(
                    schema.Name,
                    "declared scalar union",
                    value.Kind.ToString());
        }
    }

    private static void WriteFiniteDouble(Utf8JsonWriter writer, double value)
    {
        var lexical = value.ToString("R", CultureInfo.InvariantCulture);
        if (!lexical.Contains('.') &&
            !lexical.Contains('E') &&
            !lexical.Contains('e'))
        {
            lexical += ".0";
        }

        writer.WriteRawValue(lexical, skipInputValidation: false);
    }

    private static bool BoundaryAbsence(JsonSchemaNode schema, FerruleInstance instance) =>
        instance is FerruleScalar { Value.Kind: FerruleValueKind.Null } &&
        (schema.ContainerNullable || !schema.Repeating && schema.IsScalar);

    private static void ValidateAlternatives(
        JsonSchemaNode schema,
        IReadOnlyList<JsonProperty> properties)
    {
        if (schema.Alternatives.Count == 0)
        {
            return;
        }

        var matches = schema.Alternatives.Count(alternative =>
            alternative.Required.All(required =>
            {
                var property = properties.FirstOrDefault(candidate =>
                    string.Equals(candidate.Name, required, StringComparison.Ordinal));
                return property is not null &&
                       (property.Value.ValueKind != JsonValueKind.Null ||
                        schema.Child(required)?.Nullable == true);
            }) &&
            properties.All(property => alternative.Members.Contains(
                property.Name,
                StringComparer.Ordinal)) &&
            alternative.Constraints.All(constraint =>
            {
                var property = properties.FirstOrDefault(candidate =>
                    string.Equals(candidate.Name, constraint.Member, StringComparison.Ordinal));
                return property is null || ConstraintMatches(constraint, property.Value);
            }));
        if (matches == 0)
        {
            throw Boundary($"JSON object '{schema.Name}' matches no declared schema alternative.");
        }

        if (matches > 1 && !schema.InclusiveAlternatives)
        {
            throw Boundary(
                $"JSON object '{schema.Name}' matches more than one declared schema alternative.");
        }
    }

    private static void ValidateOutputAlternatives(
        JsonSchemaNode schema,
        FerruleGroup group)
    {
        if (schema.Alternatives.Count == 0)
        {
            return;
        }

        var fields = new List<OutputProperty>();
        foreach (var child in schema.Children)
        {
            if (group.TryGetField(child.Name, out var value) &&
                !BoundaryAbsence(child, value))
            {
                fields.Add(new OutputProperty(child, value));
            }
        }

        var matches = schema.Alternatives.Count(alternative =>
            alternative.Required.All(required =>
                fields.Any(field =>
                    string.Equals(field.Schema.Name, required, StringComparison.Ordinal) &&
                    (!IsExplicitJsonNull(field.Value) || field.Schema.Nullable))) &&
            fields.All(field => alternative.Members.Contains(
                field.Schema.Name,
                StringComparer.Ordinal)) &&
            alternative.Constraints.All(constraint =>
            {
                var field = fields.FirstOrDefault(candidate =>
                    string.Equals(
                        candidate.Schema.Name,
                        constraint.Member,
                        StringComparison.Ordinal));
                return field is null ||
                       OutputConstraintMatches(constraint, field.Schema, field.Value);
            }));
        if (matches == 0)
        {
            throw Boundary($"JSON object '{schema.Name}' matches no declared schema alternative.");
        }

        if (matches > 1 && !schema.InclusiveAlternatives)
        {
            throw Boundary(
                $"JSON object '{schema.Name}' matches more than one declared schema alternative.");
        }
    }

    private static bool OutputConstraintMatches(
        JsonConstraint constraint,
        JsonSchemaNode schema,
        FerruleInstance instance)
    {
        if (instance is not FerruleScalar scalar)
        {
            return false;
        }

        var value = scalar.Value;
        if (constraint.Type == "json_null")
        {
            return value.Kind == FerruleValueKind.JsonNull &&
                   (schema.Nullable || schema.ContainerNullable);
        }

        if (!schema.IsScalar)
        {
            return false;
        }

        if (schema.IsScalarUnion)
        {
            try
            {
                return TaggedConstraintMatches(
                    constraint,
                    NormalizeScalarUnion(schema, value));
            }
            catch (FerruleRuntimeException)
            {
                return false;
            }
        }

        var domain = schema.ScalarDomain;
        return constraint.Type switch
        {
            "string" when domain.HasFlag(JsonScalarDomain.String) =>
                TryOutputString(value, out var actualString) &&
                string.Equals(
                    actualString,
                    constraint.Expected.GetString(),
                    StringComparison.Ordinal),
            "int" when domain.HasFlag(JsonScalarDomain.Int64) =>
                TryOutputInt64(value, out var actualInteger) &&
                constraint.Expected.TryGetInt64(out var expectedInteger) &&
                actualInteger == expectedInteger,
            "float" when domain.HasFlag(JsonScalarDomain.Double) =>
                TryOutputDouble(value, out var actualNumber) &&
                TryReadExactDouble(constraint.Expected, out var expectedNumber) &&
                actualNumber == expectedNumber,
            "bool" when domain.HasFlag(JsonScalarDomain.Bool) =>
                TryOutputBoolean(value, out var actualBoolean) &&
                constraint.Expected.ValueKind is JsonValueKind.True or JsonValueKind.False &&
                actualBoolean == constraint.Expected.GetBoolean(),
            _ => false,
        };
    }

    private static bool TaggedConstraintMatches(
        JsonConstraint constraint,
        FerruleValue value) =>
        (constraint.Type, value.Kind) switch
        {
            ("string", FerruleValueKind.String) =>
                string.Equals(
                    value.StringValue,
                    constraint.Expected.GetString(),
                    StringComparison.Ordinal),
            ("int", FerruleValueKind.Int64) =>
                constraint.Expected.TryGetInt64(out var expectedInteger) &&
                value.Int64Value == expectedInteger,
            ("float", FerruleValueKind.Double) =>
                TryReadExactDouble(constraint.Expected, out var expectedNumber) &&
                value.DoubleValue == expectedNumber,
            ("bool", FerruleValueKind.Bool) =>
                constraint.Expected.ValueKind is
                    JsonValueKind.True or JsonValueKind.False &&
                value.BooleanValue == constraint.Expected.GetBoolean(),
            _ => false,
        };

    private static bool TryOutputString(FerruleValue value, out string output)
    {
        output = value.Kind switch
        {
            FerruleValueKind.String => value.StringValue,
            FerruleValueKind.Bool => value.BooleanValue ? "true" : "false",
            FerruleValueKind.Int64 => value.Int64Value.ToString(CultureInfo.InvariantCulture),
            FerruleValueKind.Double when double.IsFinite(value.DoubleValue) =>
                value.DoubleValue.ToString("R", CultureInfo.InvariantCulture),
            _ => string.Empty,
        };
        return value.Kind is FerruleValueKind.String or
            FerruleValueKind.Bool or
            FerruleValueKind.Int64 ||
            value.Kind == FerruleValueKind.Double && double.IsFinite(value.DoubleValue);
    }

    private static bool TryOutputInt64(FerruleValue value, out long output)
    {
        if (value.Kind == FerruleValueKind.Int64)
        {
            output = value.Int64Value;
            return true;
        }

        output = 0;
        return value.Kind == FerruleValueKind.String &&
               long.TryParse(
                   value.StringValue.Trim(),
                   NumberStyles.AllowLeadingSign,
                   CultureInfo.InvariantCulture,
                   out output);
    }

    private static bool TryOutputDouble(FerruleValue value, out double output)
    {
        if (value.Kind == FerruleValueKind.Int64 &&
            TryExactDouble(value.Int64Value, out output))
        {
            return true;
        }
        if (value.Kind == FerruleValueKind.Double && double.IsFinite(value.DoubleValue))
        {
            output = value.DoubleValue;
            return true;
        }

        output = 0;
        return value.Kind == FerruleValueKind.String &&
               double.TryParse(
                   value.StringValue.Trim(),
                   NumberStyles.Float,
                   CultureInfo.InvariantCulture,
                   out output) &&
               double.IsFinite(output);
    }

    private static bool TryReadExactDouble(JsonElement value, out double output)
    {
        output = 0;
        if (value.ValueKind != JsonValueKind.Number)
        {
            return false;
        }
        if (value.TryGetInt64(out var integer))
        {
            return TryExactDouble(integer, out output);
        }
        if (value.TryGetUInt64(out var unsignedInteger))
        {
            return TryExactDouble(unsignedInteger, out output);
        }

        return value.TryGetDouble(out output) && double.IsFinite(output);
    }

    private static bool TryExactDouble(long value, out double output)
    {
        output = value;
        return new BigInteger(output) == new BigInteger(value);
    }

    private static bool TryExactDouble(ulong value, out double output)
    {
        output = value;
        return new BigInteger(output) == new BigInteger(value);
    }

    private static bool TryOutputBoolean(FerruleValue value, out bool output)
    {
        if (value.Kind == FerruleValueKind.Bool)
        {
            output = value.BooleanValue;
            return true;
        }
        if (value.Kind == FerruleValueKind.String &&
            string.Equals(value.StringValue.Trim(), "true", StringComparison.Ordinal))
        {
            output = true;
            return true;
        }
        if (value.Kind == FerruleValueKind.String &&
            string.Equals(value.StringValue.Trim(), "false", StringComparison.Ordinal))
        {
            output = false;
            return true;
        }

        output = false;
        return false;
    }

    private static bool IsExplicitJsonNull(FerruleInstance instance) =>
        instance is FerruleScalar { Value.Kind: FerruleValueKind.JsonNull };

    private static bool ConstraintMatches(JsonConstraint constraint, JsonElement value) =>
        constraint.Type switch
        {
            "string" => value.ValueKind == JsonValueKind.String &&
                        string.Equals(
                            constraint.Expected.GetString(),
                            value.GetString(),
                            StringComparison.Ordinal),
            "int" => value.ValueKind == JsonValueKind.Number &&
                     value.TryGetInt64(out var actualInteger) &&
                     constraint.Expected.TryGetInt64(out var expectedInteger) &&
                     actualInteger == expectedInteger,
            "float" => value.ValueKind == JsonValueKind.Number &&
                       TryReadExactDouble(value, out var actualNumber) &&
                       TryReadExactDouble(constraint.Expected, out var expectedNumber) &&
                       actualNumber == expectedNumber,
            "bool" => value.ValueKind is JsonValueKind.True or JsonValueKind.False &&
                      constraint.Expected.ValueKind is JsonValueKind.True or JsonValueKind.False &&
                      value.GetBoolean() == constraint.Expected.GetBoolean(),
            "json_null" => value.ValueKind == JsonValueKind.Null,
            _ => false,
        };

    private static List<JsonProperty> OrderedProperties(JsonElement element)
    {
        var properties = new List<JsonProperty>();
        var indexes = new Dictionary<string, int>(StringComparer.Ordinal);
        foreach (var property in element.EnumerateObject())
        {
            var item = new JsonProperty(property.Name, property.Value);
            if (indexes.TryGetValue(property.Name, out var index))
            {
                properties[index] = item;
            }
            else
            {
                indexes.Add(property.Name, properties.Count);
                properties.Add(item);
            }
        }

        return properties;
    }

    private static string[] RequiredStrings(JsonElement element, string name)
    {
        var values = RequiredProperty(element, name);
        RequireKind(values, JsonValueKind.Array, name, "array");
        return values.EnumerateArray().Select(value =>
        {
            if (value.ValueKind != JsonValueKind.String)
            {
                throw Boundary($"Embedded JSON schema field '{name}' must contain strings.");
            }

            return value.GetString() ?? string.Empty;
        }).ToArray();
    }

    private static JsonElement RequiredProperty(JsonElement element, string name) =>
        element.TryGetProperty(name, out var value)
            ? value
            : throw Boundary($"Embedded JSON schema is missing field '{name}'.");

    private static string RequiredString(JsonElement element, string name)
    {
        var value = RequiredProperty(element, name);
        return value.ValueKind == JsonValueKind.String
            ? value.GetString() ?? string.Empty
            : throw Boundary($"Embedded JSON schema field '{name}' must be a string.");
    }

    private static bool OptionalBoolean(JsonElement element, string name) =>
        element.TryGetProperty(name, out var value) &&
        value.ValueKind switch
        {
            JsonValueKind.True => true,
            JsonValueKind.False => false,
            _ => throw Boundary($"Embedded JSON schema field '{name}' must be a boolean."),
        };

    private static void RequireKind(
        JsonElement element,
        JsonValueKind expected,
        string name,
        string expectedName)
    {
        if (element.ValueKind != expected)
        {
            throw Shape(name, expectedName, element.ValueKind.ToString());
        }
    }

    private static void RequireUtf8Limit(string value, int maximum, string label)
    {
        var bytes = Encoding.UTF8.GetByteCount(value);
        if (bytes > maximum)
        {
            throw Boundary($"{label} is {bytes} bytes; maximum is {maximum}.");
        }
    }

    private static FerruleRuntimeException Shape(string name, string expected, string found) =>
        Boundary($"JSON field '{name}' expected {expected}, got {found}.");

    private static FerruleRuntimeException Boundary(string message) =>
        new(FerruleRuntimeError.JsonBoundary, message, detail: message);

    private static FerruleRuntimeException Boundary(string message, Exception innerException) =>
        new(
            FerruleRuntimeError.JsonBoundary,
            message,
            innerException,
            detail: message);

    private static string ScalarName(JsonScalarType scalar) => scalar switch
    {
        JsonScalarType.String => "string",
        JsonScalarType.Int64 => "integer",
        JsonScalarType.Double => "number",
        JsonScalarType.Bool => "bool",
        _ => "scalar",
    };

    private static string ScalarName(JsonScalarDomain domain) =>
        IsSingleScalar(domain)
            ? ScalarName(SingleScalar(domain))
            : "declared scalar union";

    private static JsonScalarType SingleScalar(JsonScalarDomain domain) => domain switch
    {
        JsonScalarDomain.String => JsonScalarType.String,
        JsonScalarDomain.Int64 => JsonScalarType.Int64,
        JsonScalarDomain.Double => JsonScalarType.Double,
        JsonScalarDomain.Bool => JsonScalarType.Bool,
        _ => throw Boundary("Embedded JSON schema scalar domain is invalid."),
    };

    private static JsonScalarDomain? ValueDomain(FerruleValueKind kind) => kind switch
    {
        FerruleValueKind.String => JsonScalarDomain.String,
        FerruleValueKind.Int64 => JsonScalarDomain.Int64,
        FerruleValueKind.Double => JsonScalarDomain.Double,
        FerruleValueKind.Bool => JsonScalarDomain.Bool,
        _ => null,
    };

    private static bool IsSingleScalar(JsonScalarDomain domain)
    {
        var bits = (int)domain;
        return bits != 0 && (bits & (bits - 1)) == 0;
    }

    private static string InstanceKind(FerruleInstance instance) => instance switch
    {
        FerruleScalar scalar => scalar.Value.Kind.ToString(),
        FerruleGroup => "object",
        FerruleRepeated => "array",
        FerruleMappedSequence => "mapped sequence",
        FerruleDocumentSet => "document set",
        _ => "unknown",
    };

    private enum JsonScalarType
    {
        String,
        Int64,
        Double,
        Bool,
    }

    [Flags]
    private enum JsonScalarDomain
    {
        None = 0,
        String = 1 << 0,
        Int64 = 1 << 1,
        Double = 1 << 2,
        Bool = 1 << 3,
    }

    private sealed record JsonProperty(string Name, JsonElement Value);

    private sealed record OutputProperty(JsonSchemaNode Schema, FerruleInstance Value);

    private sealed record JsonConstraint(string Member, string Type, JsonElement Expected);

    private sealed record JsonAlternative(
        IReadOnlyList<string> Members,
        IReadOnlyList<string> Required,
        IReadOnlyList<JsonConstraint> Constraints);

    private sealed class JsonSchemaNode
    {
        public JsonSchemaNode(
            string name,
            bool repeating,
            bool nullable,
            bool containerNullable,
            bool jsonAny,
            JsonScalarDomain scalarDomain,
            IReadOnlyList<JsonSchemaNode> children,
            JsonSchemaNode? dynamic,
            IReadOnlyList<JsonAlternative> alternatives,
            bool inclusiveAlternatives)
        {
            Name = name;
            Repeating = repeating;
            Nullable = nullable;
            ContainerNullable = containerNullable;
            JsonAny = jsonAny;
            ScalarDomain = scalarDomain;
            Children = children;
            Dynamic = dynamic;
            Alternatives = alternatives;
            InclusiveAlternatives = inclusiveAlternatives;
        }

        public string Name { get; }

        public bool Repeating { get; }

        public bool Nullable { get; }

        public bool ContainerNullable { get; }

        public bool JsonAny { get; }

        public JsonScalarDomain ScalarDomain { get; }

        public bool IsScalar => ScalarDomain != JsonScalarDomain.None;

        public bool IsScalarUnion => IsScalar && !IsSingleScalar(ScalarDomain);

        public IReadOnlyList<JsonSchemaNode> Children { get; }

        public JsonSchemaNode? Dynamic { get; }

        public IReadOnlyList<JsonAlternative> Alternatives { get; }

        public bool InclusiveAlternatives { get; }

        public JsonSchemaNode? Child(string name) =>
            Children.FirstOrDefault(child =>
                string.Equals(child.Name, name, StringComparison.Ordinal));
    }

    private sealed class NodeBudget
    {
        private int _nodes;

        public void Visit(int depth)
        {
            if (depth > MaximumDepth)
            {
                throw Boundary($"JSON nesting exceeds the {MaximumDepth}-level limit.");
            }

            _nodes = checked(_nodes + 1);
            if (_nodes > MaximumNodes)
            {
                throw Boundary($"JSON document exceeds the {MaximumNodes}-node limit.");
            }
        }
    }
}
