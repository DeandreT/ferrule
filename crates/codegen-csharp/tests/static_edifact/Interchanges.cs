using System.Text;
using System.Text.Json.Nodes;
using Ferrule.Generated;
using Ferrule.Runtime;

var context = new FerruleExecutionContext("mapping.json", "mapping.json", "2026-10-26T12:00:00Z");
var legacy = File.ReadAllText("legacy.x12");
foreach (var item in JsonNode.Parse(File.ReadAllText("cases.json"))!.AsArray())
{
    var test = item!;
    var name = (string)test["name"]!;
    var wire = (string)test["wire"]!;
    var expected = (string)test["output"]!;
    var named = new[] { new NamedDocumentInput("Reference", wire), new NamedDocumentInput("Legacy", legacy) };
    var namedBytes = named.Select(value => new NamedDocumentBytesInput(value.Name, Encoding.UTF8.GetBytes(value.Document))).ToArray();
    var actual = GeneratedMapping.ExecuteDocumentOutputsWithHost(wire, named, context);
    Check(actual.Primary == expected, name + " canonical wire");
    Check(actual.Extras[1].Document == expected, name + " named canonical wire");
    var json = JsonNode.Parse(actual.Extras[0].Document)!;
    Check(JsonNode.DeepEquals(json, JsonNode.Parse((string)test["json"]!)), name + " whole schema");
    for (int at = 0; at < 2; at++)
    {
        var bgm = json["Interchanges"]![at]!["Messages"]![0]!["BGM"]!;
        Check((string?)bgm["F1"] == (string?)test["codes"]![at], name + " scalar component spelling");
        Check((string?)bgm["F2"] == (string?)test["values"]![at], name + " released and UNA-looking data");
    }
    var bytes = GeneratedMapping.ExecuteDocumentBytesOutputsWithHost(Encoding.UTF8.GetBytes(wire), namedBytes, context);
    Check(Encoding.UTF8.GetString(bytes.Primary) == expected, name + " byte wire");
    var selected = GeneratedMapping.ExecuteDocumentSelectedTargetWithHost(wire, FerruleTargetSelection.Named("Wire"), named, context);
    Check(selected is SelectedDocumentTargetOutput.Named n && n.Output.Document == expected, name + " selected");
    var selectedBytes = GeneratedMapping.ExecuteDocumentBytesSelectedTargetWithHost(Encoding.UTF8.GetBytes(wire), FerruleTargetSelection.Named("Wire"), namedBytes, context);
    Check(selectedBytes is SelectedDocumentBytesTargetOutput.Named b && Encoding.UTF8.GetString(b.Output.Document) == expected, name + " selected bytes");
    var badReference = wire.Replace("UNT+3+MA", "UNT+3+wrong").Replace("UNT|3|MB", "UNT|3|wrong").Replace("UNT;3;MC", "UNT;3;wrong");
    Refuses(badReference, named, FerruleEdiError.Envelope, false);
    Refuses(wire, [new("Reference", badReference), new("Legacy", legacy)], FerruleEdiError.Envelope, true);
    foreach (string bad in new[] { "UNA;|", "UNA;;.! $UNH|M$UNT|2|M$" })
        Refuses(wire + bad, named, FerruleEdiError.Format, false);
}
// The same emitted shared codec also retains standalone and versioned repetition semantics.
const string bodySchema = """
{"name":"Root","kind":{"kind":"group","children":[{"name":"BGM","repeating":true,"kind":{"kind":"group","children":[{"name":"Data","repeating":true,"kind":{"kind":"scalar","ty":"string"}}]}}]}}
""";
var bodyCodec = new FerruleEdi(bodySchema, "{\"lenient_segments\":true}", "Edifact", input: true, strictEnvelope: true);
string[] Values(string text)
{
    var root = (FerruleGroup)bodyCodec.Parse(Encoding.UTF8.GetBytes(text));
    var segments = (FerruleRepeated)root.Fields[0].Value;
    return segments.Items.SelectMany(segment => ((FerruleRepeated)((FerruleGroup)segment).Fields[0].Value).Items)
        .Select(value => ((FerruleScalar)value).Value.StringValue).ToArray();
}
Check(Values("UNA;|.!^$UNB|UNOC;4|S|R|D|A$UNH|M$BGM|A!^B^C$UNT|3|M$UNZ|1|A$\nUNB+UNOC:3+S+R+D+B'UNH+N'BGM+A*B'UNT+3+N'UNZ+1+B'")
    .SequenceEqual(new[] { "A^B", "C", "A*B" }), "v4 to v3 repetition reset");
Check(Values("UNA;|.! $UNH|M$BGM|A!!$UNT|3|M$\nUNH+N'BGM+A??'UNT+3+N'")
    .SequenceEqual(new[] { "A!", "A?" }), "standalone default reset");
try
{
    bodyCodec.Parse(Encoding.UTF8.GetBytes("UNB+UNOC:3+S+R+D+A'UNH+M'UNA;|.! $BGM|X$UNT|3|M$UNZ|1|A$"));
    throw new Exception("mid-message UNA accepted");
}
catch (FerruleEdiException error) when (error.Code == FerruleEdiError.Envelope) { }
var trailing = "UNB+UNOC:3+S+R+D+A'UNH+M'BGM+X'UNT+3+M'UNZ+1+A'ZZZ+unknown'";
new FerruleEdi(bodySchema, "{\"lenient_segments\":true}", "Edifact", input: true).Parse(Encoding.UTF8.GetBytes(trailing));
try { bodyCodec.ValidateEnvelope(Encoding.UTF8.GetBytes(trailing)); throw new Exception("strict trailing data accepted"); }
catch (FerruleEdiException error) when (error.Code == FerruleEdiError.Envelope && error.SegmentIndex == 5) { }
Console.WriteLine("EDIFACT interchange generated text/bytes, named/selected, schema and output parity passed.");

static void Refuses(string wire, NamedDocumentInput[] named, FerruleEdiError code, bool secondary)
{
    try { GeneratedMapping.ExecuteDocumentOutputsWithHost(wire, named); throw new Exception("accepted malformed interchange"); }
    catch (DocumentExecutionException error)
    {
        Check(error.Boundary is FerruleEdiException edi && edi.Code == code, "typed error");
        Check(secondary ? error.Owner is DocumentBoundaryOwner.NamedSource { Name: "Reference", DeclarationIndex: 0 } : error.Owner is DocumentBoundaryOwner.Source, "typed owner");
    }
}
static void Check(bool condition, string message) { if (!condition) throw new Exception(message); }
