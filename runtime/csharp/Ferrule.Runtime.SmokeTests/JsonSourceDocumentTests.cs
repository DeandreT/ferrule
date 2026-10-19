using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static object JsonSourceView(FerruleJsonSourceValue value) => value.Kind switch
    {
        FerruleJsonSourceKind.Null => new object[] { "null" },
        FerruleJsonSourceKind.Boolean => new object[] { "boolean", value.BooleanValue!.Value },
        FerruleJsonSourceKind.Number => new object[] { "number", value.NumberText! },
        FerruleJsonSourceKind.String => new object[] { "string", value.StringValue! },
        FerruleJsonSourceKind.Array => new object[] { "array", value.Items.Select(JsonSourceView).ToArray() },
        FerruleJsonSourceKind.Object => new object[] { "object", value.Members.Select(member => new object[] { member.Key, JsonSourceView(member.Value) }).ToArray() },
        _ => throw new Exception("Unexpected kind."),
    };

    private static void JsonSourceCorpus()
    {
        using var corpus = JsonDocument.Parse(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "json-source-cases.json")));
        foreach (var item in corpus.RootElement.GetProperty("valid").EnumerateArray())
        {
            var input = Encoding.UTF8.GetBytes(item.GetProperty("input").GetString()!);
            var document = FerruleJsonSourceDocument.Parse("  supplied/雪.json  ", input);
            Array.Fill(input, (byte)'x');
            Equal("  supplied/雪.json  ", document.Identity);
            Equal(true, JsonNode.DeepEquals(JsonNode.Parse(item.GetProperty("expected").GetRawText()),
                JsonSerializer.SerializeToNode(JsonSourceView(document.Root))));
        }
        foreach (var input in corpus.RootElement.GetProperty("invalid").EnumerateArray())
            JsonSourceRefuses(FerruleJsonSourceErrorKind.Syntax, Encoding.UTF8.GetBytes(input.GetString()!));
    }

    private static void JsonSourceRefuses(FerruleJsonSourceErrorKind expected, byte[] input,
        FerruleJsonSourceLimits? limits = null, string identity = "doc")
    {
        try { _ = FerruleJsonSourceDocument.Parse(identity, input, limits); }
        catch (FerruleJsonSourceException error) { Equal(expected, error.Kind); return; }
        throw new Exception($"Expected {expected} refusal.");
    }

    private static void JsonSourceSelectionAndLimits()
    {
        var document = FerruleJsonSourceDocument.Parse("doc", """
            {"v":"12","v":12,"v":null,"V":"upper","v":false,"v":[1],"v":{"x":2}}
            """u8);
        var values = document.Root.MembersNamed("v").ToArray();
        Equal(6, values.Length);
        Equal("12", values[0].StringValue); Equal((string?)null, values[0].NumberText);
        Equal("12", values[1].NumberText); Equal((string?)null, values[1].StringValue);
        Equal(FerruleJsonSourceKind.Null, values[2].Kind); Equal((string?)null, values[2].StringValue);
        Equal(false, values[3].BooleanValue); Equal(0, values[0].Items.Count);
        Equal(0, values[4].Members.Count); Equal("1", values[4].Items[0].NumberText);
        Equal("2", values[5].MembersNamed("x").Single().NumberText);
        Equal(0, document.Root.MembersNamed("missing").Count());
        Throws<NotSupportedException>(() =>
            ((IList<KeyValuePair<string, FerruleJsonSourceValue>>)document.Root.Members).Clear());
        var limits = new FerruleJsonSourceLimits { DocumentBytes = 7, ContainerDepth = 1, Values = 2 };
        _ = FerruleJsonSourceDocument.Parse("doc", "{\"x\":0}"u8, limits);
        JsonSourceRefuses(FerruleJsonSourceErrorKind.DocumentBytes, "{\"x\":0} "u8.ToArray(), limits);
        JsonSourceRefuses(FerruleJsonSourceErrorKind.ContainerDepth, "[[]]"u8.ToArray(), limits);
        JsonSourceRefuses(FerruleJsonSourceErrorKind.Values, "[0,1]"u8.ToArray(), limits);
        JsonSourceRefuses(FerruleJsonSourceErrorKind.Values, "null"u8.ToArray(), limits with { Values = 0 });
        _ = FerruleJsonSourceDocument.Parse("doc", "\"[{}]\""u8, limits with { ContainerDepth = 0 });
        foreach (var input in new byte[][] { [0xff], [0xef, 0xbb, 0xbf, 0xc0, 0xaf], [0xed, 0xa0, 0x80] })
            JsonSourceRefuses(FerruleJsonSourceErrorKind.Utf8, input);
        foreach (var name in new[] { "", "a\0b", new string('雪', 1366), "\ud800" })
            JsonSourceRefuses(FerruleJsonSourceErrorKind.InvalidIdentity, "null"u8.ToArray(), identity: name);
        _ = FerruleJsonSourceDocument.Parse(new string('x', 4096), "null"u8);
        foreach (var depth in new[] { 64, 65, 129 })
        {
            var input = Encoding.UTF8.GetBytes(new string('[', depth) + "0" + new string(']', depth));
            if (depth == 64) _ = FerruleJsonSourceDocument.Parse("doc", input);
            else JsonSourceRefuses(FerruleJsonSourceErrorKind.ContainerDepth, input);
        }
        JsonSourceRefuses(FerruleJsonSourceErrorKind.InvalidLimits, "null"u8.ToArray(), limits with { ContainerDepth = 65 });
    }
}
