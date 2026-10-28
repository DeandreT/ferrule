using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void FloatTextCoercion()
    {
        foreach (var (text, expected) in new[] {
            (" -0 ", -0.0), (" 1.25e2\n", 125.0),
            ("9007199254740993", 9007199254740992.0),
            ("9223372036854775807", 9223372036854775808.0),
        })
        {
            var actual = FerruleFunctions.Call("coerce_float", new[] { Text(text) });
            Equal(FerruleValueKind.Double, actual.Kind);
            Equal(BitConverter.DoubleToInt64Bits(expected), BitConverter.DoubleToInt64Bits(actual.DoubleValue));
        }
        foreach (var text in new[] { "", " ", "not-a-number", "NaN", "inf", "-inf", "1e9999" })
        {
            AssertInvalidArgument("coerce_float", "requires finite floating-point text", Text(text));
        }
        AssertFunctionArity("coerce_float", 1, Array.Empty<FerruleValue>());
        foreach (var value in new[] {
            FerruleValue.Null, FerruleValue.JsonNull, FerruleValue.XmlNil,
            FerruleValue.FromBoolean(true), FerruleValue.FromInt64(long.MaxValue),
            FerruleValue.FromDouble(1.25), FerruleValue.FromDecimal(FerruleDecimal96.ParseExact("1.25")),
        })
        {
            CallEquals(value, "coerce_float", value);
        }
        var nan = BitConverter.Int64BitsToDouble(unchecked((long)0x7ff8000000000042UL));
        var passed = FerruleFunctions.Call("coerce_float", new[] { FerruleValue.FromDouble(nan) });
        Equal(FerruleValueKind.Double, passed.Kind);
        Equal(BitConverter.DoubleToInt64Bits(nan), BitConverter.DoubleToInt64Bits(passed.DoubleValue));
    }
}
