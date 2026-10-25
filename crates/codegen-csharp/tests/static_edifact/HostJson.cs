using System.Text;
using System.Text.Json.Nodes;
using Ferrule.Generated;
using Ferrule.Runtime;

var wire = File.ReadAllText("input.edi");
var json = File.ReadAllText("expected.json");
var legacy = File.ReadAllText("legacy.x12");
var named = new[] { new NamedDocumentInput("Reference", wire), new NamedDocumentInput("Legacy", legacy) };
var expected = File.ReadAllText("expected.edi");
var complete = GeneratedMapping.ExecuteDocumentOutputsWithHost(json, named);
Check(complete.PrimaryFormat == DocumentBoundaryFormat.Json, "JSON source/target identity");
Check(complete.Extras[1].Document == expected, "JSON to named EDIFACT parity");

var malformed = JsonNode.Parse(json)!;
malformed["Interchanges"]![0]!["UNZ"]!["F1"] = "999";
var badJson = malformed.ToJsonString();
// The selected JSON target may succeed, but complete publication must fail on
// the later EDIFACT writer. There is no partially returned output bundle.
Check(GeneratedMapping.ExecuteDocumentSelectedTargetWithHost(badJson, FerruleTargetSelection.Primary(), named)
    is SelectedDocumentTargetOutput.Primary { Format: DocumentBoundaryFormat.Json }, "selected JSON is independent");
Refuses(() => GeneratedMapping.ExecuteDocumentOutputsWithHost(badJson, named));
Refuses(() => GeneratedMapping.ExecuteDocumentBytesOutputsWithHost(Encoding.UTF8.GetBytes(badJson),
    new[] { new NamedDocumentBytesInput("Reference", Encoding.UTF8.GetBytes(wire)), new NamedDocumentBytesInput("Legacy", Encoding.UTF8.GetBytes(legacy)) }));
Refuses(() => GeneratedMapping.ExecuteDocumentSelectedTargetWithHost(badJson, FerruleTargetSelection.Named("Wire"), named));
var missingTrailer = JsonNode.Parse(json)!;
missingTrailer["Interchanges"]![0]!.AsObject().Remove("UNZ");
Refuses(() => GeneratedMapping.ExecuteDocumentOutputsWithHost(missingTrailer.ToJsonString(), named));

static void Refuses(Action action) {
    try { action(); throw new Exception("invalid output envelope was published"); }
    catch (DocumentExecutionException error) {
        Check(error.Boundary is FerruleEdiException { Code: FerruleEdiError.Envelope, SegmentIndex: >= 0 }, "typed output envelope cause");
        Check(error.Owner is DocumentBoundaryOwner.NamedTarget { DeclarationIndex: 1, Name: "Wire" }, "typed named output ownership");
    }
}
static void Check(bool condition, string message) { if (!condition) throw new Exception(message); }
