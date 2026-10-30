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
        foreach (var (input, expected) in new[] {
            ("1.9", "1"), ("-1.9", "-1"), ("0.9", "0"), ("-0.9", "0"),
            ("9223372036854775808.9", "9223372036854775808"),
            ("79228162514264337593543950335", "79228162514264337593543950335"),
            ("0.0000000000000000000000000001", "0"),
        })
        {
            var value = FerruleValue.FromDecimal(FerruleDecimal96.ParseExact(input));
            Equal(expected, FerruleFunctions.Call("decimal_truncate", new[] { value }).DecimalValue.ToString());
        }
        Throws<FerruleRuntimeException>(() => FerruleFunctions.Call("decimal_truncate", Array.Empty<FerruleValue>()));
        Throws<FerruleRuntimeException>(() => FerruleFunctions.Call("decimal_truncate", new[] { FerruleValue.FromInt64(1) }));
        foreach (var (left, right, expected) in new[] {
            ("7.9", "3.9", "2"), ("-7.9", "3.9", "-3"), ("7.9", "-3.9", "-3"),
            ("-7.9", "-3.9", "2"), ("0.9", "3", "0"),
            ("-1", "79228162514264337593543950335", "0"),
            ("-1", "10000000000000000000000000000", "-1"),
            ("79228162514264337593543950335", "1", "79228162514264337593543950335"),
            ("71305346262837903834189555302", "9", "7922816251426433759354395034"),
            ("-71305346262837903834189555302", "9", "-7922816251426433759354395034"),
        })
        {
            var values = new[] { left, right }.Select(text => FerruleValue.FromDecimal(FerruleDecimal96.ParseExact(text))).ToArray();
            Equal(expected, FerruleFunctions.Call("mapforce_integer_quotient_floor", values).DecimalValue.ToString());
        }
        Throws<FerruleRuntimeException>(() => FerruleFunctions.Call("mapforce_integer_quotient_floor", Array.Empty<FerruleValue>()));
        Throws<FerruleRuntimeException>(() => FerruleFunctions.Call("mapforce_integer_quotient_floor", new[] { FerruleValue.FromInt64(1), FerruleValue.FromInt64(0) }));
        Throws<FerruleRuntimeException>(() => FerruleFunctions.Call("mapforce_integer_quotient_floor", new[] {
            FerruleValue.FromDecimal(FerruleDecimal96.ParseExact("1")),
            FerruleValue.FromDecimal(FerruleDecimal96.ParseExact("0.9")) }));
        var oraclePath = Environment.GetEnvironmentVariable("FERRULE_NATIVE_DECIMAL_ORACLE");
        if (!string.IsNullOrEmpty(oraclePath))
        {
            using var oracle = System.Text.Json.JsonDocument.Parse(File.ReadAllBytes(oraclePath));
            foreach (var row in oracle.RootElement.EnumerateArray())
            {
                var values = new[] { "left", "right" }.Select(key =>
                    FerruleValue.FromDecimal(FerruleDecimal96.ParseExact(row.GetProperty(key).GetString()!))).ToArray();
                var expected = row.GetProperty("value").GetString();
                if (expected is not null)
                    Equal(expected, FerruleFunctions.Call("mapforce_integer_quotient_floor", values).DecimalValue.ToString());
                else
                {
                    Equal("DivideByZeroException", row.GetProperty("error").GetString());
                    try
                    {
                        FerruleFunctions.Call("mapforce_integer_quotient_floor", values);
                        throw new Exception("Expected native quotient division failure.");
                    }
                    catch (FerruleRuntimeException failure) when (failure.Error == FerruleRuntimeError.DivideByZero) { }
                }
            }
        }
    }
}
