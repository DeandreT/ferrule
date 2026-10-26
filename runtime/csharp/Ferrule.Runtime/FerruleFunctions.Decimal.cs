namespace Ferrule.Runtime;

public static partial class FerruleFunctions
{
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
            return FerruleValue.FromDecimal(FerruleDecimal96.ParseDecimalText(value.StringValue));
        }
        catch (FerruleDecimalException error)
        {
            throw new FerruleRuntimeException(FerruleRuntimeError.DecimalConversion,
                $"`{function}`: {error.Message}", error, function: function, detail: error.Error.ToString());
        }
    }
}
