using System.Text;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static FerruleCsvSourceDocument CsvDocument(string path, string text) =>
        FerruleCsvSourceDocument.ParseBytes(path, "C:/resolved/same.csv", Encoding.UTF8.GetBytes(text),
            [new("Value", FerruleScalarType.String)], FerruleCsvByteDecodingPolicy.StrictUtf8);

    private static string CsvRowText(FerruleCsvRowOccurrence row)
    {
        var group = (FerruleGroup)row.Value;
        Equal(1, group.Fields.Count);
        Equal("Value", group.Fields[0].Name);
        var value = ((FerruleScalar)group.Fields[0].Value).Value;
        Equal(FerruleValueKind.String, value.Kind);
        return value.StringValue;
    }

    private static void CsvSourceOccurrences()
    {
        var first = CsvDocument(" ./same.csv ", "Value\nA\nA\nB\n");
        var second = CsvDocument(" ./same.csv ", "Value\nA\nA\nB\n");
        Equal(true, first.SameDocument(first));
        Equal(false, first.SameDocument(second));
        Equal(" ./same.csv ", first.Path);
        Equal("C:/resolved/same.csv", first.SourcePath);
        var rows = first.Rows().ToArray();
        Equal(3, rows.Length);
        for (var i = 0; i < rows.Length; i++)
        {
            Equal(i + 1, rows[i].Position);
            Equal(i == 2 ? "B" : "A", CsvRowText(rows[i]));
            Equal(true, rows[i].Document.SameDocument(first));
        }
        Equal(true, rows[0].SameOccurrence(first.Rows().First()));
        Equal(false, rows[0].SameOccurrence(rows[1]));
        Equal(false, rows[0].SameOccurrence(second.Rows().First()));

        var requested = new List<string>();
        var loaded = new List<string>();
        IEnumerable<string> Requests()
        {
            foreach (var name in new[] { "first", "empty", "second" })
            {
                requested.Add(name);
                yield return name;
            }
        }
        using var stream = new FerruleCsvSourceRows(Requests(), name =>
        {
            loaded.Add(name);
            return CsvDocument(name, name switch
            {
                "first" => "Value\nA\nB\n",
                "empty" => "Value\n",
                "second" => "Value\nA\n",
                _ => throw new Exception("Unexpected request."),
            });
        }, 3);
        Equal(0, requested.Count);
        Equal(0, loaded.Count);
        Throws<InvalidOperationException>(() => { _ = stream.Current; });
        Equal(true, stream.MoveNext());
        Equal(1, requested.Count);
        var retained = new List<FerruleCsvRowOccurrence> { stream.Current };
        while (stream.MoveNext()) retained.Add(stream.Current);
        Equal(false, stream.MoveNext());
        Throws<InvalidOperationException>(() => { _ = stream.Current; });
        Equal("first,empty,second", string.Join(',', requested));
        Equal("first,empty,second", string.Join(',', loaded));
        Equal(3, retained.Count);
        var group = retained.Where(row => CsvRowText(row) == "A").ToArray();
        Equal(2, group.Length);
        Equal("first", group[0].Document.Path);
        Equal("second", group[1].Document.Path);
        Equal(1, group[0].Position);
        Equal(1, group[1].Position);
        Equal(false, group[0].Document.SameDocument(group[1].Document));
        Equal(2, retained[1].Position);
        Throws<NotSupportedException>(() => stream.Reset());
    }

    private static void CsvSourceFailures()
    {
        for (var failed = 0; failed < 3; failed++)
        {
            var calls = new List<string>();
            var expected = new InvalidOperationException("loader failure");
            using var stream = new FerruleCsvSourceRows(["0", "1", "2"], name =>
            {
                calls.Add(name);
                if (name == failed.ToString()) throw expected;
                return CsvDocument(name, "Value\nA\n");
            }, 3);
            for (var index = 0; index < failed; index++) Equal(true, stream.MoveNext());
            try { stream.MoveNext(); throw new Exception("Expected load failure."); }
            catch (InvalidOperationException actual) { Equal(true, ReferenceEquals(expected, actual)); }
            Equal(false, stream.MoveNext());
            Equal(failed + 1, calls.Count);
        }
        IEnumerable<string> BadRequests()
        {
            yield return "first";
            throw new InvalidOperationException("enumeration failure");
        }
        var count = 0;
        using (var stream = new FerruleCsvSourceRows(BadRequests(), name =>
        {
            count++;
            return CsvDocument(name, "Value\nA\n");
        }, 3))
        {
            Equal(true, stream.MoveNext());
            Throws<InvalidOperationException>(() => stream.MoveNext());
            Equal(false, stream.MoveNext());
            Equal(1, count);
        }
        foreach (var maximum in new[] { 0, 1 })
        {
            var calls = 0;
            using var stream = new FerruleCsvSourceRows(["a", "b"], name =>
            {
                calls++;
                return CsvDocument(name, "Value\n");
            }, maximum);
            try { stream.MoveNext(); throw new Exception("Expected count refusal."); }
            catch (FerruleCsvSourceLimitException error) { Equal(maximum, error.Maximum); }
            Equal(false, stream.MoveNext());
            Equal(maximum, calls);
        }
        Throws<ArgumentOutOfRangeException>(() => new FerruleCsvSourceRows([], name => CsvDocument(name, ""),
            FerruleCsvSourceRows.MaximumDocuments + 1));
        Throws<ArgumentException>(() => CsvDocument("", "Value\n"));
        Throws<ArgumentException>(() => CsvDocument("x\0y", "Value\n"));
        Throws<ArgumentException>(() => CsvDocument("\ud800", "Value\n"));
        Throws<ArgumentException>(() => CsvDocument(new string('x', FerruleCsvSourceDocument.MaximumPathBytes + 1), "Value\n"));
        CsvDocument(new string('x', FerruleCsvSourceDocument.MaximumPathBytes), "Value\n");
        var fields = new[] { new FerruleCsvReadField("Value", FerruleScalarType.String) };
        Throws<ArgumentException>(() => FerruleCsvSourceDocument.ParseBytes("ok", "", [], fields,
            FerruleCsvByteDecodingPolicy.StrictUtf8));
        var malformed = new byte[] { 86, 97, 108, 117, 101, 10, 255, 10 };
        Throws<FerruleCsvInputException>(() => FerruleCsvSourceDocument.ParseBytes("ok", null, malformed,
            fields, FerruleCsvByteDecodingPolicy.StrictUtf8));
        var decoded = FerruleCsvSourceDocument.ParseBytes("ok", null, malformed, fields,
            FerruleCsvByteDecodingPolicy.BomAwareNonFlushingReplacement);
        Equal("ok", decoded.SourcePath);
        Equal("\ufffd", CsvRowText(decoded.Rows().Single()));
        Throws<FerruleCsvInputException>(() => FerruleCsvSourceDocument.ParseBytes("bad", null,
            Encoding.UTF8.GetBytes("Value\n1\ninvalid\n2\n"), [new("Value", FerruleScalarType.Int64)],
            FerruleCsvByteDecodingPolicy.StrictUtf8));
    }
}
