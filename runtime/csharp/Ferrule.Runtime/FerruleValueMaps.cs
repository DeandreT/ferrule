using System.Globalization;
using System.Text;

namespace Ferrule.Runtime;

/// <summary>Scalar types that a value map can apply before matching.</summary>
public enum FerruleScalarType
{
    String,
    Int64,
    Double,
    Bool,
}

/// <summary>One ordered input/output pair in a value map.</summary>
public readonly record struct FerruleValueMapEntry(
    FerruleValue From,
    FerruleValue To);

/// <summary>Applies engine-compatible typed, ordered value-map lookups.</summary>
public static class FerruleValueMaps
{
    public static FerruleValue Apply(
        FerruleValue input,
        FerruleScalarType? inputType,
        IReadOnlyList<FerruleValueMapEntry> table,
        FerruleValue? defaultValue = null)
    {
        ArgumentNullException.ThrowIfNull(table);
        var value = inputType.HasValue ? Coerce(input, inputType.Value) : input;
        foreach (var entry in table)
        {
            if (entry.From == value)
            {
                return entry.To;
            }
        }

        return defaultValue ?? FerruleValue.Null;
    }

    private static FerruleValue Coerce(FerruleValue value, FerruleScalarType type)
    {
        if (value.Kind is FerruleValueKind.Null or FerruleValueKind.XmlNil)
        {
            return value;
        }

        return type switch
        {
            FerruleScalarType.String => CoerceString(value),
            FerruleScalarType.Int64 => CoerceInt64(value),
            FerruleScalarType.Double => CoerceDouble(value),
            FerruleScalarType.Bool => CoerceBool(value),
            _ => throw new ArgumentOutOfRangeException(nameof(type), type, null),
        };
    }

    private static FerruleValue CoerceString(FerruleValue value) => value.Kind switch
    {
        FerruleValueKind.String => value,
        FerruleValueKind.Bool => FerruleValue.FromString(
            value.BooleanValue ? "true" : "false"),
        FerruleValueKind.Int64 => FerruleValue.FromString(
            value.Int64Value.ToString(CultureInfo.InvariantCulture)),
        FerruleValueKind.Double when double.IsFinite(value.DoubleValue) =>
            FerruleValue.FromString(RustFloatText(value.DoubleValue)),
        _ => value,
    };

    private static FerruleValue CoerceInt64(FerruleValue value)
    {
        if (value.Kind == FerruleValueKind.Int64)
        {
            return value;
        }
        if (value.Kind == FerruleValueKind.Double)
        {
            var number = value.DoubleValue;
            if (double.IsFinite(number) &&
                Math.Truncate(number) == number &&
                number >= (double)long.MinValue &&
                number < -(double)long.MinValue)
            {
                return FerruleValue.FromInt64((long)number);
            }
            return value;
        }
        if (value.Kind == FerruleValueKind.String &&
            long.TryParse(
                value.StringValue.AsSpan().Trim(),
                NumberStyles.Integer,
                CultureInfo.InvariantCulture,
                out var integer))
        {
            return FerruleValue.FromInt64(integer);
        }
        return value;
    }

    private static FerruleValue CoerceDouble(FerruleValue value)
    {
        if (value.Kind == FerruleValueKind.Double)
        {
            return value;
        }
        if (value.Kind == FerruleValueKind.Int64)
        {
            return FerruleValue.FromDouble(value.Int64Value);
        }
        if (value.Kind == FerruleValueKind.String &&
            double.TryParse(
                value.StringValue.AsSpan().Trim(),
                NumberStyles.Float,
                CultureInfo.InvariantCulture,
                out var number) &&
            double.IsFinite(number))
        {
            return FerruleValue.FromDouble(number);
        }
        return value;
    }

    private static FerruleValue CoerceBool(FerruleValue value)
    {
        if (value.Kind == FerruleValueKind.Bool)
        {
            return value;
        }
        if (value.Kind != FerruleValueKind.String)
        {
            return value;
        }

        return value.StringValue.AsSpan().Trim() switch
        {
            "true" or "1" => FerruleValue.FromBoolean(true),
            "false" or "0" => FerruleValue.FromBoolean(false),
            _ => value,
        };
    }

    internal static string RustFloatText(double value)
    {
        var text = value.ToString("R", CultureInfo.InvariantCulture);
        var exponentIndex = text.IndexOf('E', StringComparison.Ordinal);
        if (exponentIndex < 0)
        {
            return text;
        }

        var negative = text[0] == '-';
        var mantissaStart = negative ? 1 : 0;
        var mantissa = text.AsSpan(mantissaStart, exponentIndex - mantissaStart);
        var decimalIndex = mantissa.IndexOf('.');
        var decimalPosition = decimalIndex < 0 ? mantissa.Length : decimalIndex;
        var digits = decimalIndex < 0
            ? mantissa.ToString()
            : string.Concat(mantissa[..decimalIndex], mantissa[(decimalIndex + 1)..]);
        var exponent = int.Parse(
            text.AsSpan(exponentIndex + 1),
            NumberStyles.AllowLeadingSign,
            CultureInfo.InvariantCulture);
        var outputDecimalPosition = decimalPosition + exponent;
        var output = new StringBuilder(
            digits.Length + Math.Abs(exponent) + 3);
        if (negative)
        {
            output.Append('-');
        }
        if (outputDecimalPosition <= 0)
        {
            output.Append("0.");
            output.Append('0', -outputDecimalPosition);
            output.Append(digits);
        }
        else if (outputDecimalPosition >= digits.Length)
        {
            output.Append(digits);
            output.Append('0', outputDecimalPosition - digits.Length);
        }
        else
        {
            output.Append(digits.AsSpan(0, outputDecimalPosition));
            output.Append('.');
            output.Append(digits.AsSpan(outputDecimalPosition));
        }
        return output.ToString();
    }
}
