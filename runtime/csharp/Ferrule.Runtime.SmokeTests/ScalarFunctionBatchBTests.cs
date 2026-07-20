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
        DateTimeExtractorFunctions();
        DateTimeCompositionFunctions();
        DateTimePictureFunctions();
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

    private static void DateTimeExtractorFunctions()
    {
        CallEquals(
            FerruleValue.FromInt64(1999),
            "year_from_datetime",
            Text("1999-12-31T19:20:00-05:00"));
        CallEquals(
            FerruleValue.FromInt64(2000),
            "year_from_datetime",
            Text("1999-12-31T24:00:00"));
        CallEquals(
            FerruleValue.FromInt64(1),
            "year_from_datetime",
            Text("-0001-12-31T24:00:00.0Z"));
        CallEquals(
            FerruleValue.FromInt64(long.MinValue),
            "year_from_datetime",
            Text("-9223372036854775808-01-01"));
        CallEquals(
            FerruleValue.FromInt64(3),
            "month_from_datetime",
            Text("2000-02-29T24:00:00"));
        CallEquals(
            FerruleValue.FromInt64(1),
            "month_from_datetime",
            Text("1999-12-31T24:00:00"));
        CallEquals(
            FerruleValue.FromInt64(1),
            "day_from_datetime",
            Text("2000-02-29T24:00:00"));
        CallEquals(
            FerruleValue.FromInt64(8),
            "day_from_datetime",
            Text("2019-07-08-05:00"));
        CallEquals(
            FerruleValue.FromInt64(0),
            "hours_from_datetime",
            Text("1999-12-31T24:00:00.000-05:00"));
        CallEquals(
            FerruleValue.FromInt64(59),
            "minutes_from_datetime",
            Text("-0004-02-29T23:59:59.5+14:00"));
        CallEquals(
            Text("09:30:02.5+05:00"),
            "time_from_datetime",
            Text("2001-12-17T09:30:02.5+05:00"));

        foreach (var function in new[]
        {
            "year_from_datetime",
            "month_from_datetime",
            "day_from_datetime",
            "hours_from_datetime",
            "minutes_from_datetime",
        })
        {
            CallEquals(FerruleValue.Null, function, FerruleValue.Null);
            AssertFunctionArity(function, 1, Array.Empty<FerruleValue>());
            AssertFunctionType(function, FerruleValue.FromInt64(1));
        }

        AssertFunctionType("time_from_datetime", FerruleValue.Null);
        AssertInvalidArgument(
            "year_from_datetime",
            "requires a year within the signed 64-bit integer range",
            Text("9223372036854775807-12-31T24:00:00"));
        AssertInvalidArgument(
            "month_from_datetime",
            "requires a valid ISO date or dateTime",
            Text("2001-02-29T00:00:00"));
        AssertInvalidArgument(
            "hours_from_datetime",
            "requires a valid ISO dateTime",
            Text("2024-01-01"));
        AssertInvalidArgument(
            "minutes_from_datetime",
            "requires a valid ISO dateTime",
            Text("2024-01-01T00:00:00+15:00"));
        AssertInvalidArgument(
            "time_from_datetime",
            "requires a value matching a supported date/time picture",
            Text("2001-02-29T09:30:02"));
    }

    private static void DateTimeCompositionFunctions()
    {
        CallEquals(
            Text("2024-02-29T09:08:07.125+05:30"),
            "datetime_from_date_and_time",
            Text("2024-02-29+05:30"),
            Text("09:08:07.125+05:30"));
        CallEquals(
            Text("2024-02-29T09:08:07-04:00"),
            "datetime_from_date_and_time",
            Text("2024-02-29"),
            Text("09:08:07-04:00"));
        CallEquals(
            Text("2024-01-02T00:00:00Z"),
            "datetime_from_date_and_time",
            Text("2024-01-02Z"));
        CallEquals(
            Text("-0001-01-02T00:00:00"),
            "datetime_from_date_and_time",
            Text("-0001-01-02"),
            FerruleValue.Null);
        AssertInvalidArgument(
            "datetime_from_date_and_time",
            "requires a value matching a supported date/time picture",
            Text("2024-02-29+05:30"),
            Text("09:08:07-04:00"));

        CallEquals(
            Text("2031-08-17T00:00:00"),
            "coerce_datetime",
            Text("2031-08-17"));
        CallEquals(
            Text("2031-08-17T00:00:00+05:45"),
            "coerce_datetime",
            Text("2031-08-17+05:45"));
        CallEquals(
            Text("2031-08-17T06:07:08.9Z"),
            "coerce_datetime",
            Text("2031-08-17T06:07:08.9Z"));
        CallEquals(FerruleValue.Null, "coerce_datetime", FerruleValue.Null);
        CallEquals(FerruleValue.XmlNil, "coerce_datetime", FerruleValue.XmlNil);

        CallEquals(
            Text("2024-02-29T09:08:07.1255+05:30"),
            "datetime_from_parts",
            Text("2024"),
            FerruleValue.FromInt64(2),
            FerruleValue.FromDouble(29.0),
            FerruleValue.FromInt64(9),
            FerruleValue.FromInt64(8),
            FerruleValue.FromInt64(7),
            FerruleValue.FromDouble(125.5),
            FerruleValue.FromInt64(330));
        CallEquals(
            Text("2024-01-02T00:00:00"),
            "datetime_from_parts",
            Text("2024"),
            Text("1"),
            Text("2"));
        CallEquals(
            Text("-0001-01-02T00:00:00"),
            "datetime_from_parts",
            Text("-1"),
            Text("1"),
            Text("2"),
            FerruleValue.Null,
            FerruleValue.Null,
            FerruleValue.Null,
            FerruleValue.FromDouble(double.Epsilon));
        CallEquals(
            Text("2024-01-02T00:00:00"),
            "datetime_from_parts",
            Text("2024"),
            Text("1"),
            Text("2"),
            FerruleValue.Null,
            FerruleValue.Null,
            FerruleValue.Null,
            FerruleValue.Null,
            FerruleValue.FromInt64(-32_768));

        AssertFunctionArity("datetime_from_date_and_time", 1, Array.Empty<FerruleValue>());
        AssertFunctionType("datetime_from_date_and_time", FerruleValue.FromInt64(2024));
        AssertFunctionArity("coerce_datetime", 1, Array.Empty<FerruleValue>());
        AssertFunctionType("coerce_datetime", FerruleValue.FromInt64(1));
        AssertFunctionArity(
            "datetime_from_parts",
            3,
            Text("2024"),
            Text("1"));
        AssertInvalidArgument(
            "datetime_from_parts",
            "requires a value matching a supported date/time picture",
            Text("2023"),
            Text("2"),
            Text("29"));
        AssertInvalidArgument(
            "datetime_from_parts",
            "requires a value matching a supported date/time picture",
            Text("2024"),
            Text("1"),
            Text("2"),
            FerruleValue.Null,
            FerruleValue.Null,
            FerruleValue.Null,
            FerruleValue.Null,
            FerruleValue.FromInt64(841));
    }

    private static void DateTimePictureFunctions()
    {
        CallEquals(
            Text("2014-12-09"),
            "parse_date",
            Text("09-12-2014"),
            Text("[D]-[M]-[Y]"));
        CallEquals(
            Text("2015-04-01"),
            "parse_date",
            Text("01 Apr 2015"),
            Text("[D01] [MNn,3-3] [Y]"));
        CallEquals(
            Text("2004-11-10+01:00"),
            "parse_date",
            Text("315 2004 +01:00"),
            Text("[d] [Y] [Z]"));

        CallEquals(
            Text("2014-09-12T13:56:24"),
            "parse_datetime",
            Text("09-12-2014 13:56:24"),
            Text("[M]-[D]-[Y] [H]:[m]:[s]"));
        CallEquals(
            Text("2010-12-01T15:02:39+01:00"),
            "parse_datetime",
            Text("1.December.10 03:2:39 p.m. +01:00"),
            Text("[D].[MNn].[Y,2-2] [h]:[m]:[s] [P] [Z]"));
        CallEquals(
            Text("2011-06-20T00:00:00"),
            "parse_datetime",
            Text("20110620"),
            Text("[Y,4-4][M,2-2][D,2-2]"));

        CallEquals(
            Text("15:02:39.25+01:00"),
            "parse_time",
            Text("03:2:39.25 p.m. GMT+01:00"),
            Text("[h]:[m]:[s].[f] [P] [z]"));
        CallEquals(
            Text("09:53:00"),
            "parse_time",
            FerruleValue.FromDouble(953.0),
            Text("[H,1-1][m,2-2]"));
        CallEquals(
            Text("17:03:00"),
            "parse_time",
            FerruleValue.FromInt64(1703),
            Text("[H,2-2][m,2-2]"));

        AssertFunctionArity("parse_date", 2, Text("2014-01-02"));
        AssertFunctionType(
            "parse_datetime",
            FerruleValue.FromInt64(2014),
            Text("[Y]"));
        AssertFunctionType(
            "parse_time",
            Text("09:30"),
            FerruleValue.FromInt64(1));
        AssertInvalidArgument(
            "parse_date",
            "requires a value matching a supported date/time picture",
            Text("2014-02-29"),
            Text("[Y]-[M]-[D]"));
        AssertInvalidArgument(
            "parse_datetime",
            "requires a value matching a supported date/time picture",
            Text("2014-01-02 24:00:00"),
            Text("[Y]-[M]-[D] [H]:[m]:[s]"));
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
