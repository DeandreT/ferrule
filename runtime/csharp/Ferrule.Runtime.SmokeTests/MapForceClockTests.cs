using System.Text;
using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void MapForceClocks()
    {
        static object Fields(string name, int count, bool repeating = false) => new
        {
            name, repeating,
            kind = new { kind = "group", children = Enumerable.Range(1, count).Select(i => (object)new { name = $"E{i:D2}", repeating = false, kind = new { kind = "scalar", ty = "string" } }).ToArray() }
        };
        static object Node(string name, object[] children, bool repeating = false) => new { name, repeating, kind = new { kind = "group", children } };
        var schema = Node("Envelope", [Node("Interchange", [Fields("ISA", 16, true), Node("Group", [Fields("GS", 8, true), Node("Message_850", [Fields("ST", 2), Fields("SE", 2)], true), Fields("GE", 2, true)], true), Fields("IEA", 2, true)], true)]);
        string boundary = JsonSerializer.Serialize(new
        {
            version = "006040", schema = JsonSerializer.Serialize(schema), envelope_profile = "mapforce_interchanges",
            separators = new { element = "*", component = ":", segment = "~", repetition = "^" },
            constraints = Array.Empty<object>(), lexical_formats = Array.Empty<object>(), implied_decimals = Array.Empty<object>(),
            autocomplete = new { request_acknowledgement = true, transaction_set = "850" }, lenient_segments = true
        });
        string descriptor = JsonSerializer.Serialize(new { boundary, minimums = Array.Empty<object>() });
        static FerruleGroup Group(params (string Name, FerruleInstance Value)[] fields) => new(fields.Select(f => new FerruleField(f.Name, f.Value)));
        static FerruleRepeated Repeated(FerruleInstance value) => new([value]);
        var message = Group(("ST", Group()));
        var functionalGroup = Group(("GS", Repeated(Group())), ("Message_850", Repeated(message)));
        var interchange = Group(("ISA", Repeated(Group())), ("Group", Repeated(functionalGroup)));
        var body = Group(("Interchange", Repeated(interchange)));
        var context = new FerruleExecutionContext("original.mfd", "original.mfd", null, FerruleRuntimeParameters.Empty);
        var calls = new List<FerruleMapForceX12ClockField>();
        string Clock(FerruleMapForceX12ClockField field)
        {
            calls.Add(field);
            return field switch
            {
                FerruleMapForceX12ClockField.InterchangeDate => "2027-01-02T10:20:30",
                FerruleMapForceX12ClockField.InterchangeTime => "2028-02-03T04:05:06",
                FerruleMapForceX12ClockField.GroupDate => "2029-03-04T13:14:15",
                _ => "2030-04-05T16:17:18",
            };
        }
        string text = Encoding.Latin1.GetString(FerruleX12.SerializeMapForceDocumentBytesWithClock(descriptor, body, context, Clock));
        Equal(true, text.Contains("*270102*0405*", StringComparison.Ordinal));
        Equal(true, text.Contains("GS****20290304*1617*", StringComparison.Ordinal));
        Equal(true, calls.SequenceEqual(Enum.GetValues<FerruleMapForceX12ClockField>()));
        var source = FerruleX12.ParseLexicalSourceBytes(boundary, Encoding.UTF8.GetBytes(text.Replace("20290304", "bad-date", StringComparison.Ordinal)));
        static FerruleInstance Field(FerruleInstance value, string name) => ((FerruleGroup)value).Fields.Single(f => f.Name == name).Value;
        var sourceInterchange = ((FerruleRepeated)Field(source, "Interchange")).Items[0];
        var sourceGroup = ((FerruleRepeated)Field(sourceInterchange, "Group")).Items[0];
        var sourceGs = ((FerruleRepeated)Field(sourceGroup, "GS")).Items[0];
        Equal("", ((FerruleScalar)Field(sourceGs, "E01")).Value.StringValue);
        Equal("bad-date", ((FerruleScalar)Field(sourceGs, "E04")).Value.StringValue);
        Equal(FerruleValueKind.Null, ((FerruleScalar)Field(sourceGs, "E08")).Value.Kind);
        Throws<FerruleX12Exception>(() => FerruleX12.ParseEmbedded(boundary, text.Replace("20290304", "bad-date", StringComparison.Ordinal)));
        var projectedDescriptor = System.Text.Json.Nodes.JsonNode.Parse(boundary)!;
        projectedDescriptor["lexical_formats"] = JsonSerializer.SerializeToNode(new object[] {
            new { path = new[] { "Interchange", "ISA", "E09" }, kind = (object)"compact_date6" },
            new { path = new[] { "Interchange", "Group", "GS", "E04" }, kind = (object)"compact_date8" },
            new { path = new[] { "Interchange", "Group", "GS", "E05" }, kind = (object)new { compact_time = new { min_digits = 4, max_digits = 8 } } },
        });
        var projected = FerruleX12.ParseMapForceSourceBytes(projectedDescriptor.ToJsonString(), Encoding.UTF8.GetBytes(text.Replace("20290304", "491399", StringComparison.Ordinal).Replace("*1617*", "*abCDef*", StringComparison.Ordinal)));
        var projectedInterchange = ((FerruleRepeated)Field(projected, "Interchange")).Items[0];
        var projectedGroup = ((FerruleRepeated)Field(projectedInterchange, "Group")).Items[0];
        var projectedGs = ((FerruleRepeated)Field(projectedGroup, "GS")).Items[0];
        Equal("2049-13-99", ((FerruleScalar)Field(projectedGs, "E04")).Value.StringValue);
        Equal("ab:CD:ef", ((FerruleScalar)Field(projectedGs, "E05")).Value.StringValue);
        Equal(FerruleValueKind.Null, ((FerruleScalar)Field(projectedGs, "E01")).Value.Kind);
        for (int fail = 1; fail <= 4; fail++)
        {
            int count = 0;
            using var stream = new MemoryStream();
            Throws<IOException>(() => FerruleX12.WriteMapForceDocumentWithClock(descriptor, body, context, _ => ++count == fail ? throw new IOException("injected") : "2027-01-02T03:04:05", stream));
            Equal(fail, count); Equal(0L, stream.Length);
        }
        calls.Clear();
        Throws<FerruleX12Exception>(() => FerruleX12.SerializeMapForceDocumentBytesWithClock(descriptor, Group(("Interchange", new FerruleRepeated([]))), context, Clock));
        Equal(0, calls.Count);
        var acquisitionEvents = new List<string>();
        var acquisition = FerruleX12SourceAcquisition.Acquire(boundary,
            () => { acquisitionEvents.Add("filename"); return "pattern"; },
            path => { acquisitionEvents.Add("select:" + path); return new[] { "a", "b" }; },
            path => { acquisitionEvents.Add("load:" + path); return FerruleX12.ParseMapForceSourceDocument(boundary, path, Encoding.UTF8.GetBytes(text)); }, 1);
        Equal(0, acquisitionEvents.Count);
        using (var iterator = acquisition.GetEnumerator())
        {
            Equal(true, iterator.MoveNext()); Equal("a", iterator.Current.Path);
            Equal(true, acquisitionEvents.SequenceEqual(new[] { "filename", "select:pattern", "load:a" }));
            try { iterator.MoveNext(); throw new InvalidOperationException("Missing source limit."); }
            catch (FerruleX12AcquisitionException error) { Equal(FerruleX12AcquisitionStage.Limit, error.Stage); }
            Equal(false, iterator.MoveNext());
        }
        var mismatch = FerruleX12SourceAcquisition.Acquire(boundary, () => "pattern", _ => new[] { "a" },
            path => FerruleX12.ParseMapForceSourceDocument(boundary + " ", path, Encoding.UTF8.GetBytes(text)), 1);
        try { _ = mismatch.ToArray(); throw new InvalidOperationException("Missing descriptor refusal."); }
        catch (FerruleX12AcquisitionException error) { Equal(FerruleX12AcquisitionStage.Descriptor, error.Stage); }
    }
}
