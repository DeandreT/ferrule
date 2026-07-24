using System.Buffers;
using System.Globalization;
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

    private const long MaximumExactDoubleInteger = 1L << 53;

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
        var scalar = kind switch
        {
            "scalar" => RequiredString(kindElement, "ty") switch
            {
                "string" => JsonScalarType.String,
                "int" => JsonScalarType.Int64,
                "float" => JsonScalarType.Double,
                "bool" => JsonScalarType.Bool,
                var found => throw Boundary(
                    $"Embedded JSON schema node '{name}' has unknown scalar type '{found}'."),
            },
            "group" => (JsonScalarType?)null,
            _ => throw Boundary(
                $"Embedded JSON schema node '{name}' has unknown kind '{kind}'."),
        };
        var children = new List<JsonSchemaNode>();
        JsonSchemaNode? dynamic = null;
        var alternatives = new List<JsonAlternative>();
        if (scalar is null)
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
            scalar,
            children,
            dynamic,
            alternatives,
            element.TryGetProperty("alternative_mode", out var mode) &&
            mode.ValueKind == JsonValueKind.String &&
            string.Equals(mode.GetString(), "inclusive", StringComparison.Ordinal));
    }

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

        if (schema.Scalar is { } scalar)
        {
            return new FerruleScalar(ReadScalar(schema, scalar, element));
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
        JsonScalarType scalar,
        JsonElement element)
    {
        if (element.ValueKind == JsonValueKind.Null && schema.Nullable)
        {
            return FerruleValue.JsonNull;
        }

        return scalar switch
        {
            JsonScalarType.String when element.ValueKind == JsonValueKind.String =>
                FerruleValue.FromString(element.GetString() ?? string.Empty),
            JsonScalarType.Int64 when element.ValueKind == JsonValueKind.Number &&
                                      element.TryGetInt64(out var integer) =>
                FerruleValue.FromInt64(integer),
            JsonScalarType.Double when element.ValueKind == JsonValueKind.Number =>
                ReadDouble(schema.Name, element),
            JsonScalarType.Bool when element.ValueKind is JsonValueKind.True or JsonValueKind.False =>
                FerruleValue.FromBoolean(element.GetBoolean()),
            _ => throw Shape(schema.Name, ScalarName(scalar), element.ValueKind.ToString()),
        };
    }

    private static FerruleValue ReadDouble(string name, JsonElement element)
    {
        if (element.TryGetInt64(out var integer) &&
            Math.Abs((double)integer) > MaximumExactDoubleInteger)
        {
            throw Shape(name, "number", "integer outside the exact double range");
        }
        if (element.TryGetUInt64(out var unsignedInteger) &&
            unsignedInteger > MaximumExactDoubleInteger)
        {
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

        return schema.Scalar is null
            ? new FerruleGroup(Array.Empty<FerruleField>())
            : new FerruleScalar(FerruleValue.Null);
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

        if (schema.Scalar is { } scalar)
        {
            if (instance is not FerruleScalar value)
            {
                throw Shape(schema.Name, ScalarName(scalar), InstanceKind(instance));
            }

            WriteScalar(writer, schema, scalar, value.Value);
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
                writer.WriteNumberValue(scalar.Value.DoubleValue);
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
                when Math.Abs((double)value.Int64Value) <= MaximumExactDoubleInteger:
                writer.WriteNumberValue(value.Int64Value);
                return;
            case (JsonScalarType.Double, FerruleValueKind.Double)
                when double.IsFinite(value.DoubleValue):
                writer.WriteNumberValue(value.DoubleValue);
                return;
            case (JsonScalarType.Double, FerruleValueKind.String)
                when double.TryParse(
                         value.StringValue.Trim(),
                         NumberStyles.Float,
                         CultureInfo.InvariantCulture,
                         out var number) &&
                     double.IsFinite(number):
                writer.WriteNumberValue(number);
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

    private static bool BoundaryAbsence(JsonSchemaNode schema, FerruleInstance instance) =>
        instance is FerruleScalar { Value.Kind: FerruleValueKind.Null } &&
        (schema.ContainerNullable || !schema.Repeating && schema.Scalar is not null);

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

        if (schema.Scalar is not { } scalarType)
        {
            return false;
        }

        return constraint.Type switch
        {
            "string" when scalarType == JsonScalarType.String =>
                TryOutputString(value, out var actualString) &&
                string.Equals(
                    actualString,
                    constraint.Expected.GetString(),
                    StringComparison.Ordinal),
            "int" when scalarType == JsonScalarType.Int64 =>
                TryOutputInt64(value, out var actualInteger) &&
                constraint.Expected.TryGetInt64(out var expectedInteger) &&
                actualInteger == expectedInteger,
            "float" when scalarType == JsonScalarType.Double =>
                TryOutputDouble(value, out var actualNumber) &&
                constraint.Expected.TryGetDouble(out var expectedNumber) &&
                actualNumber == expectedNumber,
            "bool" when scalarType == JsonScalarType.Bool =>
                TryOutputBoolean(value, out var actualBoolean) &&
                constraint.Expected.ValueKind is JsonValueKind.True or JsonValueKind.False &&
                actualBoolean == constraint.Expected.GetBoolean(),
            _ => false,
        };
    }

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
            Math.Abs((double)value.Int64Value) <= MaximumExactDoubleInteger)
        {
            output = value.Int64Value;
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
                       value.TryGetDouble(out var actualNumber) &&
                       constraint.Expected.TryGetDouble(out var expectedNumber) &&
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
            JsonScalarType? scalar,
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
            Scalar = scalar;
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

        public JsonScalarType? Scalar { get; }

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
