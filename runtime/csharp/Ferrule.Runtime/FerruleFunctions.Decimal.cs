namespace Ferrule.Runtime;

public static partial class FerruleFunctions
{
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
