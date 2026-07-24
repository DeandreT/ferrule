using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void ScalarFunctionBatchC()
    {
        TrimFunction();
        NumericConversionFunctions();
        FormatNumberFunction();
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
        CallEquals(Bool(false), "boolean", FerruleValue.Null);
        CallEquals(Bool(false), "boolean", FerruleValue.XmlNil);
        CallEquals(Bool(false), "boolean", FerruleValue.FromDouble(double.NaN));
        CallEquals(Bool(false), "boolean", FerruleValue.FromDouble(-0.0));
        CallEquals(Bool(true), "boolean", FerruleValue.FromInt64(-1));
        CallEquals(Bool(true), "boolean", Text("false"));
        CallEquals(FerruleValue.FromInt64(-7), "positive", FerruleValue.FromInt64(-7));
        CallEquals(FerruleValue.FromDouble(2.5), "positive", Text("2.5"));
        CallEquals(FerruleValue.FromInt64(-7), "floor", FerruleValue.FromInt64(-7));
        CallEquals(FerruleValue.FromDouble(-3.0), "floor", FerruleValue.FromDouble(-2.1));

        AssertFunctionArity("boolean", 1, Array.Empty<FerruleValue>());
        AssertFunctionType("positive", Bool(true));
        AssertInvalidArgument(
            "floor",
            "requires a finite numeric value",
            FerruleValue.FromDouble(double.PositiveInfinity));

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

    private static void FormatNumberFunction()
    {
        CallEquals(
            Text("1,234.50"),
            "format_number",
            FerruleValue.FromDouble(1234.5),
            Text("#,##0.00"));
        CallEquals(
            Text("123.46"),
            "format_number",
            FerruleValue.FromDouble(123.456),
            Text("#,##0.00"));
        CallEquals(
            Text("0.0003"),
            "format_number",
            FerruleValue.FromDouble(0.00025),
            Text("###0.0###"));
        CallEquals(
            Text("00025.00"),
            "format_number",
            FerruleValue.FromInt64(25),
            Text("00000.00"));
        CallEquals(
            Text("1.01"),
            "format_number",
            FerruleValue.FromDouble(1.005),
            Text("0.00"));
        CallEquals(
            Text("(3.12)"),
            "format_number",
            FerruleValue.FromDouble(-3.12),
            Text("#.00;(#.00)"));
        CallEquals(
            Text("74%"),
            "format_number",
            FerruleValue.FromDouble(0.736),
            Text("#00%"));
        CallEquals(
            Text("736\u2030"),
            "format_number",
            FerruleValue.FromDouble(0.736),
            Text("#00\u2030"));
        CallEquals(
            Text("1.234,50"),
            "format_number",
            FerruleValue.FromDouble(1234.5),
            Text("#.##0,00"),
            Text(","),
            Text("."));
        CallEquals(
            Text("1\U0001F642234\u00B750"),
            "format_number",
            FerruleValue.FromDouble(1234.5),
            Text("#\U0001F642##0\u00B700"),
            Text("\u00B7"),
            Text("\U0001F642"));
        CallEquals(
            Text("-9223372036854775808"),
            "format_number",
            FerruleValue.FromInt64(long.MinValue),
            Text("0"));
        CallEquals(
            Text("9,007,199,254,740,993"),
            "format_number",
            FerruleValue.FromInt64(9_007_199_254_740_993),
            Text("#,##0"));
        CallEquals(
            Text("0"),
            "format_number",
            FerruleValue.FromInt64(0),
            Text("#.##"));
        CallEquals(
            Text("$0 USD"),
            "format_number",
            FerruleValue.FromInt64(0),
            Text("$#.## USD"));

        var precisionPicture = "0." + new string('0', 400);
        CallEquals(
            Text("1." + new string('0', 400)),
            "format_number",
            FerruleValue.FromDouble(1.0),
            Text(precisionPicture));

        var maximum = FerruleFunctions.Call(
            "format_number",
            new[] { FerruleValue.FromDouble(double.MaxValue), Text("0%") });
        Equal(FerruleValueKind.String, maximum.Kind);
        Equal(true, maximum.StringValue.EndsWith('%'));
        Equal(false, maximum.StringValue.Contains("inf", StringComparison.Ordinal));

        AssertFunctionArity("format_number", 2, FerruleValue.FromInt64(1));
        AssertFunctionArity(
            "format_number",
            2,
            FerruleValue.FromInt64(1),
            Text("0"),
            Text("."),
            Text(","),
            Text("extra"));
        AssertFunctionTypeKind(
            "format_number",
            FerruleValueKind.Bool,
            FerruleValue.FromInt64(1),
            Bool(false));
        AssertFunctionTypeKind(
            "format_number",
            FerruleValueKind.Bool,
            Bool(false),
            Text("0"));
        AssertInvalidArgument(
            "format_number",
            "format contains placeholders in an invalid order",
            FerruleValue.FromInt64(1),
            Text("#0#"));
        foreach (var picture in new[]
        {
            ".#0",
            "0;0;0",
            "0;;0",
            "0%%",
            "0%\u2030",
            "#,,##0",
        })
        {
            var error = Error(
                FerruleRuntimeError.FunctionInvalidArgument,
                () => FerruleFunctions.Call(
                    "format_number",
                    new[] { FerruleValue.FromInt64(1), Text(picture) }));
            Equal("format_number", error.Function);
        }
        AssertInvalidArgument(
            "format_number",
            "separator collides with a picture character",
            FerruleValue.FromInt64(1),
            Text("0"),
            Text("#"));
        AssertInvalidArgument(
            "format_number",
            "requires a finite number",
            FerruleValue.FromDouble(double.PositiveInfinity),
            Text("0"));
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
