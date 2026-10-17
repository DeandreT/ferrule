using System.Text.Json;
using Microsoft.Diagnostics.Tracing.Etlx;
using Microsoft.Diagnostics.Tracing.Parsers.Clr;

if (args.Length != 2) throw new ArgumentException("trace path and absent analysis directory required");
var trace = Path.GetFullPath(args[0]);
var output = Path.GetFullPath(args[1]);
if (Directory.Exists(output) || File.Exists(output)) throw new IOException("analysis output exists");
Directory.CreateDirectory(output);
var etlx = TraceLog.CreateFromEventPipeDataFile(trace, Path.Combine(output, "events.etlx"));
using var log = new TraceLog(etlx);
using var source = log.Events.GetSource();
using var events = new StreamWriter(Path.Combine(output, "allocation-ticks.jsonl"));
using var heaps = new StreamWriter(Path.Combine(output, "gc-heap-stats.jsonl"));
var totals = new Dictionary<string, long>(StringComparer.Ordinal);
var counts = new Dictionary<string, long>(StringComparer.Ordinal);
long noStack = 0, ticks = 0, heapEvents = 0;
source.Clr.GCAllocationTick += item => {
    var frames = new List<string>();
    for (var stack = item.CallStack(); stack is not null; stack = stack.Caller)
        frames.Add(stack.CodeAddress.FullMethodName);
    var phase = Classify(frames);
    var amount = item.AllocationAmount64;
    if (frames.Count == 0) noStack++;
    totals[phase] = totals.GetValueOrDefault(phase) + amount;
    counts[phase] = counts.GetValueOrDefault(phase) + 1;
    ticks++;
    events.WriteLine(JsonSerializer.Serialize(new { ms=item.TimeStampRelativeMSec, pid=item.ProcessID,
        thread=item.ThreadID, type=item.TypeName, kind=item.AllocationKind.ToString(),
        reportedAllocationBytes=amount, phase, frames }));
};
source.Clr.GCHeapStats += item => {
    heapEvents++;
    // A runtime post-GC statistic, not an object graph, retained roots, or RSS.
    heaps.WriteLine(JsonSerializer.Serialize(new { ms=item.TimeStampRelativeMSec, pid=item.ProcessID,
        size0=item.GenerationSize0, size1=item.GenerationSize1, size2=item.GenerationSize2,
        size3=item.GenerationSize3, size4=item.GenerationSize4,
        totalPromoted0=item.TotalPromotedSize0, totalPromoted1=item.TotalPromotedSize1,
        totalPromoted2=item.TotalPromotedSize2, totalPromoted3=item.TotalPromotedSize3 }));
};
source.Process();
File.WriteAllText(Path.Combine(output, "summary.json"), JsonSerializer.Serialize(new {
    trace, ticks, noStack, heapEvents, eventsLost=log.EventsLost,
    reportedAllocationBytes=totals, tickCounts=counts,
    meaning="GCAllocationTick sampled allocation accounting; not exact per-phase allocations, live/retained heap, or an API-only peak"
}, new JsonSerializerOptions { WriteIndented=true })+"\n");
if (ticks == 0 || log.EventsLost != 0) throw new InvalidOperationException("missing allocation ticks or lost events; retain inconclusive originals");

static string Classify(List<string> frames)
{
    // Specific enclosing phases precede shared helpers and public wrappers.
    // CountBoundaryBytes is used by output validation too; its presence alone
    // must never turn serializer allocations into normalization allocations.
    bool Has(string name) => frames.Any(frame => frame.Contains(name, StringComparison.Ordinal));
    if (Has("FerruleJson5Syntax.Normalize") || Has("FerruleJson5Syntax+Parser.")) return "normalization";
    if (Has("FerruleJson.Json5Serialize") || Has("FerruleJson.CanonicalizeOutputStrings") ||
        Has("FerruleJson.WriteNode")) return "serialization";
    if (Has("FerruleJson.ReadNode") || Has("FerruleJson.ReadScalar") ||
        Has("FerruleJson.ValidateInputJsonScalars")) return "typed-projection";
    if (frames.Any(frame => frame.Contains("GeneratedMapping.", StringComparison.Ordinal) &&
        !frame.Contains("ExecuteJson5", StringComparison.Ordinal))) return "mapping";
    if (Has("FerruleJson.ExecuteJson5Accepted") || Has("FerruleJson.Json5Schema") ||
        Has("FerruleJson.ValidateJson5Descriptor") || Has("FerruleJson.ParseSchema")) return "schema-or-json-parse";
    if (Has("FerruleJson5.Execute") || Has("FerruleJson5Syntax.Count")) return "boundary-encoding";
    if (frames.Any(frame => frame.Contains("Ferrule.Runtime.", StringComparison.Ordinal))) return "other-runtime";
    return frames.Count == 0 ? "unresolved" : "host-or-framework";
}
