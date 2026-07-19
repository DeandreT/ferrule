using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void ScalarFunctionBatchB()
    {
        SubstringFunctions();
        SqlLikeFunction();
        PaddingFunctions();
        IsbnFunction();
        RoundFunction();
        DateFromDateTimeFunction();
    }

    private static void SubstringFunctions()
    {
        CallEquals(Text(" car"), "substring", Text("motor car"), FerruleValue.FromInt64(6));
        CallEquals(
            Text("ada"),
            "substring",
            Text("metadata"),
            FerruleValue.FromInt64(4),
            FerruleValue.FromInt64(3));
        CallEquals(
            Text("\U0001F642b"),
            "substring",
            Text("a\U0001F642bc"),
            FerruleValue.FromInt64(2),
            FerruleValue.FromInt64(2));
        CallEquals(
            Text("b"),
            "substring",
            Text("abc"),
            FerruleValue.FromDouble(1.5),
            FerruleValue.FromInt64(1));
        CallEquals(Text("abc"), "substring", Text("abc"), FerruleValue.FromDouble(double.NaN));
        CallEquals(
            Text(string.Empty),
            "substring",
            Text("abc"),
            FerruleValue.FromDouble(double.PositiveInfinity));
        CallEquals(
            Text(string.Empty),
            "substring",
            Text("abc"),
            FerruleValue.FromInt64(long.MaxValue),
            FerruleValue.FromInt64(long.MaxValue));

        AssertFunctionArity("substring", 2, Text("value"));
        AssertFunctionArity(
            "substring",
            2,
            Text("value"),
            FerruleValue.FromInt64(1),
            FerruleValue.FromInt64(2),
            FerruleValue.FromInt64(3));
        AssertFunctionType("substring", FerruleValue.FromInt64(1));
        AssertFunctionTypeKind(
            "substring",
            FerruleValueKind.String,
            Text("value"),
            Text("1"));
    }

    private static void SqlLikeFunction()
    {
        foreach (var (value, pattern, expected) in new[]
        {
            ("Baker", "B%", true),
            ("baker", "B%", true),
            ("Baker", "%ake_", true),
            ("Baker", "B_k_r", true),
            ("Baker", "B_k", false),
            (string.Empty, "%", true),
            (string.Empty, "_", false),
            ("\U0001F642", "_", true),
            ("é", "É", false),
        })
        {
            CallEquals(Bool(expected), "sql_like", Text(value), Text(pattern));
        }

        AssertFunctionArity("sql_like", 2, Text("value"));
        AssertFunctionType("sql_like", Text("value"), Bool(true));
    }

    private static void PaddingFunctions()
    {
        CallEquals(
            Text("007"),
            "pad_string_left",
            FerruleValue.FromInt64(7),
            FerruleValue.FromDouble(3.9),
            FerruleValue.FromInt64(0));
        CallEquals(
            Text("AP\U0001F642\U0001F642"),
            "pad_string_right",
            Text("AP"),
            FerruleValue.FromInt64(4),
            Text("\U0001F642"));
        CallEquals(
            Text("already-long"),
            "pad_string_left",
            Text("already-long"),
            FerruleValue.FromInt64(3),
            Text("x"));
        CallEquals(
            Text("AP"),
            "pad_string_right",
            Text("AP"),
            FerruleValue.FromInt64(-3),
            Text("x"));

        AssertFunctionArity("pad_string_left", 3, Text("value"));
        AssertFunctionTypeKind(
            "pad_string_right",
            FerruleValueKind.String,
            Text("value"),
            Text("3"),
            Text("x"));
        AssertInvalidArgument(
            "pad_string_left",
            "requires one padding character",
            Text("value"),
            FerruleValue.FromInt64(7),
            Text(string.Empty));
        AssertInvalidArgument(
            "pad_string_right",
            "requires one padding character",
            Text("value"),
            FerruleValue.FromInt64(7),
            Text("xy"));
        AssertInvalidArgument(
            "pad_string_left",
            "requested output exceeds 1000000 characters",
            Text(string.Empty),
            FerruleValue.FromInt64(1_000_001),
            Text("x"));
        foreach (var value in new[]
        {
            double.NaN,
            double.PositiveInfinity,
            double.NegativeInfinity,
        })
        {
            AssertInvalidArgument(
                "pad_string_right",
                "requires a finite desired length",
                Text(string.Empty),
                FerruleValue.FromDouble(value),
                Text("x"));
        }
    }

    private static void IsbnFunction()
    {
        CallEquals(Text("9780764549649"), "isbn10_to_isbn13", Text("0-7645-4964-2"));
        CallEquals(Text("9780804429573"), "isbn10_to_isbn13", Text("080442957X"));
        CallEquals(Text("9780804429573"), "isbn10_to_isbn13", Text("080442957x"));
        AssertInvalidArgument(
            "isbn10_to_isbn13",
            "ISBN-10 check digit is invalid",
            Text("0764549643"));
        AssertInvalidArgument(
            "isbn10_to_isbn13",
            "expected a 10-character ISBN with an optional final X check digit",
            Text("978-0-123"));
        AssertFunctionArity("isbn10_to_isbn13", 1, Array.Empty<FerruleValue>());
        AssertFunctionType("isbn10_to_isbn13", FerruleValue.FromInt64(764549642));
    }

    private static void RoundFunction()
    {
        CallEquals(FerruleValue.FromDouble(3.0), "round", FerruleValue.FromDouble(2.5));
        CallEquals(FerruleValue.FromDouble(-3.0), "round", FerruleValue.FromDouble(-2.5));
        CallEquals(FerruleValue.FromInt64(7), "round", FerruleValue.FromInt64(7));
        CallEquals(
            FerruleValue.FromDouble(1.23),
            "round",
            FerruleValue.FromDouble(1.23456),
            FerruleValue.FromInt64(2));
        CallEquals(
            FerruleValue.FromDouble(1.23),
            "round",
            FerruleValue.FromDouble(1.23456),
            FerruleValue.FromDouble(1.6));
        CallEquals(
            FerruleValue.FromDouble(2.0),
            "round",
            FerruleValue.FromDouble(2.4),
            FerruleValue.FromDouble(double.NaN));

        var negativeZero = FerruleFunctions.Call(
            "round",
            new[] { FerruleValue.FromDouble(-0.1) });
        Equal(
            BitConverter.DoubleToInt64Bits(-0.0),
            BitConverter.DoubleToInt64Bits(negativeZero.DoubleValue));
        Equal(
            true,
            double.IsNaN(FerruleFunctions.Call(
                "round",
                new[] { FerruleValue.FromDouble(double.NaN) }).DoubleValue));
        Equal(
            double.PositiveInfinity,
            FerruleFunctions.Call(
                "round",
                new[] { FerruleValue.FromDouble(double.PositiveInfinity) }).DoubleValue);

        AssertFunctionArity("round", 1, Array.Empty<FerruleValue>());
        AssertFunctionArity(
            "round",
            1,
            FerruleValue.FromInt64(1),
            FerruleValue.FromInt64(2),
            FerruleValue.FromInt64(3));
        AssertFunctionTypeKind("round", FerruleValueKind.String, Text("2.5"));
        AssertFunctionTypeKind(
            "round",
            FerruleValueKind.Bool,
            FerruleValue.FromDouble(2.5),
            Bool(false));
    }

    private static void DateFromDateTimeFunction()
    {
        CallEquals(
            Text("2024-03-01"),
            "date_from_datetime",
            Text("2024-03-01T10:30:00"));
        CallEquals(Text("2024-03-01"), "date_from_datetime", Text("2024-03-01"));
        CallEquals(
            Text("2024-03-01"),
            "date_from_datetime",
            Text("\u3000\u00A02024-03-01\u205F T10:30:00"));
        CallEquals(Text(string.Empty), "date_from_datetime", Text("T10:30:00"));
        AssertFunctionArity("date_from_datetime", 1, Array.Empty<FerruleValue>());
        AssertFunctionType("date_from_datetime", FerruleValue.Null);
    }

    private static void AssertInvalidArgument(
        string function,
        string detail,
        params FerruleValue[] arguments)
    {
        var error = Error(
            FerruleRuntimeError.FunctionInvalidArgument,
            () => FerruleFunctions.Call(function, arguments));
        Equal(function, error.Function);
        Equal(detail, error.Detail);
    }

    private static void AssertFunctionTypeKind(
        string function,
        FerruleValueKind expectedKind,
        params FerruleValue[] arguments)
    {
        var error = Error(
            FerruleRuntimeError.FunctionType,
            () => FerruleFunctions.Call(function, arguments));
        Equal(function, error.Function);
        Equal(expectedKind, error.FoundKind);
    }
}
