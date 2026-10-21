using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void DecimalValueIdentity()
    {
        Equal(6, (int)FerruleValueKind.String);
        foreach (var text in new[] { "0", "0.1", "-1.25", "9007199254740993", "79228162514264337593543950335", "0.0000000000000000000000000001" })
        {
            var number = FerruleDecimal96.ParseExact(text);
            var value = FerruleValue.FromDecimal(number);
            Equal(FerruleValueKind.Decimal, value.Kind);
            Equal(number, value.DecimalValue);
            Equal(text, value.ToString());
            Equal(value, FerruleValue.FromDecimal(FerruleDecimal96.ParseExact(text)));
            Equal(value.GetHashCode(), FerruleValue.FromDecimal(number).GetHashCode());
            NotEqual(value, FerruleValue.FromString(text));
            Error(FerruleRuntimeError.ValueKindMismatch, () => { _ = value.DoubleValue; });
        }
        NotEqual(FerruleValue.FromDecimal(FerruleDecimal96.ParseExact("1")), FerruleValue.FromInt64(1));
        NotEqual(FerruleValue.FromDecimal(FerruleDecimal96.ParseExact("0.1")), FerruleValue.FromDouble(0.1));
        Equal(FerruleValueKind.Null, default(FerruleValue).Kind);
    }
}
