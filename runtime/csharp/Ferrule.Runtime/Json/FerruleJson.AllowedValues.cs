using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleJson
{
    private const int MaximumJsonAllowedValues = 4096;
    private const int MaximumJsonAllowedValueStringBytes = 256 * 1024;
    private const int MaximumJsonAllowedValueTotalStringBytes = 1024 * 1024;
    private const double Int64UpperExclusive = 9_223_372_036_854_775_808.0;

    private static JsonAllowedValues? ReadJsonAllowedValues(
        string name,
        JsonElement element,
        JsonScalarDomain scalarDomain,
        bool jsonAny,
        bool nullable,
        FerruleValue? fixedValue)
    {
        JsonElement? declaredValues = null;
        foreach (var property in element.EnumerateObject())
        {
            if (!string.Equals(property.Name, "json_allowed_values", StringComparison.Ordinal))
            {
                continue;
            }
            if (declaredValues is not null)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has duplicate JSON allowed-values metadata.");
            }
            declaredValues = property.Value;
        }
        if (declaredValues is not { } valuesElement ||
            valuesElement.ValueKind == JsonValueKind.Null)
        {
            return null;
        }
        RequireKind(
            valuesElement,
            JsonValueKind.Array,
            $"schema node '{name}' JSON allowed values",
            "array");
        if (scalarDomain == JsonScalarDomain.None || jsonAny || fixedValue is not null)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has JSON allowed values outside a non-fixed concrete scalar domain.");
        }

        var values = new List<JsonAllowedValue>();
        var totalStringBytes = 0;
        foreach (var valueElement in valuesElement.EnumerateArray())
        {
            if (values.Count == MaximumJsonAllowedValues)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has more than {MaximumJsonAllowedValues} JSON allowed values.");
            }
            var value = ReadJsonAllowedValue(name, valueElement);
            if (!value.IsAdmittedBy(scalarDomain, nullable))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has a JSON allowed value outside its declared scalar domain.");
            }
            if (value.Kind == JsonAllowedValueKind.String)
            {
                var stringBytes = StrictUtf8.GetByteCount(value.Value.StringValue);
                if (stringBytes > MaximumJsonAllowedValueStringBytes)
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' has a JSON allowed string larger than {MaximumJsonAllowedValueStringBytes} UTF-8 bytes.");
                }
                totalStringBytes = checked(totalStringBytes + stringBytes);
                if (totalStringBytes > MaximumJsonAllowedValueTotalStringBytes)
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' has more than {MaximumJsonAllowedValueTotalStringBytes} UTF-8 bytes across JSON allowed strings.");
                }
            }
            if (values.Count != 0)
            {
                var comparison = values[^1].CanonicalCompareTo(value);
                if (comparison == 0)
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' has a duplicate JSON allowed value.");
                }
                if (comparison > 0)
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' JSON allowed values are not in canonical order.");
                }
            }
            values.Add(value);
        }
        if (values.Count < 2)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' must have at least two distinct JSON allowed values.");
        }

        var allowedValues = new JsonAllowedValues(values);
        if (nullable != allowedValues.ContainsJsonNull)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' nullability does not match its JSON allowed values.");
        }
        return allowedValues;
    }

    private static JsonAllowedValue ReadJsonAllowedValue(
        string name,
        JsonElement element)
    {
        RequireKind(
            element,
            JsonValueKind.Object,
            $"schema node '{name}' JSON allowed value",
            "object");
        var fields = new HashSet<string>(StringComparer.Ordinal);
        foreach (var property in element.EnumerateObject())
        {
            if (property.Name is not ("type" or "value") ||
                !fields.Add(property.Name))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' JSON allowed value has an unknown or duplicate field '{property.Name}'.");
            }
        }
        var type = RequiredString(element, "type");
        if (string.Equals(type, "json_null", StringComparison.Ordinal))
        {
            if (fields.Contains("value"))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' JSON null allowed value cannot have a payload.");
            }
            return JsonAllowedValue.JsonNull;
        }

        var payload = RequiredProperty(element, "value");
        return type switch
        {
            "bool" when payload.ValueKind is JsonValueKind.True or JsonValueKind.False =>
                JsonAllowedValue.FromBoolean(payload.GetBoolean()),
            "int" when payload.ValueKind == JsonValueKind.Number &&
                       payload.TryGetInt64(out var integer) =>
                JsonAllowedValue.FromInt64(integer),
            "float" when TryReadExactDouble(payload, out var number) &&
                         !TryExactInt64(number, out _) =>
                JsonAllowedValue.FromDouble(number),
            "string" when payload.ValueKind == JsonValueKind.String =>
                JsonAllowedValue.FromString(payload.GetString() ?? string.Empty),
            "bool" or "int" or "float" or "string" => throw Boundary(
                $"Embedded JSON schema node '{name}' has an invalid or non-canonical JSON allowed {type} payload."),
            _ => throw Boundary(
                $"Embedded JSON schema node '{name}' has unknown JSON allowed value type '{type}'."),
        };
    }

    private static void ValidateJsonAllowedValues(
        JsonSchemaNode schema,
        FerruleValue value)
    {
        if (schema.JsonAllowedValues is { } allowedValues &&
            !allowedValues.Matches(value))
        {
            throw Boundary(
                $"JSON scalar '{schema.Name}' is not one of its declared allowed values: {value}.");
        }
    }

    private static void ValidateJsonAllowedValuesOutput(
        JsonSchemaNode schema,
        JsonScalarType scalar,
        FerruleValue value)
    {
        if (schema.JsonAllowedValues is null)
        {
            return;
        }
        if (value.Kind == FerruleValueKind.JsonNull && schema.Nullable)
        {
            ValidateJsonAllowedValues(schema, value);
            return;
        }

        FerruleValue normalized;
        if (scalar == JsonScalarType.String && TryOutputString(value, out var text))
        {
            normalized = FerruleValue.FromString(text);
        }
        else if (scalar == JsonScalarType.Int64 && TryOutputInt64(value, out var integer))
        {
            normalized = FerruleValue.FromInt64(integer);
        }
        else if (scalar == JsonScalarType.Double && TryOutputDouble(value, out var number))
        {
            normalized = FerruleValue.FromDouble(number);
        }
        else if (scalar == JsonScalarType.Bool && TryOutputBoolean(value, out var boolean))
        {
            normalized = FerruleValue.FromBoolean(boolean);
        }
        else
        {
            return;
        }
        ValidateJsonAllowedValues(schema, normalized);
    }

    private static bool TryExactInt64(double value, out long integer)
    {
        integer = 0;
        if (!double.IsFinite(value) ||
            Math.Truncate(value) != value ||
            value < long.MinValue ||
            value >= Int64UpperExclusive)
        {
            return false;
        }
        integer = (long)value;
        return integer == value;
    }

    private enum JsonAllowedValueKind
    {
        JsonNull,
        Bool,
        Int64,
        Double,
        String,
    }

    private readonly record struct JsonAllowedValue(
        JsonAllowedValueKind Kind,
        FerruleValue Value)
    {
        public static JsonAllowedValue JsonNull =>
            new(JsonAllowedValueKind.JsonNull, FerruleValue.JsonNull);

        public static JsonAllowedValue FromBoolean(bool value) =>
            new(JsonAllowedValueKind.Bool, FerruleValue.FromBoolean(value));

        public static JsonAllowedValue FromInt64(long value) =>
            new(JsonAllowedValueKind.Int64, FerruleValue.FromInt64(value));

        public static JsonAllowedValue FromDouble(double value) =>
            new(JsonAllowedValueKind.Double, FerruleValue.FromDouble(value));

        public static JsonAllowedValue FromString(string value) =>
            new(JsonAllowedValueKind.String, FerruleValue.FromString(value));

        public bool IsAdmittedBy(JsonScalarDomain domain, bool nullable) =>
            Kind switch
            {
                JsonAllowedValueKind.JsonNull => nullable,
                JsonAllowedValueKind.Bool => domain.HasFlag(JsonScalarDomain.Bool),
                JsonAllowedValueKind.Int64 =>
                    domain.HasFlag(JsonScalarDomain.Int64) ||
                    domain.HasFlag(JsonScalarDomain.Double),
                JsonAllowedValueKind.Double => domain.HasFlag(JsonScalarDomain.Double),
                JsonAllowedValueKind.String => domain.HasFlag(JsonScalarDomain.String),
                _ => false,
            };

        public int CanonicalCompareTo(JsonAllowedValue other)
        {
            var kindComparison = Kind.CompareTo(other.Kind);
            if (kindComparison != 0)
            {
                return kindComparison;
            }
            return Kind switch
            {
                JsonAllowedValueKind.JsonNull => 0,
                JsonAllowedValueKind.Bool =>
                    Value.BooleanValue.CompareTo(other.Value.BooleanValue),
                JsonAllowedValueKind.Int64 =>
                    Value.Int64Value.CompareTo(other.Value.Int64Value),
                JsonAllowedValueKind.Double =>
                    TotalOrderKey(Value.DoubleValue).CompareTo(
                        TotalOrderKey(other.Value.DoubleValue)),
                JsonAllowedValueKind.String =>
                    CompareUtf8Strings(Value.StringValue, other.Value.StringValue),
                _ => 0,
            };
        }

        public bool Matches(FerruleValue actual) =>
            (Kind, actual.Kind) switch
            {
                (JsonAllowedValueKind.JsonNull, FerruleValueKind.JsonNull) => true,
                (JsonAllowedValueKind.Bool, FerruleValueKind.Bool) =>
                    Value.BooleanValue == actual.BooleanValue,
                (JsonAllowedValueKind.Int64, FerruleValueKind.Int64) =>
                    Value.Int64Value == actual.Int64Value,
                (JsonAllowedValueKind.Int64, FerruleValueKind.Double) =>
                    TryExactInt64(actual.DoubleValue, out var integer) &&
                    Value.Int64Value == integer,
                (JsonAllowedValueKind.Double, FerruleValueKind.Double) =>
                    Value.DoubleValue == actual.DoubleValue,
                (JsonAllowedValueKind.Double, FerruleValueKind.Int64) =>
                    TryExactDouble(actual.Int64Value, out var number) &&
                    Value.DoubleValue == number,
                (JsonAllowedValueKind.String, FerruleValueKind.String) =>
                    string.Equals(
                        Value.StringValue,
                        actual.StringValue,
                        StringComparison.Ordinal),
                _ => false,
            };

        private static long TotalOrderKey(double value)
        {
            var bits = BitConverter.DoubleToInt64Bits(value);
            var negativeMask = (long)((ulong)(bits >> 63) >> 1);
            return bits ^ negativeMask;
        }

        private static int CompareUtf8Strings(string left, string right)
        {
            var leftRunes = left.EnumerateRunes().GetEnumerator();
            var rightRunes = right.EnumerateRunes().GetEnumerator();
            while (true)
            {
                var hasLeft = leftRunes.MoveNext();
                var hasRight = rightRunes.MoveNext();
                if (!hasLeft || !hasRight)
                {
                    return hasLeft.CompareTo(hasRight);
                }
                var comparison = leftRunes.Current.Value.CompareTo(
                    rightRunes.Current.Value);
                if (comparison != 0)
                {
                    return comparison;
                }
            }
        }
    }

    private sealed record JsonAllowedValues(
        IReadOnlyList<JsonAllowedValue> Values)
    {
        public bool ContainsJsonNull =>
            Values.Count != 0 &&
            Values[0].Kind == JsonAllowedValueKind.JsonNull;

        public bool Matches(FerruleValue value) =>
            Values.Any(candidate => candidate.Matches(value));
    }
}
