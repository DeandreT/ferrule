using System.Globalization;
using System.Text;
using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string CorrectRoundingFloatSchema =
        """{"name":"Number","kind":{"kind":"scalar","ty":"float"}}""";
    private const string CorrectRoundingIntSchema =
        """{"name":"Number","kind":{"kind":"scalar","ty":"int"}}""";

    private static void JsonCorrectlyRoundedNumbers()
    {
        JsonCorrectlyRoundedLiteralBits();
        JsonCorrectlyRoundedExponentTails();
        JsonCorrectlyRoundedDomainsAndRefusals();
    }

    // Fixed binary64 oracles, independent of the parser and formatter under test.
    private static void JsonCorrectlyRoundedLiteralBits()
    {
        var cases = new (string Token, ulong Bits, string Canonical)[]
        {
            ("0.0", 0x0000000000000000UL, "0.0"),
            ("-0.0", 0x8000000000000000UL, "-0.0"),
            ("1.0", 0x3ff0000000000000UL, "1.0"),
            ("-1.0", 0xbff0000000000000UL, "-1.0"),
            ("9007199254740990.0", 0x433ffffffffffffeUL, "9007199254740990.0"),
            ("9007199254740991.0", 0x433fffffffffffffUL, "9007199254740991.0"),
            ("9007199254740992.0", 0x4340000000000000UL, "9007199254740992.0"),
            ("9007199254740994.0", 0x4340000000000001UL, "9007199254740994.0"),
            ("-9007199254740990.0", 0xc33ffffffffffffeUL, "-9007199254740990.0"),
            ("-9007199254740991.0", 0xc33fffffffffffffUL, "-9007199254740991.0"),
            ("-9007199254740992.0", 0xc340000000000000UL, "-9007199254740992.0"),
            ("-9007199254740994.0", 0xc340000000000001UL, "-9007199254740994.0"),
            ("9007199254740993.0", 0x4340000000000000UL, "9007199254740992.0"),
            ("9007199254740995.0", 0x4340000000000002UL, "9007199254740996.0"),
            ("-9007199254740993.0", 0xc340000000000000UL, "-9007199254740992.0"),
            ("-9007199254740995.0", 0xc340000000000002UL, "-9007199254740996.0"),
            ("5e-324", 0x0000000000000001UL, "5e-324"),
            ("-5e-324", 0x8000000000000001UL, "-5e-324"),
            ("1e-323", 0x0000000000000002UL, "1e-323"),
            ("2.225073858507201e-308", 0x000fffffffffffffUL, "2.225073858507201e-308"),
            ("2.2250738585072014e-308", 0x0010000000000000UL, "2.2250738585072014e-308"),
            ("-2.2250738585072014e-308", 0x8010000000000000UL, "-2.2250738585072014e-308"),
            ("1.7976931348623157e308", 0x7fefffffffffffffUL, "1.7976931348623157e+308"),
            ("-1.7976931348623157e308", 0xffefffffffffffffUL, "-1.7976931348623157e+308"),
            ("0.09999999999999999", 0x3fb9999999999999UL, "0.09999999999999999"),
            ("0.1", 0x3fb999999999999aUL, "0.1"),
            ("0.10000000000000002", 0x3fb999999999999bUL, "0.10000000000000002"),
            ("0.9999999999999999", 0x3fefffffffffffffUL, "0.9999999999999999"),
            ("1.0000000000000002", 0x3ff0000000000001UL, "1.0000000000000002"),
            ("-1.2345678901234567", 0xbff3c0ca428c59fbUL, "-1.2345678901234567"),
            ("1e-307", 0x0031fa182c40c60dUL, "1e-307"),
            ("1.0000000000000001e-307", 0x0031fa182c40c60eUL, "1.0000000000000001e-307"),
            ("-1e-307", 0x8031fa182c40c60dUL, "-1e-307"),
            ("-1.0000000000000001e-307", 0x8031fa182c40c60eUL, "-1.0000000000000001e-307"),
            ("1.00000000000000011102230246251565404236316680908203125", 0x3ff0000000000000UL, "1.0"),
            ("1.000000000000000111022302462515654042363166809082031251", 0x3ff0000000000001UL, "1.0000000000000002"),
            ("2.4703282292062327e-324", 0x0000000000000000UL, "0.0"),
            ("2.4703282292062328e-324", 0x0000000000000001UL, "5e-324"),
            ("1.7976931348623158e308", 0x7fefffffffffffffUL, "1.7976931348623157e+308"),
            ("-1.7976931348623158e308", 0xffefffffffffffffUL, "-1.7976931348623157e+308"),
            ("18446744073709551616", 0x43f0000000000000UL, "1.8446744073709552e+19"),
            ("-9223372036854775809", 0xc3e0000000000000UL, "-9.223372036854776e+18"),
        };
        foreach (var (token, bits, canonical) in cases)
        {
            AssertCorrectRoundingToken(token, bits, canonical);
        }
    }

    private static void JsonCorrectlyRoundedExponentTails()
    {
        foreach (var (token, bits, canonical) in new (string, ulong, string)[]
                 {
                     ("0e999999999999999999999999999999999999", 0, "0.0"),
                     ("-0e999999999999999999999999999999999999", 0x8000000000000000UL, "-0.0"),
                     ("1e-999999999999999999999999999999999999", 0, "0.0"),
                     ("-1e-999999999999999999999999999999999999", 0x8000000000000000UL, "-0.0"),
                     ("1e-400", 0, "0.0"),
                     ("-1e-400", 0x8000000000000000UL, "-0.0"),
                     ("0e-309", 0, "0.0"),
                     ("-0e-309", 0x8000000000000000UL, "-0.0"),
                 })
        {
            AssertCorrectRoundingToken(token, bits, canonical);
        }
        foreach (var sign in new[] { "", "-" })
        {
            var bits = sign == "-" ? 0x8000000000000000UL : 0UL;
            var zero = sign == "-" ? "-0.0" : "0.0";
            foreach (var finalDigit in new[] { "1", "0" })
            {
                AssertCorrectRoundingToken(
                    sign + "0." + new string('0', 4096) + finalDigit, bits, zero);
            }
            AssertCorrectRoundingToken(sign + "0.0e-2147483647", bits, zero);
        }
        // A nonzero tail far beyond a short significand still breaks the tie.
        AssertCorrectRoundingToken(
            "1.00000000000000011102230246251565404236316680908203125" +
            new string('0', 4096) + "1",
            0x3ff0000000000001UL, "1.0000000000000002");
    }

    private static void JsonCorrectlyRoundedDomainsAndRefusals()
    {
        foreach (var (token, integer) in new (string, long)[]
                 {
                     ("0", 0),
                     ("9007199254740993", 9_007_199_254_740_993),
                     ("-9007199254740993", -9_007_199_254_740_993),
                     ("9223372036854775807", long.MaxValue),
                     ("-9223372036854775808", long.MinValue),
                 })
        {
            var text = CaptureCorrectRoundingScalar(token + ": int text",
                () => FerruleJson.Parse(CorrectRoundingIntSchema, token));
            var bytes = CaptureCorrectRoundingScalar(token + ": int bytes",
                () => FerruleJson.ParseBytes(CorrectRoundingIntSchema, Encoding.UTF8.GetBytes(token)));
            var field = CaptureCorrectRounding(token + ": int field",
                () => FerruleFunctions.Call("json_parse_field", new[]
                {
                    Text(token), Text(CorrectRoundingIntSchema), Text("[]"),
                }));
            Equal(FerruleValue.FromInt64(integer), text);
            Equal(text, bytes);
            Equal(text, field);
        }
        foreach (var token in new[]
                 {
                     "9007199254740993", "-9007199254740993",
                     "9223372036854775807", "18446744073709551615",
                 })
        {
            RequireCorrectRoundingRefusal(token + ": exact integer to float",
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(CorrectRoundingFloatSchema, token));
        }
        foreach (var token in new[]
                 {
                     "-0", "1.0", "1e0", "9223372036854775808",
                     "18446744073709551615", "18446744073709551616", "-9223372036854775809",
                 })
        {
            RequireCorrectRoundingRefusal(token + ": integer tag refusal",
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(CorrectRoundingIntSchema, token));
        }
        foreach (var token in new[]
                 {
                     "9007199254740993", "9223372036854775808", "18446744073709551615",
                 })
        {
            var arbitrary = CaptureCorrectRoundingScalar(token + ": arbitrary integer",
                () => FerruleJson.Parse(JsonAnyScalarSchema, token));
            var rendered = CaptureCorrectRounding(token + ": arbitrary integer output",
                () => FerruleJson.Serialize(JsonAnyScalarSchema, Scalar(arbitrary)));
            Equal(Text(token), arbitrary);
            Equal(token + "\n", rendered);
        }
        AssertCorrectRoundingToken("-0", 0x8000000000000000UL, "-0.0");
        AssertCorrectRoundingToken("9223372036854775808", 0x43e0000000000000UL,
            "9223372036854775808");
        foreach (var token in new[]
                 {
                     "1e309", "-1e309", "1.7976931348623159e308", "-1.7976931348623159e308",
                     "1e999999999999999999999999999999999999",
                     "-1e999999999999999999999999999999999999",
                     "+1", "01", "1.", ".1", "1e", "NaN", "Infinity", "1,000",
                 })
        {
            RequireCorrectRoundingRefusal(token + ": text refusal", FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(CorrectRoundingFloatSchema, token));
            RequireCorrectRoundingRefusal(token + ": byte refusal", FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.ParseBytes(CorrectRoundingFloatSchema, Encoding.UTF8.GetBytes(token)));
            RequireCorrectRoundingRefusal(token + ": arbitrary refusal", FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(JsonAnyScalarSchema, token));
            RequireCorrectRoundingRefusal(token + ": field refusal", FerruleRuntimeError.FunctionInvalidArgument,
                () => FerruleFunctions.Call("json_parse_field", new[]
                {
                    Text(token), Text(CorrectRoundingFloatSchema), Text("[]"),
                }));
        }
        RequireCorrectRoundingRefusal("invalid UTF-8 bytes", FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.ParseBytes(CorrectRoundingFloatSchema, new byte[] { 0xff }));
        foreach (var number in new[] { double.PositiveInfinity, double.NegativeInfinity, double.NaN })
        {
            RequireCorrectRoundingRefusal("nonfinite output " + number, FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Serialize(CorrectRoundingFloatSchema, Scalar(FerruleValue.FromDouble(number))));
        }
        const string exactLowRange =
            """{"name":"Number","numeric_range":{"kind":"number","bounds":{"minimum":{"value":1e-307},"maximum":{"value":1e-307}}},"kind":{"kind":"scalar","ty":"float"}}""";
        var low = CaptureCorrectRoundingScalar("exact low metadata",
            () => FerruleJson.Parse(exactLowRange, "1e-307"));
        AssertCorrectRoundingBits(low, 0x0031fa182c40c60dUL);
        RequireCorrectRoundingRefusal("adjacent high metadata refusal", FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(exactLowRange, "1.0000000000000001e-307"));
        RequireCorrectRoundingRefusal("adjacent high output refusal", FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(exactLowRange,
                Scalar(FerruleValue.FromDouble(BitConverter.Int64BitsToDouble(0x0031fa182c40c60e)))));
        const string exactMaxRange =
            """{"name":"Number","numeric_range":{"kind":"number","bounds":{"minimum":{"value":1.7976931348623158e308},"maximum":{"value":1.7976931348623158e308}}},"kind":{"kind":"scalar","ty":"float"}}""";
        var maximum = CaptureCorrectRoundingScalar("rounded max-finite metadata",
            () => FerruleJson.Parse(exactMaxRange, "1.7976931348623158e308"));
        AssertCorrectRoundingBits(maximum, 0x7fefffffffffffffUL);
        var maximumOutput = CaptureCorrectRounding("max-finite metadata output",
            () => FerruleJson.Serialize(exactMaxRange, Scalar(maximum)));
        Equal("1.7976931348623157e+308\n", maximumOutput);
        var overflowMetadata = exactMaxRange.Replace(
            "1.7976931348623158", "1.7976931348623159", StringComparison.Ordinal);
        RequireCorrectRoundingRefusal("overflow metadata input refusal", FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(overflowMetadata, "0.0"));
        RequireCorrectRoundingRefusal("overflow metadata output refusal", FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(overflowMetadata, Scalar(maximum)));
    }

    private static void AssertCorrectRoundingToken(string token, ulong bits, string canonical)
    {
        var encoded = Encoding.UTF8.GetBytes(token);
        var text = CaptureCorrectRoundingScalar(token + ": typed text",
            () => FerruleJson.Parse(CorrectRoundingFloatSchema, token));
        var bytes = CaptureCorrectRoundingScalar(token + ": typed bytes",
            () => FerruleJson.ParseBytes(CorrectRoundingFloatSchema, encoded));
        var embedded = CaptureCorrectRoundingScalar(token + ": embedded text",
            () => FerruleJson.ParseEmbedded(CorrectRoundingFloatSchema, token));
        var embeddedBytes = CaptureCorrectRoundingScalar(token + ": embedded bytes",
            () => FerruleJson.ParseEmbeddedBytes(CorrectRoundingFloatSchema, encoded));
        var field = CaptureCorrectRounding(token + ": typed field",
            () => FerruleFunctions.Call("json_parse_field", new[]
            {
                Text(token), Text(CorrectRoundingFloatSchema), Text("[]"),
            }));
        var arbitrary = CaptureCorrectRoundingScalar(token + ": arbitrary text",
            () => FerruleJson.Parse(JsonAnyScalarSchema, token));
        var arbitraryBytes = CaptureCorrectRoundingScalar(token + ": arbitrary bytes",
            () => FerruleJson.ParseBytes(JsonAnyScalarSchema, encoded));
        var arbitraryField = CaptureCorrectRounding(token + ": arbitrary field",
            () => FerruleFunctions.Call("json_parse_field", new[]
            {
                Text(token), Text(JsonAnyScalarSchema), Text("[]"),
            }));
        foreach (var actual in new[] { text, bytes, embedded, embeddedBytes, field })
        {
            AssertCorrectRoundingBits(actual, bits);
        }
        Equal(Text(canonical), arbitrary);
        Equal(arbitrary, arbitraryBytes);
        Equal(arbitrary, arbitraryField);
        var rendered = CaptureCorrectRounding(token + ": arbitrary output",
            () => FerruleJson.Serialize(JsonAnyScalarSchema, Scalar(arbitrary)));
        Equal(canonical + "\n", rendered);
        // Typed float output can use floating syntax even when input had an integer tag.
        var typedRendered = CaptureCorrectRounding(token + ": typed output",
            () => FerruleJson.Serialize(CorrectRoundingFloatSchema, Scalar(text)));
        var reparsed = CaptureCorrectRoundingScalar(token + ": typed output reload",
            () => FerruleJson.Parse(CorrectRoundingFloatSchema, typedRendered));
        AssertCorrectRoundingBits(reparsed, bits);
    }

    private static void AssertCorrectRoundingBits(FerruleValue actual, ulong bits)
    {
        Equal(FerruleValueKind.Double, actual.Kind);
        Equal(bits, unchecked((ulong)BitConverter.DoubleToInt64Bits(actual.DoubleValue)));
    }

    private static FerruleValue CaptureCorrectRoundingScalar(string label, Func<FerruleInstance> read)
    {
        var instance = CaptureCorrectRounding(label, read);
        return instance is FerruleScalar scalar
            ? scalar.Value
            : throw new InvalidOperationException("Expected scalar after retaining full instance: " + label);
    }

    private static T CaptureCorrectRounding<T>(string label, Func<T> read)
    {
        try
        {
            var value = read();
            Console.WriteLine(label + ": actual " + CorrectRoundingSnapshot(value));
            return value;
        }
        catch (Exception error)
        {
            RetainCorrectRoundingError(label, error);
            throw;
        }
    }

    private static void RequireCorrectRoundingRefusal<T>(
        string label, FerruleRuntimeError expected, Func<T> read)
    {
        try
        {
            var value = read();
            Console.WriteLine(label + ": actual success before refusal assertion " +
                CorrectRoundingSnapshot(value));
        }
        catch (Exception error)
        {
            RetainCorrectRoundingError(label, error);
            if (error is FerruleRuntimeException runtime)
            {
                Equal(expected, runtime.Error);
                if (expected == FerruleRuntimeError.FunctionInvalidArgument)
                {
                    Equal("json_parse_field", runtime.Function);
                }
                return;
            }
            throw;
        }
        throw new InvalidOperationException("Expected refusal after retaining result: " + label);
    }

    private static void RetainCorrectRoundingError(string label, Exception error)
    {
        Console.WriteLine(label + ": actual exception " + error);
        if (error is FerruleRuntimeException runtime)
        {
            Console.WriteLine(label + ": complete runtime payload " + JsonSerializer.Serialize(new
            {
                runtime.Error, runtime.Node, runtime.Function, runtime.ExpectedArity, runtime.ActualArity,
                runtime.FoundKind, runtime.AggregateOperation, runtime.Detail,
                runtime.RequestedItems, runtime.MaximumItems, runtime.MaximumDepth, runtime.RuntimeValue,
                runtime.FailureRule, runtime.MappingFailureMessage, runtime.Join, runtime.UserFunction,
                runtime.FunctionParameter, runtime.ExpectedScalarType, runtime.RuntimeParameter,
                runtime.SourceField, runtime.FoundInstance, runtime.PrimaryRoot, runtime.MappingExceptionMessage,
            }));
        }
    }

    private static string CorrectRoundingSnapshot(object? value) => value switch
    {
        FerruleScalar scalar => "Scalar(" + CorrectRoundingSnapshot(scalar.Value) + ")",
        FerruleGroup group => "Group(origin=" + JsonSerializer.Serialize(group.XmlTypeOrigin) + "; " +
            string.Join(", ", group.Fields.Select(field =>
                JsonSerializer.Serialize(field.Name) + ": " + CorrectRoundingSnapshot(field.Value))) + ")",
        FerruleRepeated repeated => "Repeated[" +
            string.Join(", ", repeated.Items.Select(item => CorrectRoundingSnapshot(item))) + "]",
        FerruleMappedSequence mapped => "MappedSequence[" +
            string.Join(", ", mapped.Items.Select(item => CorrectRoundingSnapshot(item))) + "]",
        FerruleDocumentSet documents => "DocumentSet[" +
            string.Join(", ", documents.Documents.Select(document =>
                JsonSerializer.Serialize(document.Path) + "/resolved=" +
                JsonSerializer.Serialize(document.ResolvedSourcePath) + ": " +
                CorrectRoundingSnapshot(document.Value))) + "]",
        FerruleValue scalar => scalar.Kind + ": " + JsonSerializer.Serialize(scalar.ToString()) +
            (scalar.Kind == FerruleValueKind.Double
                ? "; bits=" + unchecked((ulong)BitConverter.DoubleToInt64Bits(scalar.DoubleValue))
                    .ToString("x16", CultureInfo.InvariantCulture)
                : string.Empty),
        string text => JsonSerializer.Serialize(text),
        byte[] bytes => "bytes=" + Convert.ToHexString(bytes),
        _ => value?.ToString() ?? "null",
    };
}
