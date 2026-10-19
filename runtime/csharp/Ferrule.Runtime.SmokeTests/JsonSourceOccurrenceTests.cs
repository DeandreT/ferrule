using System.Text;
using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static FerruleJsonSourceDocument SourceDocument(string text) =>
        FerruleJsonSourceDocument.Parse("same.json", Encoding.UTF8.GetBytes(text));

    private static void JsonSourceOccurrenceDocument()
    {
        using var fixture = JsonDocument.Parse(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "json-source-occurrences.json")));
        var inputs = fixture.RootElement.GetProperty("documents").EnumerateArray().ToArray();
        var calls = new List<string>();
        var loaded = FerruleJsonSources.Load(inputs.Select(input => input.GetProperty("request").GetString()!), request =>
        {
            calls.Add(request);
            var input = inputs.Single(input => input.GetProperty("request").GetString() == request);
            return FerruleJsonSourceDocument.Parse(input.GetProperty("identity").GetString()!,
                Encoding.UTF8.GetBytes(input.GetProperty("input").GetString()!));
        }, 3);
        Equal(0, calls.Count);
        var documents = new List<FerruleInstance>();
        var retained = new List<FerruleJsonSourceOccurrence>();
        foreach (var root in loaded)
        {
            var rows = new List<FerruleInstance>();
            foreach (var array in root.MembersNamed("rows"))
            foreach (var row in array.Items())
            foreach (var candidate in row.MembersNamed("value"))
            {
                var value = candidate.OfKind(FerruleJsonSourceKind.String);
                if (value is null) continue;
                var owner = value.Parent!;
                Equal(true, owner.SameOccurrence(row));
                var name = owner.MembersNamed("owner").Single();
                rows.Add(new FerruleGroup([
                    new("owner", new FerruleScalar(FerruleValue.FromString(name.Value.StringValue!))),
                    new("value", new FerruleScalar(FerruleValue.FromString(value.Value.StringValue!))),
                ]));
                retained.Add(value);
            }
            documents.Add(new FerruleGroup([
                new("identity", new FerruleScalar(FerruleValue.FromString(root.Document.Identity))),
                new("rows", new FerruleRepeated(rows)),
            ]));
        }
        var output = new FerruleGroup([new("documents", new FerruleRepeated(documents))]);
        Equal(fixture.RootElement.GetProperty("expected").GetString(),
            FerruleJson.SerializeEmbeddedCompact(fixture.RootElement.GetProperty("schema").GetRawText(), output));
        Equal("z-first,a-second,last", string.Join(",", calls));
        Equal("a", retained[0].Value.StringValue);
        Equal(false, retained[0].SameOccurrence(retained[4]));
        Equal(retained[0].Document.Identity, retained[4].Document.Identity);
    }

    private static void JsonSourceOccurrenceIdentity()
    {
        var parsed = SourceDocument("""{"x":"same","x":"same","a":["same","same"]}""");
        var first = FerruleJsonSourceOccurrence.Create(parsed);
        var second = FerruleJsonSourceOccurrence.Create(parsed);
        Equal(false, first.SameOccurrence(second));
        Equal((FerruleJsonSourceOccurrence?)null, first.Parent);
        var values = first.MembersNamed("x").ToArray();
        Equal(false, values[0].SameOccurrence(values[1]));
        Equal(true, values[0].SameOccurrence(first.MembersNamed("x").First()));
        Equal(true, values[0].SameOccurrence(values[0].OfKind(FerruleJsonSourceKind.String)));
        Equal((FerruleJsonSourceOccurrence?)null, values[0].OfKind(FerruleJsonSourceKind.Number));
        Equal(true, values[0].Parent!.SameOccurrence(first));
        Equal(0, values[0].Items().Count());
        Equal(0, first.MembersNamed("X").Count());
        var array = first.MembersNamed("a").Single();
        var items = array.Items().ToArray();
        Equal(false, items[0].SameOccurrence(items[1]));
        Equal(true, items[1].Parent!.SameOccurrence(array));
        Equal(0, array.MembersNamed("x").Count());
        var loaded = FerruleJsonSources.Load(["same", "same"], _ => parsed, 2).ToArray();
        Equal(false, loaded[0].SameOccurrence(loaded[1]));
    }

    private static void JsonSourceReachedFailures()
    {
        foreach (var failure in new[] { "syntax", "validation", "cleanup", "enumeration" })
        {
            var events = new List<string>();
            var original = new InvalidOperationException(failure);
            IEnumerable<string> Requests()
            {
                try
                {
                    foreach (var request in new[] { "good", failure, "unreached" })
                    {
                        events.Add($"enumerate:{request}");
                        if (request == "enumeration") throw original;
                        yield return request;
                    }
                }
                finally { events.Add("dispose"); }
            }
            var loaded = FerruleJsonSources.Load(Requests(), request =>
            {
                events.Add($"load:{request}");
                var parsed = SourceDocument(request == "syntax" ? "{" : "null");
                if (request != "good") throw original;
                events.Add("ready:good");
                return parsed;
            }, 3);
            Equal(0, events.Count);
            using var iterator = loaded.GetEnumerator();
            Equal(0, events.Count);
            Equal(true, iterator.MoveNext());
            Equal("enumerate:good,load:good,ready:good", string.Join(",", events));
            Exception? observed = null;
            try { iterator.MoveNext(); }
            catch (Exception error) { observed = error; }
            if (failure == "syntax") Equal(FerruleJsonSourceErrorKind.Syntax, ((FerruleJsonSourceException)observed!).Kind);
            else Equal(true, ReferenceEquals(original, observed));
            var expected = $"enumerate:good,load:good,ready:good,enumerate:{failure}";
            if (failure != "enumeration") expected += $",load:{failure}";
            expected += ",dispose";
            Equal(expected, string.Join(",", events));
            Equal(false, iterator.MoveNext());
            Equal(false, iterator.MoveNext());
            Equal(expected, string.Join(",", events));
        }
    }

    private static void JsonSourceLoadLimits()
    {
        var calls = 0;
        FerruleJsonSourceDocument Load(string _) { calls++; return SourceDocument("null"); }
        using var limited = FerruleJsonSources.Load(["a", "b"], Load, 1).GetEnumerator();
        Equal(true, limited.MoveNext());
        Throws<FerruleJsonSourceLoadException>(() => limited.MoveNext());
        Equal(false, limited.MoveNext());
        Equal(1, calls);
        Throws<FerruleJsonSourceLoadException>(() => FerruleJsonSources.Load(["a"], Load, 0).ToArray());
        Equal(0, FerruleJsonSources.Load([], Load, 0).Count());
        Throws<ArgumentOutOfRangeException>(() => FerruleJsonSources.Load(["a"], Load, FerruleJsonSources.MaxDocuments + 1));
        Throws<ArgumentOutOfRangeException>(() => FerruleJsonSources.Load(["a"], Load, -1));
        Equal(1, calls);
        var disposed = false;
        IEnumerable<string> Requests()
        {
            try { yield return "a"; throw new Exception("unreached"); }
            finally { disposed = true; }
        }
        FerruleJsonSourceOccurrence retained;
        using (var early = FerruleJsonSources.Load(Requests(), Load, 2).GetEnumerator())
        {
            Equal(true, early.MoveNext());
            retained = early.Current;
        }
        Equal(true, disposed);
        Equal(2, calls);
        Equal("same.json", retained.Document.Identity);
        Equal(2, FerruleJsonSources.Load(["a", "b"], Load, 2).Count());
        Equal(4, calls);
    }
}
