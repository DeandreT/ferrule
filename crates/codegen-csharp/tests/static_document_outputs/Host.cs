using System.Collections;
using System.Reflection;
using System.Text;
using System.Text.Json;
using Ferrule.Generated;
using Ferrule.Runtime;

internal static class Program
{
    private static readonly UTF8Encoding Utf8 = new(false, true);
    private static string fixtures = "";
    private static string observations = "";
    private static int checks;

    private static void Main(string[] args)
    {
        fixtures = args[0];
        observations = args[1];
        Directory.CreateDirectory(observations);
        var original = Directory.GetFiles(fixtures).ToDictionary(path => path, File.ReadAllBytes);
        try
        {
#if MIXED
            Mixed();
#elif PRIMARY
            Primary();
#elif GROUPED
            Grouped();
#elif SINGLE
            Single();
#endif
            Budgets();
        }
        finally
        {
            // Check all literal fixture bytes even if the first assertion failed.
            File.WriteAllText(Path.Combine(observations, "fixture-guards.json"), JsonSerializer.Serialize(
                original.Select(pair => new { Path = pair.Key, Unchanged = File.ReadAllBytes(pair.Key).SequenceEqual(pair.Value) })));
        }
        Require(original.All(pair => File.ReadAllBytes(pair.Key).SequenceEqual(pair.Value)), "fixture mutation");
        Console.WriteLine($"complete physical document outputs: {checks} checks passed");
    }

    private static string Text(string file) => File.ReadAllText(Path.Combine(fixtures, file), Utf8);
    private static byte[] Bytes(string file) => File.ReadAllBytes(Path.Combine(fixtures, file));
    // JSON fixture files have a framing LF; generated compact JSON has no LF.
    private static string Json(string file) => Text(file).TrimEnd('\r', '\n');
    private static void Require(bool condition, string detail)
    {
        checks++;
        if (!condition) throw new Exception(detail);
    }

    private static FerruleExecutionContext Context(string? clock = "2028-02-29T12:34:56+05:30") => new(
        "authored/map.mfd", "authored/main.mfd", clock,
        new FerruleRuntimeParameters([new("tag", FerruleValue.FromString("HOST-TAG"))]));

    private static NamedDocumentInput[] Inputs(string? reference = null, string? unused = null) =>
        [new("Unused", unused ?? Text("unused.json")), new("Reference", reference ?? Text("reference.x12"))];
    private static NamedDocumentBytesInput[] ByteInputs(byte[]? reference = null, byte[]? unused = null) =>
        [new("Unused", unused ?? Bytes("unused.json")), new("Reference", reference ?? Bytes("reference.x12"))];

    private static void Retain(string label, DocumentExecutionOutputs bundle)
    {
        File.WriteAllText(Path.Combine(observations, label + "-primary.txt"), bundle.Primary, Utf8);
        for (var index = 0; index < bundle.Extras.Count; index++)
            File.WriteAllText(Path.Combine(observations, $"{label}-named-{index}.txt"), bundle.Extras[index].Document, Utf8);
        File.WriteAllText(Path.Combine(observations, label + "-bundle.json"), JsonSerializer.Serialize(bundle));
    }
    private static void Retain(string label, DocumentBytesExecutionOutputs bundle)
    {
        File.WriteAllBytes(Path.Combine(observations, label + "-primary.bin"), bundle.Primary);
        for (var index = 0; index < bundle.Extras.Count; index++)
            File.WriteAllBytes(Path.Combine(observations, $"{label}-named-{index}.bin"), bundle.Extras[index].Document);
        File.WriteAllText(Path.Combine(observations, label + "-bundle.json"), JsonSerializer.Serialize(bundle));
    }

    private static Exception Refuse(string label, Func<object> action, Func<Exception, bool> accepts)
    {
        object? published = null;
        Exception? failure = null;
        try { published = action(); }
        catch (Exception error) { failure = error; }
        // Keep the complete original failure before comparing its category/owner.
        File.WriteAllText(Path.Combine(observations, label + "-failure.txt"), failure?.ToString() ?? "NO FAILURE");
        Require(published is null && failure is not null, label + ": a bundle escaped refusal");
        Require(accepts(failure!), label + ": wrong original failure/owner");
        return failure!;
    }
    private static bool Owned(Exception error, DocumentBoundaryOwner owner, Type boundary) =>
        error is DocumentExecutionException failure && Equals(failure.Owner, owner)
        && boundary.IsInstanceOfType(failure.Boundary) && ReferenceEquals(failure.Boundary, failure.InnerException);
    private static bool NameError(Exception error, FerruleRuntimeError kind) =>
        error is FerruleRuntimeException failure && failure.Error == kind;

#if MIXED
    private static void Mixed()
    {
        var inputs = new CountedInputs<NamedDocumentInput>(Inputs());
        var text = GeneratedMapping.ExecuteDocumentOutputsWithHost(Text("input.json"), inputs, Context());
        Retain("mixed-text", text);
        Require(inputs.Reads.SequenceEqual(new[] { 1, 1 }), "each input envelope must be snapshotted once");
        Require(text.PrimaryFormat == DocumentBoundaryFormat.Json && text.Primary == Json("primary.expected.json"), "complete primary JSON/context");
        Require(text.Extras.Select(output => output.Name).SequenceEqual(new[] { "Zeta", "Wire", "Alpha" }), "declaration order, not sorted names");
        Require(text.Extras.Select(output => output.Format).SequenceEqual(new[] { DocumentBoundaryFormat.Json, DocumentBoundaryFormat.X12, DocumentBoundaryFormat.Json }), "endpoint formats");
        Require(text.Extras[0].Document == Json("zeta.expected.json"), "Zeta ownership");
        Require(text.Extras[1].Document == Text("wire.expected.x12"), "named 005010 syntax/completed envelope bytes");
        Require(text.Extras[2].Document == Json("alpha.expected.json"), "Alpha ownership");
        Require(text.Extras is not NamedDocumentOutput[], "caller must not reorder bundle backing array");
        var byteInputs = ByteInputs();
        var before = byteInputs.Select(input => input.Document.ToArray()).ToArray();
        var bytes = GeneratedMapping.ExecuteDocumentBytesOutputsWithHost(Bytes("input.json"), byteInputs, Context());
        Retain("mixed-bytes", bytes);
        Require(bytes.PrimaryFormat == text.PrimaryFormat && bytes.Primary.SequenceEqual(Utf8.GetBytes(text.Primary)), "primary bytes");
        for (var i = 0; i < text.Extras.Count; i++)
            Require(bytes.Extras[i].Name == text.Extras[i].Name && bytes.Extras[i].Format == text.Extras[i].Format
                && bytes.Extras[i].Document.SequenceEqual(Utf8.GetBytes(text.Extras[i].Document)), "named exact bytes/order");
        Require(before.Zip(byteInputs).All(pair => pair.First.SequenceEqual(pair.Second.Document)), "input byte ownership");

        foreach (var binary in new[] { false, true })
        {
            object Invoke(string source, NamedDocumentInput[] supplied, FerruleExecutionContext? context = null) => binary
                ? GeneratedMapping.ExecuteDocumentBytesOutputsWithHost(Utf8.GetBytes(source),
                    supplied.Select(input => new NamedDocumentBytesInput(input.Name, Utf8.GetBytes(input.Document))).ToArray(), context ?? Context())
                : GeneratedMapping.ExecuteDocumentOutputsWithHost(source, supplied, context ?? Context());
            var route = binary ? "bytes-" : "text-";
            Refuse(route + "unused-malformed", () => Invoke(Text("input.json"), Inputs(unused: "{\"permit\":true} trailing")),
                error => Owned(error, new DocumentBoundaryOwner.NamedSource(1, "Unused"), typeof(FerruleRuntimeException)));
            Refuse(route + "unused-type", () => Invoke(Text("input.json"), Inputs(unused: "{\"permit\":\"true\"}")),
                error => Owned(error, new DocumentBoundaryOwner.NamedSource(1, "Unused"), typeof(FerruleRuntimeException)));
            Refuse(route + "reference-envelope", () => Invoke(Text("input.json"), Inputs(reference: Text("reference.x12").Replace("IEA*1*000000123~", ""))),
                error => Owned(error, new DocumentBoundaryOwner.NamedSource(0, "Reference"), typeof(FerruleX12Exception)));
            Refuse(route + "primary-type", () => Invoke("{\"message\":7}", Inputs()),
                error => Owned(error, new DocumentBoundaryOwner.Source(), typeof(FerruleRuntimeException)));
            Refuse(route + "duplicate-before-parse", () => Invoke("not JSON", [Inputs()[0], Inputs()[0]]),
                error => NameError(error, FerruleRuntimeError.DuplicateNamedSource));
            Refuse(route + "missing-before-parse", () => Invoke("not JSON", [Inputs()[0]]),
                error => NameError(error, FerruleRuntimeError.MissingNamedSource));
            Refuse(route + "unexpected-before-parse", () => Invoke("not JSON", [new("reference", "not X12"), Inputs()[0]]),
                error => NameError(error, FerruleRuntimeError.UnexpectedNamedSource));
            Refuse(route + "late-x12-writer", () => Invoke("{\"message\":\"BAD|VALUE\"}", Inputs()),
                error => Owned(error, new DocumentBoundaryOwner.NamedTarget(1, "Wire"), typeof(FerruleX12Exception)));
            Refuse(route + "late-json-writer", () => Invoke("{\"message\":\"DENY\"}", Inputs()),
                error => Owned(error, new DocumentBoundaryOwner.NamedTarget(2, "Alpha"), typeof(FerruleRuntimeException)));
            Refuse(route + "completion-clock", () => Invoke(Text("input.json"), Inputs(), Context("not-a-clock")),
                error => Owned(error, new DocumentBoundaryOwner.NamedTarget(1, "Wire"), typeof(FerruleX12Exception)));
        }
        Refuse("unused-text-unicode", () => GeneratedMapping.ExecuteDocumentOutputsWithHost(Text("input.json"), Inputs(unused: "\ud800"), Context()),
            error => Owned(error, new DocumentBoundaryOwner.NamedSource(1, "Unused"), typeof(EncoderFallbackException)));
        Refuse("unused-bytes-unicode", () => GeneratedMapping.ExecuteDocumentBytesOutputsWithHost(Bytes("input.json"), ByteInputs(unused: [0xff]), Context()),
            error => Owned(error, new DocumentBoundaryOwner.NamedSource(1, "Unused"), typeof(FerruleRuntimeException)));
        var oversized = new byte[FerruleJson.MaximumDocumentBytes + 1];
        Refuse("primary-document-byte-limit", () => GeneratedMapping.ExecuteDocumentBytesOutputsWithHost(oversized, ByteInputs(), Context()),
            error => Owned(error, new DocumentBoundaryOwner.Source(), typeof(FerruleRuntimeException)));
        Refuse("input-count-limit", () => GeneratedMapping.ExecuteDocumentOutputsWithHost("not JSON",
            new CountOnlyInputs<NamedDocumentInput>(DocumentExecutionLimits.MaximumDocuments), Context()),
            error => error is DocumentExecutionException { Owner: null, Boundary: DocumentBundleResourceException { Resource: "document_bundle_count" } });
        var scalarParameters = new FerruleExecutionContext("authored/map.mfd", "authored/main.mfd", "2028-02-29T12:34:56+05:30",
            new FerruleRuntimeParameters([new("tag", FerruleValue.FromInt64(7))]));
        var coerced = GeneratedMapping.ExecuteDocumentOutputsWithHost(Text("input.json"), Inputs(), scalarParameters);
        Retain("parameter-coercion", coerced);
        Require(coerced.Primary == Json("primary.expected.json").Replace("HOST-TAG", "7")
            && coerced.Extras[0].Document == Json("zeta.expected.json").Replace("HOST-TAG", "7"), "ordinary runtime parameter coercion");
        Refuse("missing-runtime-parameter", () => GeneratedMapping.ExecuteDocumentOutputsWithHost(Text("input.json"), Inputs(),
            new FerruleExecutionContext("map", "main", "2028-02-29T12:34:56Z")),
            error => NameError(error, FerruleRuntimeError.MissingRuntimeParameter));

        // Selected calls intentionally do not validate unselected serializers.
        var selected = GeneratedMapping.ExecuteDocumentSelectedTargetWithHost("{\"message\":\"DENY\"}", FerruleTargetSelection.Primary(), Inputs(), Context());
        Require(selected is SelectedDocumentTargetOutput.Primary, "selected API compatibility");
        var mapping = GeneratedMapping.ExecuteOutputsWithSources(
            FerruleJson.ParseEmbedded(GetDescriptor("DocumentSourceDescriptor"), Text("input.json")),
            [new NamedInput("Reference", FerruleX12.ParseEmbedded(GetDescriptors("ExtraDocumentSourceDescriptors")[0], Text("reference.x12"))),
             new NamedInput("Unused", FerruleJson.ParseEmbedded(GetDescriptors("ExtraDocumentSourceDescriptors")[1], Text("unused.json")))], Context());
        var malformed = new ExecutionOutputs(mapping.Primary, [mapping.Extras[2], mapping.Extras[1], mapping.Extras[0]]);
        foreach (var suffix in new[] { "", "Bytes" })
            Refuse("alignment-" + suffix, () => InvokePrivate("SerializeDocument" + suffix + "Outputs", malformed, Context()),
                error => error is DocumentExecutionException { Owner: null, Boundary: InvalidOperationException });
        Require(text.Primary == Json("primary.expected.json") && bytes.Primary.SequenceEqual(Utf8.GetBytes(text.Primary)), "a refused run changed an earlier successful bundle");
    }
#endif

#if PRIMARY
    private static void Primary()
    {
        var text = GeneratedMapping.ExecuteDocumentOutputsWithHost(Text("input.json"), Inputs(), Context());
        Retain("primary-x12-text", text);
        Require(text.PrimaryFormat == DocumentBoundaryFormat.X12 && text.Primary == Text("wire.expected.x12"), "primary 005010 completion");
        Require(text.Extras.Select(output => output.Name).SequenceEqual(new[] { "Zeta", "Wire", "Alpha" }), "swapped schema still owns declared names");
        Require(text.Extras[0].Document == Json("zeta.expected.json") && text.Extras[1].Document == Json("primary.expected.json")
            && text.Extras[1].Format == DocumentBoundaryFormat.Json && text.Extras[2].Document == Json("alpha.expected.json"), "all named documents with primary X12");
        var bytes = GeneratedMapping.ExecuteDocumentBytesOutputsWithHost(Bytes("input.json"), ByteInputs(), Context());
        Retain("primary-x12-bytes", bytes);
        Require(bytes.Primary.SequenceEqual(Bytes("wire.expected.x12")) && bytes.PrimaryFormat == DocumentBoundaryFormat.X12, "primary wire bytes");
        for (var i = 0; i < text.Extras.Count; i++)
            Require(bytes.Extras[i].Name == text.Extras[i].Name && bytes.Extras[i].Format == text.Extras[i].Format
                && bytes.Extras[i].Document.SequenceEqual(Utf8.GetBytes(text.Extras[i].Document)), "primary X12 named byte policy");
        Refuse("primary-completion-refusal", () => GeneratedMapping.ExecuteDocumentOutputsWithHost(Text("input.json"), Inputs(), Context("invalid")),
            error => Owned(error, new DocumentBoundaryOwner.Target(), typeof(FerruleX12Exception)));
        Refuse("late-json-after-primary-wire", () => GeneratedMapping.ExecuteDocumentBytesOutputsWithHost(Utf8.GetBytes("{\"message\":\"DENY\"}"), ByteInputs(), Context()),
            error => Owned(error, new DocumentBoundaryOwner.NamedTarget(2, "Alpha"), typeof(FerruleRuntimeException)));
    }
#endif

#if GROUPED
    private static void Grouped()
    {
        var text = GeneratedMapping.ExecuteDocumentOutputs(Text("grouped.x12"));
        Retain("grouped-text", text);
        Require(text.PrimaryFormat == DocumentBoundaryFormat.Json && text.Primary == Json("grouped.expected.json"), "grouped source real JSON summary");
        Require(text.Extras.Count == 1 && text.Extras[0].Name == "Explicit grouped" && text.Extras[0].Format == DocumentBoundaryFormat.X12
            && text.Extras[0].Document == Text("grouped.expected.x12"), "explicit named grouped profile and both transactions");
        var bytes = GeneratedMapping.ExecuteDocumentBytesOutputs(Bytes("grouped.x12"));
        Retain("grouped-bytes", bytes);
        Require(bytes.Primary.SequenceEqual(Utf8.GetBytes(text.Primary)) && bytes.Extras[0].Document.SequenceEqual(Bytes("grouped.expected.x12")), "grouped exact wire bytes");
        Refuse("grouped-source-owner", () => GeneratedMapping.ExecuteDocumentOutputs(Text("grouped.x12").Replace("GE*2*8~", "GE*1*8~")),
            error => Owned(error, new DocumentBoundaryOwner.Source(), typeof(FerruleX12Exception)));
    }
#endif

#if SINGLE
    private static void Single()
    {
        var text = GeneratedMapping.ExecuteDocumentOutputs(Text("reference.x12"));
        Retain("single-text", text);
        Require(text.PrimaryFormat == DocumentBoundaryFormat.X12 && text.Primary == Text("reference.expected.x12") && text.Extras.Count == 0, "single complete primary wire");
        var bytes = GeneratedMapping.ExecuteDocumentBytesOutputs(Bytes("reference.x12"));
        Retain("single-bytes", bytes);
        Require(bytes.PrimaryFormat == DocumentBoundaryFormat.X12 && bytes.Primary.SequenceEqual(Bytes("reference.expected.x12")) && bytes.Extras.Count == 0, "single primary wire bytes");
        Require(GeneratedMapping.ExecuteDocumentOutputsWithHost(Text("reference.x12"), []).Primary == text.Primary, "empty named host overload");
        Require(GeneratedMapping.ExecuteDocumentBytesOutputsWithHost(Bytes("reference.x12"), []).Primary.SequenceEqual(bytes.Primary), "empty named byte host overload");
    }
#endif

    private static object InvokePrivate(string name, params object?[] args)
    {
        try { return typeof(GeneratedMapping).GetMethod(name, BindingFlags.Static | BindingFlags.NonPublic)!.Invoke(null, args)!; }
        catch (TargetInvocationException error) when (error.InnerException is not null) { throw error.InnerException; }
    }
    private static string GetDescriptor(string name) => (string)typeof(GeneratedMapping).GetField(name, BindingFlags.Static | BindingFlags.NonPublic)!.GetRawConstantValue()!;
    private static string[] GetDescriptors(string name) => (string[])typeof(GeneratedMapping).GetField(name, BindingFlags.Static | BindingFlags.NonPublic)!.GetValue(null)!;

    private static void Budgets()
    {
        var type = typeof(GeneratedMapping).GetNestedType("DocumentBundleBudget", BindingFlags.NonPublic)!;
        object Budget(long count) => Activator.CreateInstance(type, BindingFlags.Instance | BindingFlags.NonPublic, null, [count], null)!;
        var budget = Budget(DocumentExecutionLimits.MaximumDocuments);
        var owner = new DocumentBoundaryOwner.NamedTarget(2, "later");
        var charge = type.GetMethod("Charge", BindingFlags.Instance | BindingFlags.NonPublic)!;
        charge.Invoke(budget, [new DocumentBoundaryOwner.Target(), DocumentExecutionLimits.MaximumTotalUtf8Bytes]);
        Exception? observed = null;
        try { charge.Invoke(budget, [owner, 1L]); }
        catch (TargetInvocationException error) { observed = error.InnerException; }
        File.WriteAllText(Path.Combine(observations, "bundle-budget-failure.txt"), observed?.ToString() ?? "NO FAILURE");
        Require(observed is DocumentExecutionException failure && Equals(failure.Owner, owner)
            && failure.Boundary is DocumentBundleResourceException resource && resource.Resource == "document_bundle_utf8_bytes"
            && resource.Observed == DocumentExecutionLimits.MaximumTotalUtf8Bytes + 1, "exact total UTF-8 counter/owner");
    }

    private sealed class CountedInputs<T>(T[] values) : IReadOnlyList<T>
    {
        public int[] Reads { get; } = new int[values.Length];
        public int Count => values.Length;
        public T this[int index] { get { Reads[index]++; return values[index]; } }
        public IEnumerator<T> GetEnumerator() => throw new Exception("the source envelopes must be snapshotted by index");
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
    }
    private sealed class CountOnlyInputs<T>(int count) : IReadOnlyList<T>
    {
        public int Count => count;
        public T this[int index] => throw new Exception("count budget must precede envelope access");
        public IEnumerator<T> GetEnumerator() => throw new Exception("count budget must precede enumeration");
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
    }
}
