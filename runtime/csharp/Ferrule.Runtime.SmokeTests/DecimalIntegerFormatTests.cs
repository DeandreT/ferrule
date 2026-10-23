using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void DecimalIntegerFormat()
    {
        foreach (var (input, expected) in new[] {
            ("0", "0"), ("-0", "0"), ("0.1", "0"), ("0.49", "0"), ("0.5", "1"),
            ("0.5000000000000000000000000001", "1"), ("0.999", "1"), ("1.5", "2"), ("2.5", "3"),
            ("-0.5", "-1"), ("-1.5", "-2"), ("9007199254740993.5", "9007199254740994"),
            ("-0.1", "-0"), ("-0.49", "-0"), ("-0.4999999999999999999999999999", "-0"),
            ("0.4999999999999999999999999999", "0"), ("1.0000000000000000000000000001", "1"),
            ("-1.0000000000000000000000000001", "-1"), ("-0.0000000000000000000000000001", "-0"),
            ("79228162514264337593543950335", "79228162514264337593543950335"),
            ("-79228162514264337593543950335", "-79228162514264337593543950335"),
        })
        {
            var value = FerruleValue.FromDecimal(FerruleDecimal96.ParseExact(input));
            Equal(expected, FerruleFunctions.Call("format_decimal_integer", new[] { value }).StringValue);
        }
        Throws<FerruleRuntimeException>(() => FerruleFunctions.Call("format_decimal_integer", new[] { FerruleValue.FromInt64(1) }));
    }
}
