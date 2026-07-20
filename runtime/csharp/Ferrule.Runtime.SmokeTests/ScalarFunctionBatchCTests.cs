using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void ScalarFunctionBatchC()
    {
        TrimFunction();
        NumericConversionFunctions();
        DelayPassthroughFunction();
    }

    private static void TrimFunction()
    {
        CallEquals(
            Text("value"),
            "trim",
            Text("\u0085\u00A0\u2003value\u202F\u3000"));
        CallEquals(Text(""), "trim", Text("\t\r\n\u205F"));
        CallEquals(Text("\u200Bvalue\u200B"), "trim", Text("\u200Bvalue\u200B"));
        AssertFunctionArity("trim", 1, Array.Empty<FerruleValue>());
        AssertFunctionType("trim", FerruleValue.FromInt64(1));
    }

    private static void NumericConversionFunctions()
    {
        foreach (var value in new[]
        {
            FerruleValue.FromInt64(long.MinValue),
            FerruleValue.FromInt64(long.MaxValue),
            FerruleValue.FromDouble(-0.0),
            FerruleValue.FromDouble(1.25e-12),
            Text("  +42  "),
            Text("-9223372036854775808"),
            Text("6.022e23"),
            Text("\u200312.5\u3000"),
        })
        {
            CallEquals(Bool(true), "is_numeric", value);
        }

        foreach (var value in new[]
        {
            FerruleValue.Null,
            FerruleValue.XmlNil,
            Bool(true),
            FerruleValue.FromDouble(double.NaN),
            FerruleValue.FromDouble(double.PositiveInfinity),
            Text(""),
            Text("NaN"),
            Text("1e309"),
            Text("1,000"),
        })
        {
            CallEquals(Bool(false), "is_numeric", value);
        }

        CallEquals(FerruleValue.Null, "to_number", FerruleValue.Null);
        CallEquals(FerruleValue.FromInt64(42), "to_number", Text("\u00A0+42\u2003"));
        CallEquals(
            FerruleValue.FromInt64(long.MinValue),
            "to_number",
            Text("-9223372036854775808"));
        CallEquals(
            FerruleValue.FromInt64(long.MaxValue),
            "to_number",
            Text("9223372036854775807"));
        CallEquals(
            FerruleValue.FromDouble(9.223372036854776e18),
            "to_number",
            Text("9223372036854775808"));
        CallEquals(FerruleValue.FromDouble(1250.0), "to_number", Text("1.25e3"));
        CallEquals(FerruleValue.FromDouble(-0.0), "to_number", FerruleValue.FromDouble(-0.0));

        AssertFunctionArity("is_numeric", 1, Text("1"), Text("2"));
        AssertFunctionArity("to_number", 1, Array.Empty<FerruleValue>());
        foreach (var value in new[]
        {
            FerruleValue.XmlNil,
            Bool(false),
            FerruleValue.FromDouble(double.NaN),
            FerruleValue.FromDouble(double.NegativeInfinity),
            Text("not a number"),
            Text("1e309"),
        })
        {
            AssertInvalidArgument(
                "to_number",
                "requires a finite numeric value",
                value);
        }
    }

    private static void DelayPassthroughFunction()
    {
        foreach (var value in new[]
        {
            FerruleValue.Null,
            FerruleValue.XmlNil,
            Bool(true),
            FerruleValue.FromInt64(7),
            FerruleValue.FromDouble(-0.0),
            Text("unchanged"),
        })
        {
            CallEquals(
                value,
                "delay_passthrough",
                value,
                FerruleValue.FromDouble(0.25));
        }

        AssertFunctionArity("delay_passthrough", 2, Text("value"));
        AssertFunctionTypeKind(
            "delay_passthrough",
            FerruleValueKind.String,
            Text("value"),
            Text("1"));
        foreach (var duration in new[]
        {
            -0.01,
            double.NaN,
            double.PositiveInfinity,
            double.NegativeInfinity,
        })
        {
            AssertInvalidArgument(
                "delay_passthrough",
                "requires a finite nonnegative duration",
                Text("value"),
                FerruleValue.FromDouble(duration));
        }
    }
}
