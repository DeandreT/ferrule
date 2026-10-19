using System.Globalization;

namespace Ferrule.Runtime;

/// <summary>Typed scalar boundaries used by generated user functions.</summary>
public static class FerruleUserFunctions
{
    public static FerruleValue Adapt(
        FerruleValue value,
        FerruleScalarType expected,
        ulong function,
        ulong? parameter)
    {
        if (value.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull or FerruleValueKind.XmlNil ||
            Matches(value.Kind, expected))
        {
            return value;
        }

        FerruleValue? adapted = expected switch
        {
            FerruleScalarType.String => StringValue(value),
            FerruleScalarType.Int64 => Int64Value(value),
            FerruleScalarType.Double => DoubleValue(value),
            FerruleScalarType.Bool => BooleanValue(value),
            _ => null,
        };
        if (adapted.HasValue)
        {
            return adapted.Value;
        }

        var location = parameter.HasValue
            ? $"parameter {parameter.Value}"
            : "output";
        throw new FerruleRuntimeException(
            FerruleRuntimeError.UserFunctionType,
            $"user function {function} {location}: expected {expected}, got {value.Kind}",
            foundKind: value.Kind,
            userFunction: function,
            functionParameter: parameter,
            expectedScalarType: expected);
    }

    private static bool Matches(FerruleValueKind kind, FerruleScalarType expected) =>
        (kind, expected) switch
        {
            (FerruleValueKind.String, FerruleScalarType.String) or
            (FerruleValueKind.Int64, FerruleScalarType.Int64) or
            (FerruleValueKind.Double, FerruleScalarType.Double) or
            (FerruleValueKind.Bool, FerruleScalarType.Bool) => true,
            _ => false,
        };

    private static FerruleValue? StringValue(FerruleValue value) => value.Kind switch
    {
        FerruleValueKind.Bool => FerruleValue.FromString(
            value.BooleanValue ? "true" : "false"),
        FerruleValueKind.Int64 => FerruleValue.FromString(
            value.Int64Value.ToString(CultureInfo.InvariantCulture)),
        FerruleValueKind.Double when double.IsFinite(value.DoubleValue) =>
            FerruleValue.FromString(FerruleValueMaps.RustFloatText(value.DoubleValue)),
        _ => null,
    };

    private static FerruleValue? Int64Value(FerruleValue value)
    {
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
        }
        else if (value.Kind == FerruleValueKind.String &&
                 long.TryParse(
                     value.StringValue.AsSpan().Trim(),
                     NumberStyles.Integer,
                     CultureInfo.InvariantCulture,
                     out var integer))
        {
            return FerruleValue.FromInt64(integer);
        }
        return null;
    }

    private static FerruleValue? DoubleValue(FerruleValue value)
    {
        if (value.Kind == FerruleValueKind.Int64)
        {
            var integer = value.Int64Value;
            var number = (double)integer;
            if ((long)number == integer)
            {
                return FerruleValue.FromDouble(number);
            }
        }
        else if (value.Kind == FerruleValueKind.String &&
                 double.TryParse(
                     value.StringValue.AsSpan().Trim(),
                     NumberStyles.Float,
                     CultureInfo.InvariantCulture,
                     out var number) &&
                 double.IsFinite(number))
        {
            return FerruleValue.FromDouble(number);
        }
        return null;
    }

    private static FerruleValue? BooleanValue(FerruleValue value)
    {
        if (value.Kind != FerruleValueKind.String)
        {
            return null;
        }
        return value.StringValue.AsSpan().Trim() switch
        {
            "true" or "1" => FerruleValue.FromBoolean(true),
            "false" or "0" => FerruleValue.FromBoolean(false),
            _ => null,
        };
    }
}
