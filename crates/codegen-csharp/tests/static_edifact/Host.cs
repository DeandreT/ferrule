using System.Text;
using System.Text.Json.Nodes;
using Ferrule.Generated;
using Ferrule.Runtime;

var text = File.ReadAllText("input.edi");
var bytes = Encoding.UTF8.GetBytes(text);
var expected = File.ReadAllText("expected.edi");
var legacy = File.ReadAllText("legacy.x12");
var named = new[] { new NamedDocumentInput("Legacy", legacy), new NamedDocumentInput("Reference", text) };
var namedBytes = new[] { new NamedDocumentBytesInput("Legacy", Encoding.UTF8.GetBytes(legacy)), new NamedDocumentBytesInput("Reference", bytes) };
var context = new FerruleExecutionContext("/logical/mapping.json", "/logical/main.json", "2026-10-25T12:00:00Z");
var complete = GeneratedMapping.ExecuteDocumentOutputsWithHost(text, named, context);
Check(complete.Primary == expected, "primary native parity");
Check(complete.PrimaryFormat == DocumentBoundaryFormat.Edifact, "primary format");
Check(complete.Extras.Select(item => item.Name).SequenceEqual(new[] { "Json", "Wire" }), "declaration order");
Check(JsonNode.DeepEquals(JsonNode.Parse(complete.Extras[0].Document), JsonNode.Parse(File.ReadAllText("expected.json"))), "JSON native parity");
Check(complete.Extras[1].Document == expected, "named native parity");
var binary = GeneratedMapping.ExecuteDocumentBytesOutputsWithHost(bytes, namedBytes, context);
Check(Encoding.UTF8.GetString(binary.Primary) == expected, "bytes parity");
var selected = GeneratedMapping.ExecuteDocumentSelectedTargetWithHost(text, FerruleTargetSelection.Named("Wire"), named, context);
Check(selected is SelectedDocumentTargetOutput.Named n && n.Output.Document == expected, "selected named parity");
var selectedBytes = GeneratedMapping.ExecuteDocumentBytesSelectedTargetWithHost(bytes, FerruleTargetSelection.Named("Wire"), namedBytes, context);
Check(selectedBytes is SelectedDocumentBytesTargetOutput.Named nb && Encoding.UTF8.GetString(nb.Output.Document) == expected, "selected named bytes parity");
var primary = GeneratedMapping.ExecuteDocumentBytesSelectedTargetWithHost(bytes, FerruleTargetSelection.Primary(), namedBytes, context);
Check(primary is SelectedDocumentBytesTargetOutput.Primary p && Encoding.UTF8.GetString(p.Document) == expected, "selected primary parity");

// Custom UNA service characters are input syntax; output is canonical native syntax.
var v4 = text.Contains("UNOC:4", StringComparison.Ordinal);
// Transcribe this fixed fixture without changing its released data characters.
var customBody = text.Replace("?+", "\u0001").Replace("?*", "\u0002").Replace("|", "\u0003").Replace("^", "\u0004")
    .Replace('?', '!').Replace(':', ';').Replace('+', '|').Replace('\'', '$');
if (v4) customBody = customBody.Replace('*', '^');
customBody = customBody.Replace("\u0001", "+").Replace("\u0002", "*").Replace("\u0003", "!|").Replace("\u0004", v4 ? "!^" : "^");
var custom = (v4 ? "UNA;|.!^$" : "UNA;|.! $") + customBody;
Check(GeneratedMapping.ExecuteDocumentOutputsWithHost(custom, named, context).Primary == expected, "UNA custom syntax");
var unicodeCustom = custom.Replace("!|", "\u0005").Replace("|", "😀").Replace("\u0005", "|");
Check(GeneratedMapping.ExecuteDocumentOutputsWithHost(unicodeCustom, named, context).Primary == expected, "UNA Unicode scalar syntax");
Check(Encoding.UTF8.GetString(GeneratedMapping.ExecuteDocumentBytesOutputsWithHost(Encoding.UTF8.GetBytes(unicodeCustom), namedBytes, context).Primary) == expected, "UNA Unicode byte syntax");
foreach (var malformed in new[] {
    text.Replace("UNT+3+", "UNT+8+"),
    text.Replace("UNT+3+M11", "UNT+3+WRONG"),
    text.Replace("UNZ+2+", "UNZ+8+").Replace("UNZ+1+", "UNZ+8+"),
    text.Replace("UNZ+2+I1", "UNZ+2+WRONG").Replace("UNZ+1+I1", "UNZ+1+WRONG"),
    text.Replace("UNE+2+", "UNE+8+"),
    text.Replace("UNE+2+G1", "UNE+2+WRONG"),
    text[..text.LastIndexOf("UNZ+", StringComparison.Ordinal)],
    text + "UNH+LOOSE+ORDERS'",
}) {
    if (malformed == text) continue;
    Refuses(() => GeneratedMapping.ExecuteDocumentOutputsWithHost(malformed, named), false);
    Refuses(() => GeneratedMapping.ExecuteDocumentOutputsWithHost(text, new[] { new NamedDocumentInput("Reference", malformed), new NamedDocumentInput("Legacy", legacy) }), true);
    Refuses(() => GeneratedMapping.ExecuteDocumentSelectedTargetWithHost(malformed, FerruleTargetSelection.Primary(), named), false);
}
Refuses(() => GeneratedMapping.ExecuteDocumentBytesOutputsWithHost(new byte[] { 0xff }, namedBytes), false, FerruleEdiError.Format);
Refuses(() => GeneratedMapping.ExecuteDocumentBytesOutputsWithHost(new byte[64 * 1024 * 1024 + 1], namedBytes), false, FerruleEdiError.Limit);

static void Refuses(Action action, bool named, FerruleEdiError expected = FerruleEdiError.Envelope) {
    try { action(); throw new Exception("malformed boundary accepted"); }
    catch (DocumentExecutionException error) {
        Check(error.Boundary is FerruleEdiException edi && edi.Code == expected, "typed EDI cause");
        if (expected == FerruleEdiError.Envelope) Check(error.Boundary is FerruleEdiException { SegmentIndex: >= 0 }, "typed envelope location");
        Check(named ? error.Owner is DocumentBoundaryOwner.NamedSource { DeclarationIndex: 0, Name: "Reference" } : error.Owner is DocumentBoundaryOwner.Source, "typed owner");
    }
}
static void Check(bool condition, string message) { if (!condition) throw new Exception(message); }
