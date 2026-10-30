namespace Ferrule.Runtime;

public static partial class FerruleFunctions
{
    private static FerruleValue MapforceIntegerQuotientFloor(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "mapforce_integer_quotient_floor";
        RequireArity(function, arguments, 2);
        foreach (var argument in arguments)
            if (argument.Kind != FerruleValueKind.Decimal) throw Type(function, argument);
        static decimal Native(FerruleDecimal96 value) => new decimal(
            unchecked((int)(uint)value.Coefficient),
            unchecked((int)(uint)(value.Coefficient >> 32)),
            unchecked((int)(uint)(value.Coefficient >> 64)),
            value.IsNegative, value.Scale);
        var left = decimal.Truncate(Native(arguments[0].DecimalValue));
        var right = decimal.Truncate(Native(arguments[1].DecimalValue));
        if (right == 0)
            throw new FerruleRuntimeException(FerruleRuntimeError.DivideByZero, "Division by zero.");
        var result = decimal.Floor(left / right);
        var bits = decimal.GetBits(result);
        var coefficient = (UInt128)unchecked((uint)bits[0])
            | ((UInt128)unchecked((uint)bits[1]) << 32)
            | ((UInt128)unchecked((uint)bits[2]) << 64);
        return FerruleValue.FromDecimal(FerruleDecimal96.FromParts(result < 0, coefficient, 0));
    }

    private static FerruleValue DecimalTruncate(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "decimal_truncate";
        RequireArity(function, arguments, 1);
        if (arguments[0].Kind != FerruleValueKind.Decimal)
            throw Type(function, arguments[0]);
        var value = arguments[0].DecimalValue;
        UInt128 divisor = 1;
        for (int i = 0; i < value.Scale; i++) divisor *= 10;
        return FerruleValue.FromDecimal(FerruleDecimal96.FromParts(value.IsNegative, value.Coefficient / divisor, 0));
    }

    private static FerruleValue DecimalFromNullableText(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "decimal_from_nullable_text";
        RequireArity(function, arguments, 1);
        var value = arguments[0];
        if (value.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull)
            return FerruleValue.FromDecimal(FerruleDecimal96.Zero);
        if (value.Kind != FerruleValueKind.String)
            throw Type(function, value);
        try
        {
            return FerruleValue.FromDecimal(FerruleDecimal96.ParseNumber(value.StringValue));
        }
        catch (FerruleDecimalException error)
        {
            throw new FerruleRuntimeException(FerruleRuntimeError.DecimalConversion,
                $"`{function}`: {error.Message}", error, function: function, detail: error.Error.ToString());
        }
    }
}
