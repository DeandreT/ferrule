using System.Text;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void CsvScalarLexicals()
    {
        foreach (var (type, value, expected) in new (FerruleScalarType, FerruleValue, string)[]
        {
            (FerruleScalarType.String, Text("é🙂"), "é🙂"),
            (FerruleScalarType.String, Bool(true), "true"),
            (FerruleScalarType.String, FerruleValue.FromInt64(long.MinValue), "-9223372036854775808"),
            (FerruleScalarType.String, FerruleValue.FromDouble(1e20), "100000000000000000000"),
            (FerruleScalarType.Int64, FerruleValue.FromInt64(long.MaxValue), "9223372036854775807"),
            (FerruleScalarType.Int64, Text(" \u2003+007\u0085 "), "7"),
            (FerruleScalarType.Int64, Text("-9223372036854775808"), "-9223372036854775808"),
            (FerruleScalarType.Double, FerruleValue.FromDouble(-0.0), "-0"),
            (FerruleScalarType.Double, FerruleValue.FromDouble(1e-7), "0.0000001"),
            (FerruleScalarType.Double, FerruleValue.FromDouble(double.Epsilon), "0." + new string('0', 323) + "5"),
            (FerruleScalarType.Double, FerruleValue.FromDouble(double.MaxValue), "17976931348623157" + new string('0', 292)),
            (FerruleScalarType.Double, FerruleValue.FromInt64(9_007_199_254_740_992), "9007199254740992"),
            (FerruleScalarType.Double, FerruleValue.FromInt64(long.MinValue), "-9223372036854775808"),
            (FerruleScalarType.Double, Text(" \u2003+.5e+1\u0085 "), "5"),
            (FerruleScalarType.Double, Text("-0"), "-0"),
            (FerruleScalarType.Bool, Bool(false), "false"),
            (FerruleScalarType.Bool, Text(" 1 "), "true"),
            (FerruleScalarType.Bool, Text(" 0 "), "false"),
        })
        {
            CsvBytes(expected + "\n", CsvRows(Group(Field("Value", Scalar(value)))),
                [new("Value", type)], new() { HasHeaders = false });
        }
        var ordered = CsvRows(Group(
            Field("Boolean", Scalar(Text("true"))),
            Field("Number", Scalar(Text("12.5"))),
            Field("Integer", Scalar(Text("-17"))),
            Field("Text", Scalar(Text("first")))));
        CsvBytes("Text,Integer,Number,Boolean\nfirst,-17,12.5,true\n", ordered,
            [new("Text", FerruleScalarType.String), new("Integer", FerruleScalarType.Int64),
             new("Number", FerruleScalarType.Double), new("Boolean", FerruleScalarType.Bool)]);
        foreach (var type in Enum.GetValues<FerruleScalarType>())
        {
            CsvBytes(",\n", CsvRows(Group(Field("Left", Scalar(FerruleValue.Null)),
                    Field("Right", Scalar(FerruleValue.JsonNull)))),
                [new("Left", type), new("Right", type)], new() { HasHeaders = false });
        }
    }

    private static void CsvScalarRefusals()
    {
        foreach (var (type, value, got) in new (FerruleScalarType, FerruleValue, string)[]
        {
            (FerruleScalarType.String, FerruleValue.FromDouble(double.NaN), "non-finite float"),
            (FerruleScalarType.String, FerruleValue.FromDouble(double.PositiveInfinity), "non-finite float"),
            (FerruleScalarType.Double, FerruleValue.FromDouble(double.NegativeInfinity), "non-finite float"),
            (FerruleScalarType.Double, FerruleValue.FromInt64(9_007_199_254_740_993), "int outside the exact f64 range"),
            (FerruleScalarType.Double, FerruleValue.FromInt64(long.MaxValue), "int outside the exact f64 range"),
            (FerruleScalarType.Int64, FerruleValue.FromDouble(1.0), "float"),
            (FerruleScalarType.Int64, Bool(true), "bool"),
            (FerruleScalarType.Int64, Text("1.0"), "string"),
            (FerruleScalarType.Int64, Text("9223372036854775808"), "string"),
            (FerruleScalarType.Int64, Text("1\0"), "string"),
            (FerruleScalarType.Int64, Text("١"), "string"),
            (FerruleScalarType.Double, Text("1\0"), "string"),
            (FerruleScalarType.Double, Text("1e999"), "string"),
            (FerruleScalarType.Double, Text("NaN"), "string"),
            (FerruleScalarType.Double, Text("inf"), "string"),
            (FerruleScalarType.Double, Text("1,2"), "string"),
            (FerruleScalarType.Bool, Text("True"), "string"),
            (FerruleScalarType.Bool, Text("yes"), "string"),
        })
        {
            var error = CsvError(FerruleCsvError.ValueType, () => FerruleCsv.SerializeBytes(
                CsvRows(Group(Field("Value", Scalar(value)))), [new("Value", type)]));
            Equal<int?>(0, error.Row);
            Equal("Value", error.Field);
            Equal<FerruleScalarType?>(type, error.Expected);
            Equal(got, error.Got);
        }
        foreach (var type in Enum.GetValues<FerruleScalarType>())
        {
            var error = CsvError(FerruleCsvError.ValueType, () => FerruleCsv.SerializeBytes(
                CsvRows(Group(Field("Value", Scalar(FerruleValue.XmlNil)))), [new("Value", type)]));
            Equal("xml nil", error.Got);
            Equal<FerruleScalarType?>(type, error.Expected);
        }
    }

    private static void CsvQuotesAndUnicode()
    {
        CsvBytes("Name,Age\n\"café,\n🙂\"\"\",29\n", CsvRows(Group(
                Field("Age", Scalar(FerruleValue.FromInt64(29))),
                Field("Name", Scalar(Text("café,\n🙂\""))))),
            [new("Name", FerruleScalarType.String), new("Age", FerruleScalarType.Int64)]);
        CsvBytes("\uFEFF'N;ame'\n'O''Neil; Jr.\r\n🙂'\n",
            CsvRows(Group(Field("N;ame", Scalar(Text("O'Neil; Jr.\r\n🙂"))))),
            [new("N;ame", FerruleScalarType.String)],
            new() { Delimiter = ';', Quote = '\'', Utf8Bom = true });
        CsvBytes("A;B\n\"ordinary\";right\n", CsvRows(Group(
                Field("A", Scalar(Text("\"ordinary\""))), Field("B", Scalar(Text("right"))))),
            [new("A", FerruleScalarType.String), new("B", FerruleScalarType.String)],
            new() { Delimiter = ';', QuoteDisabled = true });
        // The scalar crosses the encoder's fixed byte chunk boundary intact.
        var crossing = new string('x', 4095) + "🙂";
        CsvBytes(crossing + "\n", CsvRows(Group(Field("Value", Scalar(Text(crossing))))),
            [new("Value", FerruleScalarType.String)], new() { HasHeaders = false });
        CsvTextBufferAndApiOwnership();
        CsvManyFormattedRows();
    }

    private static void CsvEmptyRecordsAndDialect()
    {
        CsvBytes("Value\n", CsvRows(), [new("Value", FerruleScalarType.String)]);
        CsvBytes("", CsvRows(), [new("Value", FerruleScalarType.String)], new() { HasHeaders = false });
        CsvBytes("\uFEFF", CsvRows(), [new("Value", FerruleScalarType.String)],
            new() { HasHeaders = false, Utf8Bom = true });
        CsvBytes("\"\"\n", CsvRows(), []);
        CsvBytes("\"\"\n", CsvRows(Group()), [], new() { HasHeaders = false });
        CsvBytes("\"\"\n", CsvRows(Group()), [], new() { HasHeaders = false, QuoteDisabled = true });
        CsvBytes("\"\"\n", CsvRows(), [new("", FerruleScalarType.String)], new() { QuoteDisabled = true });
        CsvBytes("\"\"\n", CsvRows(Group(Field("Value", Scalar(Text(""))))),
            [new("Value", FerruleScalarType.String)], new() { HasHeaders = false });
        var emptyRow = CsvError(FerruleCsvError.UnquotedSingleEmptyRow, () => FerruleCsv.SerializeBytes(
            CsvRows(Group(Field("Value", Scalar(FerruleValue.Null)))),
            [new("Value", FerruleScalarType.String)], new() { QuoteDisabled = true }));
        Equal<int?>(0, emptyRow.Row);
        foreach (var delimiter in new[] { '\0', '\r', '\n', 'é' })
        {
            var error = CsvError(FerruleCsvError.BadDelimiter, () => FerruleCsv.SerializeBytes(
                CsvRows(), [], new() { Delimiter = delimiter }));
            Equal<char?>(delimiter, error.Character);
        }
        foreach (var quote in new[] { '\t', ' ', 'é' })
        {
            var error = CsvError(FerruleCsvError.BadQuote, () => FerruleCsv.SerializeBytes(
                CsvRows(), [], new() { Quote = quote }));
            Equal<char?>(quote, error.Character);
        }
        CsvError(FerruleCsvError.DelimiterQuoteConflict, () => FerruleCsv.SerializeBytes(
            CsvRows(), [], new() { Delimiter = '|', Quote = '|' }));
        CsvError(FerruleCsvError.ConflictingQuoteSettings, () => FerruleCsv.SerializeBytes(
            CsvRows(), [], new() { QuoteDisabled = true, Quote = '"' }));
        foreach (var boundary in new[] { "a,b", "a\r", "a\n" })
        {
            var header = CsvError(FerruleCsvError.UnquotedHeaderBoundary, () => FerruleCsv.SerializeBytes(
                CsvRows(), [new(boundary, FerruleScalarType.String)], new() { QuoteDisabled = true }));
            Equal(boundary, header.Field);
            var field = CsvError(FerruleCsvError.UnquotedFieldBoundary, () => FerruleCsv.SerializeBytes(
                CsvRows(Group(Field("Value", Scalar(Text(boundary))))),
                [new("Value", FerruleScalarType.String)], new() { QuoteDisabled = true }));
            Equal<int?>(0, field.Row);
            Equal("Value", field.Field);
        }
    }

    private static void CsvShapeAndFailureOrder()
    {
        FerruleCsvField[] fields = [new("Value", FerruleScalarType.String)];
        foreach (var (primary, got) in new (FerruleInstance, string)[]
        {
            (Scalar(Text("wrong")), "string"), (Group(), "group"),
            (new FerruleMappedSequence([]), "mapped sequence"),
            (new FerruleDocumentSet([]), "document set"),
        })
        {
            Equal(got, CsvError(FerruleCsvError.RootShape,
                () => FerruleCsv.SerializeBytes(primary, fields)).Got);
        }
        var nonGroup = CsvError(FerruleCsvError.RowShape,
            () => FerruleCsv.SerializeBytes(CsvRows(Group(Field("Value", Scalar(Text("valid")))),
                Scalar(Text("wrong"))), fields));
        Equal<int?>(1, nonGroup.Row);
        Equal("string", nonGroup.Got);
        var missing = CsvError(FerruleCsvError.MissingField,
            () => FerruleCsv.SerializeBytes(CsvRows(Group()), fields));
        Equal<int?>(0, missing.Row);
        Equal("Value", missing.Field);
        var extra = CsvError(FerruleCsvError.UnexpectedField,
            () => FerruleCsv.SerializeBytes(CsvRows(Group(Field("Other", Scalar(Text("extra"))))), fields));
        Equal("Other", extra.Field);
        var nested = CsvError(FerruleCsvError.ValueType,
            () => FerruleCsv.SerializeBytes(CsvRows(Group(Field("Value", Group()))), fields));
        Equal("group", nested.Got);
        const string duplicateSchema = """
            {"name":"Root","kind":{"kind":"group","children":[
              {"name":"Value","kind":{"kind":"scalar","ty":"string"}},
              {"name":"Value","kind":{"kind":"scalar","ty":"string"}}
            ]}}
            """;
        var duplicate = FerruleJson.Parse(duplicateSchema, """{"Value":"duplicate"}""");
        Equal(2, ((FerruleGroup)duplicate).Fields.Count);
        var duplicateError = CsvError(FerruleCsvError.DuplicateField,
            () => FerruleCsv.SerializeBytes(CsvRows(duplicate), fields));
        Equal<int?>(0, duplicateError.Row);
        Equal("Value", duplicateError.Field);
        foreach (var invalid in new FerruleCsvField[][]
        {
            [new("Value", FerruleScalarType.String), new("Value", FerruleScalarType.Int64)],
            [new("Value", (FerruleScalarType)99)], [new("bad\ud800", FerruleScalarType.String)],
            [new(null!, FerruleScalarType.String)], [null!],
        })
        {
            CsvError(FerruleCsvError.UnsupportedSchema, () => FerruleCsv.SerializeBytes(CsvRows(), invalid));
        }
        // A later invalid row wins over an earlier row or header needing quotes.
        var later = CsvError(FerruleCsvError.MissingField, () => FerruleCsv.SerializeBytes(
            CsvRows(Group(Field("bad,name", Scalar(Text("first,row")))), Group()),
            [new("bad,name", FerruleScalarType.String)], new() { QuoteDisabled = true }));
        Equal<int?>(1, later.Row);
        Equal("bad,name", later.Field);
        CsvTextFailurePayloads();
        CsvDeferredQuoteFailureOrder();
    }

    private static void CsvActualUtf8Limit()
    {
        FerruleCsvField[] fields = [new("x", FerruleScalarType.String)];
        var headerless = new FerruleCsvWriteOptions { HasHeaders = false };
        var maximum = FerruleCsv.MaximumOutputBytes;
        var exact = FerruleCsv.SerializeBytes(
            CsvRows(Group(Field("x", Scalar(Text(new string('x', maximum - 1)))))), fields, headerless);
        Equal(maximum, exact.Length);
        Equal(true, exact.AsSpan(0, maximum - 1).IndexOfAnyExcept((byte)'x') < 0);
        Equal((byte)'\n', exact[^1]);
        var tooLarge = CsvError(FerruleCsvError.OutputTooLarge, () => FerruleCsv.SerializeBytes(
            CsvRows(Group(Field("x", Scalar(Text(new string('x', maximum)))))), fields, headerless));
        Equal<int?>(maximum, tooLarge.Maximum);
        Equal<int?>(null, tooLarge.Row);
        Equal<string?>(null, tooLarge.Got);
        // BOM (3), header x+LF (2), enclosing quotes (2), LF (1),
        // and every doubled quote are included in the actual sink budget.
        var quoted = new string('"', (maximum - 8) / 2);
        var bom = new FerruleCsvWriteOptions { Utf8Bom = true };
        var expanded = FerruleCsv.SerializeBytes(CsvRows(Group(Field("x", Scalar(Text(quoted))))), fields, bom);
        Equal(maximum, expanded.Length);
        Equal(true, expanded.AsSpan(0, 5).SequenceEqual(new byte[] { 0xef, 0xbb, 0xbf, (byte)'x', (byte)'\n' }));
        Equal(true, expanded.AsSpan(5, maximum - 6).IndexOfAnyExcept((byte)'"') < 0);
        Equal((byte)'\n', expanded[^1]);
        Equal<int?>(maximum, CsvError(FerruleCsvError.OutputTooLarge, () => FerruleCsv.SerializeBytes(
            CsvRows(Group(Field("x", Scalar(Text(quoted + "a"))))), fields, bom)).Maximum);
        // Complete row validation still precedes resource failure of a huge first row.
        var later = CsvError(FerruleCsvError.RowShape, () => FerruleCsv.SerializeBytes(
            CsvRows(Group(Field("x", Scalar(Text(new string('x', maximum))))), Scalar(Text("invalid"))),
            fields, headerless));
        Equal<int?>(1, later.Row);
        CsvTextOutputLimit();
    }

    private static void CsvManyFormattedRows()
    {
        FerruleCsvField[] fields = [new("Value", FerruleScalarType.String),
            new("Count", FerruleScalarType.Int64), new("Enabled", FerruleScalarType.Bool)];
        var rows = new FerruleInstance[4096];
        var expected = new StringBuilder("\uFEFFValue;Count;Enabled\n");
        for (var row = 0; row < rows.Length; row++)
        {
            var quoted = row % 2 == 0;
            rows[row] = Group(Field("Enabled", Scalar(Text(quoted ? " 1 " : " 0 "))),
                Field("Count", Scalar(Text(" +0007 "))),
                Field("Value", Scalar(Text(quoted ? "é;🙂'" : "plain"))));
            expected.Append(quoted ? "'é;🙂''';7;true\n" : "plain;7;false\n");
        }
        CsvBytes(expected.ToString(), CsvRows(rows), fields,
            new() { Delimiter = ';', Quote = '\'', Utf8Bom = true });
        CsvBytes(string.Concat(Enumerable.Repeat("7\n", 4096)),
            CsvRows(Enumerable.Range(0, 4096).Select(_ =>
                Group(Field("Count", Scalar(Text(" +0007 "))))).ToArray()),
            [new("Count", FerruleScalarType.Int64)],
            new() { HasHeaders = false, QuoteDisabled = true });
    }

    private static void CsvDeferredQuoteFailureOrder()
    {
        FerruleCsvField[] fields = [new("Value", FerruleScalarType.String)];
        var unquoted = new FerruleCsvWriteOptions { HasHeaders = false, QuoteDisabled = true };
        var emptyFirst = CsvRows(Group(Field("Value", Scalar(Text("")))),
            Group(Field("Value", Scalar(Text("needs,quotes")))));
        CsvMatchingFailure(FerruleCsvError.UnquotedSingleEmptyRow, emptyFirst, fields, unquoted);
        Equal<int?>(0, CsvError(FerruleCsvError.UnquotedSingleEmptyRow,
            () => FerruleCsv.SerializeBytes(emptyFirst, fields, unquoted)).Row);
        var quotedFirst = CsvRows(Group(Field("Value", Scalar(Text("needs,quotes")))),
            Group(Field("Value", Scalar(Text("")))));
        CsvMatchingFailure(FerruleCsvError.UnquotedFieldBoundary, quotedFirst, fields, unquoted);
        var first = CsvError(FerruleCsvError.UnquotedFieldBoundary,
            () => FerruleCsv.SerializeBytes(quotedFirst, fields, unquoted));
        Equal<int?>(0, first.Row);
        Equal("Value", first.Field);
        // Deferred early refusals must not hide any later row or scalar failure.
        foreach (var earlier in new[] { "", "needs,quotes" })
        {
            var laterShape = CsvRows(Group(Field("Value", Scalar(Text(earlier)))), Group());
            CsvMatchingFailure(FerruleCsvError.MissingField, laterShape, fields, unquoted);
            Equal<int?>(1, CsvError(FerruleCsvError.MissingField,
                () => FerruleCsv.SerializeBytes(laterShape, fields, unquoted)).Row);
            var laterType = CsvRows(Group(Field("Value", Scalar(Text(earlier)))),
                Group(Field("Value", Scalar(FerruleValue.XmlNil))));
            CsvMatchingFailure(FerruleCsvError.ValueType, laterType, fields, unquoted);
            var typed = CsvError(FerruleCsvError.ValueType,
                () => FerruleCsv.SerializeBytes(laterType, fields, unquoted));
            Equal<int?>(1, typed.Row);
            Equal("xml nil", typed.Got);
        }
        var header = CsvRows(Group(Field("bad,name", Scalar(Text("")))),
            Group(Field("bad,name", Scalar(Text("needs,quotes")))));
        CsvMatchingFailure(FerruleCsvError.UnquotedHeaderBoundary, header,
            [new("bad,name", FerruleScalarType.String)], new() { QuoteDisabled = true });
    }

    private static void CsvTextBufferAndApiOwnership()
    {
        FerruleCsvField[] fields = [new("Value", FerruleScalarType.String)];
        // Short documents leave unused MemoryStream capacity. Decode only the
        // written bytes, including the empty and BOM-only cases.
        CsvBytes("", CsvRows(), fields, new() { HasHeaders = false });
        CsvBytes("\uFEFF", CsvRows(), fields, new() { HasHeaders = false, Utf8Bom = true });
        CsvBytes("Value\n", CsvRows(), fields);
        CsvBytes("\"\"\n", CsvRows(Group()), [], new() { HasHeaders = false });
        var primary = CsvRows(Group(Field("Value", Scalar(Text("short")))));
        var bytes = FerruleCsv.SerializeBytes(primary, fields);
        var text = FerruleCsv.Serialize(primary, fields);
        Equal("Value\nshort\n", text);
        Equal(false, text.Contains('\0'));
        // Returned bytes stay caller-owned; they do not expose shared storage.
        bytes[0] = (byte)'!';
        Equal("Value\nshort\n", text);
        Equal("Value\nshort\n", FerruleCsv.Serialize(primary, fields));
        Equal((byte)'V', FerruleCsv.SerializeBytes(primary, fields)[0]);
        CsvBytes("Value\nembedded\0zero\n",
            CsvRows(Group(Field("Value", Scalar(Text("embedded\0zero"))))), fields);

        var crossing = new string('x', 4095) + "🙂';tail";
        CsvBytes("\uFEFF'V;alue'\n'" + new string('x', 4095) + "🙂'';tail'\n",
            CsvRows(Group(Field("V;alue", Scalar(Text(crossing))))),
            [new("V;alue", FerruleScalarType.String)],
            new() { Delimiter = ';', Quote = '\'', Utf8Bom = true });
        CsvBytes("Value\n'🙂\r'\n", CsvRows(Group(Field("Value", Scalar(Text("🙂\r"))))),
            fields, new() { QuoteDisabled = false, Quote = '\'' });
    }

    private static void CsvTextFailurePayloads()
    {
        FerruleCsvField[] fields = [new("Value", FerruleScalarType.String)];
        CsvMatchingFailure(FerruleCsvError.RootShape, Scalar(Text("wrong")),
            [new("duplicate", FerruleScalarType.String), new("duplicate", FerruleScalarType.Int64)],
            new() { Delimiter = '\0' });
        CsvMatchingFailure(FerruleCsvError.UnsupportedSchema, CsvRows(),
            [new("duplicate", FerruleScalarType.String), new("duplicate", FerruleScalarType.Int64)],
            new() { Delimiter = '\0' });
        CsvMatchingFailure(FerruleCsvError.BadDelimiter, CsvRows(Scalar(Text("wrong"))),
            fields, new() { Delimiter = '\0' });
        CsvMatchingFailure(FerruleCsvError.ConflictingQuoteSettings, CsvRows(),
            fields, new() { QuoteDisabled = true, Quote = '"' });
        CsvMatchingFailure(FerruleCsvError.BadQuote, CsvRows(), fields, new() { Quote = 'é' });
        CsvMatchingFailure(FerruleCsvError.DelimiterQuoteConflict, CsvRows(),
            fields, new() { Delimiter = '|', Quote = '|' });
        CsvMatchingFailure(FerruleCsvError.MissingField,
            CsvRows(Group(Field("bad,name", Scalar(Text("first,row")))), Group()),
            [new("bad,name", FerruleScalarType.String)], new() { QuoteDisabled = true });
        CsvMatchingFailure(FerruleCsvError.ValueType,
            CsvRows(Group(Field("Value", Scalar(Text("first,row")))),
                Group(Field("Value", Scalar(FerruleValue.XmlNil)))),
            fields, new() { QuoteDisabled = true });
        CsvMatchingFailure(FerruleCsvError.UnquotedHeaderBoundary, CsvRows(),
            [new("bad,name", FerruleScalarType.String)], new() { QuoteDisabled = true });
        CsvMatchingFailure(FerruleCsvError.UnquotedFieldBoundary,
            CsvRows(Group(Field("Value", Scalar(Text("first,row"))))),
            fields, new() { QuoteDisabled = true, HasHeaders = false });
        CsvMatchingFailure(FerruleCsvError.UnquotedSingleEmptyRow,
            CsvRows(Group(Field("Value", Scalar(FerruleValue.Null)))),
            fields, new() { QuoteDisabled = true, HasHeaders = false });
    }

    private static void CsvTextOutputLimit()
    {
        FerruleCsvField[] fields = [new("x", FerruleScalarType.String)];
        var maximum = FerruleCsv.MaximumOutputBytes;
        var headerless = new FerruleCsvWriteOptions { HasHeaders = false };
        var exact = FerruleCsv.Serialize(
            CsvRows(Group(Field("x", Scalar(Text(new string('x', maximum - 1)))))), fields, headerless);
        Equal(maximum, exact.Length);
        Equal(true, exact.AsSpan(0, maximum - 1).IndexOfAnyExcept('x') < 0);
        Equal('\n', exact[^1]);
        CsvMatchingFailure(FerruleCsvError.OutputTooLarge,
            CsvRows(Group(Field("x", Scalar(Text(new string('x', maximum)))))), fields, headerless);

        // Compare text to the existing exact byte-budget case without creating
        // another document-sized UTF-8 conversion array. A decoded BOM is one
        // UTF-16 code unit but occupies three bytes in the logical byte budget.
        var quotes = new string('"', (maximum - 8) / 2);
        var bom = new FerruleCsvWriteOptions { Utf8Bom = true };
        var expanded = FerruleCsv.Serialize(CsvRows(Group(Field("x", Scalar(Text(quotes))))), fields, bom);
        Equal(maximum - 2, expanded.Length);
        Equal(true, expanded.AsSpan(0, 3).SequenceEqual("\uFEFFx\n".AsSpan()));
        Equal(true, expanded.AsSpan(3, expanded.Length - 4).IndexOfAnyExcept('"') < 0);
        Equal('\n', expanded[^1]);
        CsvMatchingFailure(FerruleCsvError.OutputTooLarge,
            CsvRows(Group(Field("x", Scalar(Text(quotes + "a"))))), fields, bom);
        CsvMatchingFailure(FerruleCsvError.RowShape,
            CsvRows(Group(Field("x", Scalar(Text(new string('x', maximum))))), Scalar(Text("invalid"))),
            fields, headerless);
        CsvMatchingFailure(FerruleCsvError.UnquotedFieldBoundary,
            CsvRows(Group(Field("x", Scalar(Text(new string(',', maximum)))))),
            fields, new() { HasHeaders = false, QuoteDisabled = true });
    }

    private static void CsvMatchingFailure(
        FerruleCsvError expected,
        FerruleInstance primary,
        IReadOnlyList<FerruleCsvField> fields,
        FerruleCsvWriteOptions? options = null)
    {
        var bytes = CsvError(expected, () => FerruleCsv.SerializeBytes(primary, fields, options));
        var text = CsvError(expected, () => FerruleCsv.Serialize(primary, fields, options));
        Equal(bytes.Message, text.Message);
        Equal(bytes.Row, text.Row);
        Equal(bytes.Field, text.Field);
        Equal(bytes.Expected, text.Expected);
        Equal(bytes.Got, text.Got);
        Equal(bytes.Character, text.Character);
        Equal(bytes.Maximum, text.Maximum);
        Equal<Exception?>(null, text.InnerException);
    }

    private static FerruleRepeated CsvRows(params FerruleInstance[] rows) => new(rows);

    private static void CsvBytes(
        string expected,
        FerruleInstance primary,
        IReadOnlyList<FerruleCsvField> fields,
        FerruleCsvWriteOptions? options = null)
    {
        var bytes = FerruleCsv.SerializeBytes(primary, fields, options);
        Equal(true, bytes.AsSpan().SequenceEqual(Encoding.UTF8.GetBytes(expected)));
        Equal(expected, FerruleCsv.Serialize(primary, fields, options));
    }

    private static FerruleCsvException CsvError(FerruleCsvError expected, Action action)
    {
        try
        {
            action();
        }
        catch (FerruleCsvException exception)
        {
            Equal(expected, exception.Error);
            return exception;
        }
        throw new InvalidOperationException($"Expected CSV error {expected}.");
    }
}
