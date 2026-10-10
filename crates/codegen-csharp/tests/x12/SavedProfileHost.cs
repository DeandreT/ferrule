extern alias Strict;
extern alias Lenient;
extern alias Scaled;
extern alias Formatted;
extern alias InactiveTarget;
extern alias Completed;
extern alias FailedMap;
extern alias ConstrainedCompleted;

using System.Collections;
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using StrictMap = Strict::Ferrule.Generated.GeneratedMapping;
using LenientMap = Lenient::Ferrule.Generated.GeneratedMapping;
using ScaledMap = Scaled::Ferrule.Generated.GeneratedMapping;
using FormattedMap = Formatted::Ferrule.Generated.GeneratedMapping;
using InactiveMap = InactiveTarget::Ferrule.Generated.GeneratedMapping;
using CompletedMap = Completed::Ferrule.Generated.GeneratedMapping;
using FailureMap = FailedMap::Ferrule.Generated.GeneratedMapping;
using ConstrainedMap = ConstrainedCompleted::Ferrule.Generated.GeneratedMapping;

string input = File.ReadAllText("input.x12");
string sourceExpected = File.ReadAllText("source.expected.json");
string scaledExpected = File.ReadAllText("scaled.expected.json");
string lexicalInput = File.ReadAllText("lexical-input.json");
string outputExpected = File.ReadAllText("output.expected.x12");
string completedExpected = File.ReadAllText("completed.expected.x12");
string suppliedCompletionExpected = File.ReadAllText("supplied-completion.expected.x12");
var utf8 = new UTF8Encoding(false, true);
int passed = 0, failed = 0;
Directory.CreateDirectory("outcomes");
const string timestamp = "2028-02-29T12:34:56.12+05:30";
var completeContext = new Completed::Ferrule.Runtime.FerruleExecutionContext("authored-profile", "authored-profile", currentDateTime: timestamp);
var noDateContext = new Completed::Ferrule.Runtime.FerruleExecutionContext("authored-profile");
var badDateContext = new Completed::Ferrule.Runtime.FerruleExecutionContext("authored-profile", "authored-profile", currentDateTime: "2028-02-30T12:34:56Z");
var badSuffixContext = new Completed::Ferrule.Runtime.FerruleExecutionContext("authored-profile", "authored-profile", currentDateTime: "2028-02-29T12:34:56INVALID");
var badOffsetContext = new Completed::Ferrule.Runtime.FerruleExecutionContext("authored-profile", "authored-profile", currentDateTime: "2028-02-29T12:34:56+14:30");
var constrainedContext = new ConstrainedCompleted::Ferrule.Runtime.FerruleExecutionContext("authored-profile", "authored-profile", currentDateTime: timestamp);

TypedCase("strict-complete-source", () => StrictMap.ParseX12(input), sourceExpected);
TypedCase("lenient-complete-source", () => LenientMap.ParseX12(input), sourceExpected);
TypedCase("scaled-complete-source", () => ScaledMap.ParseX12(input), scaledExpected);
TypedCase("scaled-byte-source", () => ScaledMap.ParseX12Bytes(utf8.GetBytes(input)), scaledExpected);
JsonCase("lenient-json", () => LenientMap.ExecuteX12ToJson(input), Projection(sourceExpected));
JsonCase("scaled-json", () => ScaledMap.ExecuteX12ToJson(input), Projection(scaledExpected));
JsonCase("scaled-json-bytes", () => utf8.GetString(ScaledMap.ExecuteX12ToJsonBytes(utf8.GetBytes(input))), Projection(scaledExpected));
JsonCase("scaled-json-context", () => ScaledMap.ExecuteX12ToJson(input, new Scaled::Ferrule.Runtime.FerruleExecutionContext("authored-profile")), Projection(scaledExpected));

string topUnknown = input.Replace("ST*940*0101~\n", "ST*940*0101~\nZZZ*UNDECLARED~\n", StringComparison.Ordinal).Replace("SE*13*", "SE*14*", StringComparison.Ordinal);
string detailUnknown = input.Replace("LX*1~\n", "LX*1~\nZZZ*UNDECLARED~\n", StringComparison.Ordinal).Replace("SE*13*", "SE*14*", StringComparison.Ordinal);
var withUnknown = JsonNode.Parse(sourceExpected)!;
withUnknown["SE"]!["SE01"] = "14";
TypedCase("unknown-top-full", () => LenientMap.ParseX12(topUnknown), withUnknown.ToJsonString());
TypedCase("unknown-detail-full", () => LenientMap.ParseX12(detailUnknown), withUnknown.ToJsonString());
ExactError("strict-unknown-top", () => StrictMap.ParseX12(topUnknown), "Schema", "X12 segment or fixed qualifier does not match the schema.", 3, null);
ExactError("strict-unknown-detail", () => StrictMap.ParseX12(detailUnknown), "Schema", "X12 segment or fixed qualifier does not match the schema.", 9, null);
ExactError("lenient-declared-extra-element", () => LenientMap.ParseX12(input.Replace("W01*1250**EA", "W01*1250**EA*EXTRA", StringComparison.Ordinal)), "Schema", "X12 segment contains undeclared elements.", 9, null);
ExactError("lenient-declared-wrong-qualifier", () => LenientMap.ParseX12(input.Replace("N9*OP*", "N9*XX*", StringComparison.Ordinal)), "Schema", "X12 segment or fixed qualifier does not match the schema.", 6, null);
ExactError("lenient-required-wrong-qualifier", () => LenientMap.ParseX12(input.Replace("N9*LI*REF-A", "N9*XX*REF-A", StringComparison.Ordinal)), "Schema", "X12 segment or fixed qualifier does not match the schema.", 7, null);
ExactError("lenient-later-loop-wrong-qualifier", () => LenientMap.ParseX12(input.Replace("N9*LI*REF-B", "N9*XX*REF-B", StringComparison.Ordinal)), "Schema", "X12 segment or fixed qualifier does not match the schema.", 10, null);
ExactError("lenient-declared-misplaced-body", () => LenientMap.ParseX12(input.Replace("N9*OP*PREFIX-A", "W01*1250**EA", StringComparison.Ordinal)), "Schema", "X12 segment or fixed qualifier does not match the schema.", 6, null);
ExactError("lenient-declared-invalid-number", () => LenientMap.ParseX12(input.Replace("W01*1250", "W01*NaN", StringComparison.Ordinal)), "Value", "X12 scalar has an invalid numeric representation.", null, "Detail/W01/W0101");
ExactError("lenient-declared-number-overflow", () => ScaledMap.ParseX12(input.Replace("W01*1250", "W01*1e309", StringComparison.Ordinal)), "Value", "X12 scalar has an invalid numeric representation.", null, "Detail/W01/W0101");
string nextIteration = input.Replace("W01*1250**EA~\n", "", StringComparison.Ordinal).Replace("SE*13*", "SE*12*", StringComparison.Ordinal);
string nextSummary = input.Replace("W01*1250**EA~\nN9*LI*REF-B~\nLX*2~\nW01*-500*0*EA~\n", "", StringComparison.Ordinal).Replace("SE*13*", "SE*9*", StringComparison.Ordinal);
ExactError("protect-next-iteration", () => LenientMap.ParseX12(nextIteration), "Schema", "X12 segment or fixed qualifier does not match the schema.", 9, null);
ExactError("protect-upcoming-summary", () => LenientMap.ParseX12(nextSummary), "Schema", "X12 segment or fixed qualifier does not match the schema.", 9, null);
string missingQualified = input.Replace("N9*LI*REF-A~\n", "", StringComparison.Ordinal).Replace("SE*13*", "SE*12*", StringComparison.Ordinal);
ExactError("protect-qualified-sibling", () => LenientMap.ParseX12(missingQualified), "Schema", "X12 segment or fixed qualifier does not match the schema.", 7, null);
string optionalAbsent = input.Replace("N9*OP*PREFIX-A~\n", "", StringComparison.Ordinal).Replace("SE*13*", "SE*12*", StringComparison.Ordinal);
var absentExpected = JsonNode.Parse(sourceExpected)!;
absentExpected["Detail"]![0]!["Prefix"] = new JsonArray();
absentExpected["SE"]!["SE01"] = "12";
TypedCase("optional-prefix-absent-strict", () => StrictMap.ParseX12(optionalAbsent), absentExpected.ToJsonString());
TypedCase("optional-prefix-absent-lenient", () => LenientMap.ParseX12(optionalAbsent), absentExpected.ToJsonString());
string tooMany = input[..input.IndexOf("GS*", StringComparison.Ordinal)] + string.Concat(Enumerable.Repeat("ZZZ*UNDECLARED~\n", 100_001));
ExactError("skipped-input-budget", () => LenientMap.ParseX12(tooMany), "ResourceLimit", "X12 segment limit exceeded.", null, null);
ExactError("lenient-wrong-raw-count", () => LenientMap.ParseX12(topUnknown.Replace("SE*14*", "SE*13*", StringComparison.Ordinal)), "Envelope", "X12 trailer counts or controls do not match the interchange.", null, null);
string scaledDescriptor = Descriptor(typeof(ScaledMap), "X12SourceDescriptor");
string EditedDescriptor(Action<JsonObject> edit)
{
    var descriptor = JsonNode.Parse(scaledDescriptor)!.AsObject();
    edit(descriptor);
    return descriptor.ToJsonString();
}
void DescriptorError(string name, Action<JsonObject> edit, string message) => ExactError(name, () => Scaled::Ferrule.Runtime.FerruleX12.ParseEmbedded(EditedDescriptor(edit), "INVALID"), "Schema", message, null, null);
DescriptorError("duplicate-implied-path", descriptor => { var array = descriptor["implied_decimals"]!.AsArray(); array.Add(array[0]!.DeepClone()); }, "Invalid or duplicate X12 implied-decimal metadata.");
DescriptorError("implied-nonfloat-path", descriptor => descriptor["implied_decimals"]![0]!["path"] = new JsonArray("W05", "W0501"), "Invalid or duplicate X12 implied-decimal metadata.");
DescriptorError("implied-places-plus-one", descriptor => descriptor["implied_decimals"]![0]!["places"] = 19, "Invalid or duplicate X12 implied-decimal metadata.");
DescriptorError("missing-implied-path", descriptor => descriptor["implied_decimals"]![0]!["path"] = new JsonArray("Missing"), "X12 profile option path does not resolve.");
DescriptorError("nonleaf-implied-path", descriptor => descriptor["implied_decimals"]![0]!["path"] = new JsonArray("Detail"), "X12 profile options require scalar leaves.");
DescriptorError("invalid-lenient-descriptor-type", descriptor => descriptor["lenient_segments"] = "true", "Invalid embedded X12 descriptor.");
DescriptorError("oversized-option-array", descriptor => descriptor["implied_decimals"] = new JsonArray(Enumerable.Range(0, 10_001).Select(_ => descriptor["implied_decimals"]![0]!.DeepClone()).ToArray()), "X12 profile options require bounded arrays.");
var collidingRepetition = EditedDescriptor(descriptor => descriptor["separators"]!["repetition"] = "*");
ExactError("inactive-repetition-collision", () => Scaled::Ferrule.Runtime.FerruleX12.ParseEmbedded(collidingRepetition, "INVALID"), "Syntax", "Inactive X12 repetition metadata must be distinct ASCII punctuation.", null, null);

TextCase("formatted-full-wire", () => FormattedMap.ExecuteJsonToX12(lexicalInput), outputExpected);
TextCase("formatted-byte-wire", () => utf8.GetString(FormattedMap.ExecuteJsonToX12Bytes(utf8.GetBytes(lexicalInput))), outputExpected);
TextCase("inactive-target-reader-options", () => InactiveMap.ExecuteJsonToX12(lexicalInput), outputExpected);
var suppliedCompletion = JsonNode.Parse(lexicalInput)!;
suppliedCompletion["GS"]!["GS05"] = "12:34:56+05:30";
string suppliedCompletionInput = suppliedCompletion.ToJsonString();
TextCase("completion-preserves-supplied-values", () => CompletedMap.ExecuteJsonToX12(suppliedCompletionInput, completeContext), suppliedCompletionExpected);
TextCase("completion-preserves-supplied-bytes", () => utf8.GetString(CompletedMap.ExecuteJsonToX12Bytes(utf8.GetBytes(suppliedCompletionInput), completeContext)), suppliedCompletionExpected);
ExactError("completion-header-time-width", () => CompletedMap.ExecuteJsonToX12(lexicalInput, completeContext), "Envelope", "X12 completion requires valid header dates and times.", null, null);
ExactError("completion-missing-context", () => CompletedMap.ExecuteJsonToX12(lexicalInput), "Envelope", "X12 completion requires a supplied current dateTime.", null, null);
ExactError("completion-missing-date", () => CompletedMap.ExecuteJsonToX12(lexicalInput, noDateContext), "Envelope", "X12 completion requires a supplied current dateTime.", null, null);
ExactError("completion-invalid-date", () => CompletedMap.ExecuteJsonToX12(lexicalInput, badDateContext), "Envelope", "X12 completion requires a valid supplied current dateTime.", null, null);
ExactError("completion-invalid-suffix", () => CompletedMap.ExecuteJsonToX12(suppliedCompletionInput, badSuffixContext), "Envelope", "X12 completion requires a valid supplied current dateTime.", null, null);
ExactError("completion-invalid-offset", () => CompletedMap.ExecuteJsonToX12(suppliedCompletionInput, badOffsetContext), "Envelope", "X12 completion requires a valid supplied current dateTime.", null, null);
var completedInput = JsonNode.Parse(lexicalInput)!;
foreach (string field in new[] { "ISA01", "ISA02", "ISA03", "ISA04", "ISA05", "ISA07", "ISA09", "ISA10", "ISA13", "ISA14", "ISA15" }) completedInput["ISA"]![field] = "";
foreach (string field in new[] { "GS04", "GS05", "GS06" }) completedInput["GS"]![field] = "";
foreach (string field in new[] { "ISA09", "ISA10" }) completedInput["ISA"]!.AsObject().Remove(field);
foreach (string field in new[] { "GS04", "GS05" }) completedInput["GS"]!.AsObject().Remove(field);
completedInput["ST"]!["ST02"] = "";
foreach (string segment in new[] { "SE", "GE", "IEA" }) foreach (var field in completedInput[segment]!.AsObject().ToArray()) completedInput[segment]![field.Key] = "";
string emptyInput = completedInput.ToJsonString();
TextCase("completion-empty-fields", () => CompletedMap.ExecuteJsonToX12(emptyInput, completeContext), completedExpected);
TextCase("completion-empty-byte-fields", () => utf8.GetString(CompletedMap.ExecuteJsonToX12Bytes(utf8.GetBytes(emptyInput), completeContext)), completedExpected);
TextCase("completion-repeatability", () => CompletedMap.ExecuteJsonToX12(emptyInput, completeContext), completedExpected);
TextCase("constraints-after-completion", () => ConstrainedMap.ExecuteJsonToX12(emptyInput, constrainedContext), completedExpected);
TextCase("constraints-after-formatting", () => ConstrainedMap.ExecuteJsonToX12(suppliedCompletionInput, constrainedContext), suppliedCompletionExpected);
var badConstraint = JsonNode.Parse(emptyInput)!;
badConstraint["W05"]!["W0503"] = "OTHER";
ExactError("final-constraint-failure", () => ConstrainedMap.ExecuteJsonToX12(badConstraint.ToJsonString(), constrainedContext), "Value", "X12 lexical length or code-list constraint failed.", null, "W05/W0503");
ExactError("context-before-final-constraint", () => ConstrainedMap.ExecuteJsonToX12(badConstraint.ToJsonString()), "Envelope", "X12 completion requires a supplied current dateTime.", null, null);
var wrongCounts = JsonNode.Parse(suppliedCompletionInput)!;
wrongCounts["SE"]!["SE01"] = "12";
ExactError("completion-preserves-wrong-count", () => CompletedMap.ExecuteJsonToX12(wrongCounts.ToJsonString(), completeContext), "Envelope", "X12 trailer counts or controls do not match the interchange.", null, null);
var missingSender = JsonNode.Parse(emptyInput)!;
missingSender["ISA"]!["ISA06"] = "";
ExactError("completion-no-sender-fabrication", () => CompletedMap.ExecuteJsonToX12(missingSender.ToJsonString(), completeContext), "Envelope", "X12 requires sender, recipient, transaction and functional group identities.", null, null);
var invalidDate = JsonNode.Parse(lexicalInput)!;
invalidDate["DTM"]!["DTM01"] = "2028-02-30";
ExactError("invalid-date", () => FormattedMap.ExecuteJsonToX12(invalidDate.ToJsonString()), "Value", "X12 lexical value cannot be formatted.", null, "DTM/DTM01");
ExactError("lexical-before-context", () => CompletedMap.ExecuteJsonToX12(invalidDate.ToJsonString()), "Value", "X12 lexical value cannot be formatted.", null, "DTM/DTM01");
var precision = JsonNode.Parse(lexicalInput)!;
precision["DTM"]!["DTM02"] = "12:34:56.123";
ExactError("nonzero-time-precision", () => FormattedMap.ExecuteJsonToX12(precision.ToJsonString()), "Value", "X12 lexical value cannot be formatted.", null, "DTM/DTM02");
var shortTime = JsonNode.Parse(lexicalInput)!;
shortTime["DTM"]!["DTM02"] = "12:34";
TextCase("iso-short-time", () => FormattedMap.ExecuteJsonToX12(shortTime.ToJsonString()), outputExpected.Replace("DTM*20280229*12345612", "DTM*20280229*123400", StringComparison.Ordinal));
var narrowTimeDescriptor = JsonNode.Parse(Descriptor(typeof(FormattedMap), "X12TargetDescriptor"))!.AsObject();
var narrowTimeFormat = narrowTimeDescriptor["lexical_formats"]!.AsArray().Single(item =>
    item!["path"]![0]!.GetValue<string>() == "DTM" && item["path"]![1]!.GetValue<string>() == "DTM02")!;
narrowTimeFormat["kind"]!["compact_time"]!["max_digits"] = 5;
Formatted::Ferrule.Runtime.FerruleInstance NarrowTimeInput() => FormattedMap.Execute(
    Formatted::Ferrule.Runtime.FerruleJson.ParseEmbedded(Descriptor(typeof(FormattedMap), "SourceJsonSchema"), shortTime.ToJsonString()));
ExactError("time-conversion-exceeds-declared-width", () => Formatted::Ferrule.Runtime.FerruleX12.SerializeEmbedded(
    narrowTimeDescriptor.ToJsonString(), NarrowTimeInput()), "Value", "X12 lexical value cannot be formatted.", null, "DTM/DTM02");
shortTime["DTM"]!["DTM02"] = "1234";
TextCase("compact-four-digit-time", () => FormattedMap.ExecuteJsonToX12(shortTime.ToJsonString()), outputExpected.Replace("DTM*20280229*12345612", "DTM*20280229*1234", StringComparison.Ordinal));
TextCase("compact-time-within-five-digit-ceiling", () => Formatted::Ferrule.Runtime.FerruleX12.SerializeEmbedded(
    narrowTimeDescriptor.ToJsonString(), NarrowTimeInput()), outputExpected.Replace("DTM*20280229*12345612", "DTM*20280229*1234", StringComparison.Ordinal));
shortTime["ISA"]!["ISA10"] = "12:34:01";
ExactError("nonzero-seconds-outside-width", () => FormattedMap.ExecuteJsonToX12(shortTime.ToJsonString()), "Value", "X12 lexical value cannot be formatted.", null, "ISA/ISA10");
var exponent = JsonNode.Parse(lexicalInput)!;
exponent["AMT"]!["AMT01"] = "1e2";
ExactError("decimal-exponent-string", () => FormattedMap.ExecuteJsonToX12(exponent.ToJsonString()), "Value", "X12 lexical value cannot be formatted.", null, "AMT/AMT01");
var maxDecimal = JsonNode.Parse(lexicalInput)!;
maxDecimal["AMT"]!["AMT01"] = "12345678";
TextCase("decimal-exact-width", () => FormattedMap.ExecuteJsonToX12(maxDecimal.ToJsonString()), outputExpected.Replace("AMT*00012.50", "AMT*12345678", StringComparison.Ordinal));
maxDecimal["AMT"]!["AMT01"] = "123456789";
ExactError("decimal-width-plus-one", () => FormattedMap.ExecuteJsonToX12(maxDecimal.ToJsonString()), "Value", "X12 lexical value cannot be formatted.", null, "AMT/AMT01");
var sign = JsonNode.Parse(lexicalInput)!;
sign["AMT"]!["AMT01"] = "-0000.00";
TextCase("explicit-signed-zero-spelling", () => FormattedMap.ExecuteJsonToX12(sign.ToJsonString()), outputExpected.Replace("AMT*00012.50", "AMT*-0000.00", StringComparison.Ordinal));
var floating = JsonNode.Parse(lexicalInput)!;
floating["W76"]!["W7601"] = 0.30000000000000004;
TextCase("bounded-float-rounding", () => FormattedMap.ExecuteJsonToX12(floating.ToJsonString()), outputExpected.Replace("W76*7.5", "W76*0.3", StringComparison.Ordinal));
floating["W76"]!["W7601"] = -0.0;
TextCase("float-negative-zero", () => FormattedMap.ExecuteJsonToX12(floating.ToJsonString()), outputExpected.Replace("W76*7.5", "W76*-0", StringComparison.Ordinal));
floating["W76"]!["W7601"] = 12345678901.0;
ExactError("float-integer-width", () => FormattedMap.ExecuteJsonToX12(floating.ToJsonString()), "Value", "X12 lexical value cannot be formatted.", null, "W76/W7601");
var emptyDate = JsonNode.Parse(emptyInput)!;
emptyDate["ISA"]!["ISA09"] = "";
ExactError("empty-string-date-is-lexical-error", () => CompletedMap.ExecuteJsonToX12(emptyDate.ToJsonString(), completeContext), "Value", "X12 lexical value cannot be formatted.", null, "ISA/ISA09");
MappingError("mapping-before-context", () => FailureMap.ExecuteJsonToX12(lexicalInput));

// Direct typed APIs also prove the complete caller tree is unchanged.
string completedSchema = Descriptor(typeof(CompletedMap), "SourceJsonSchema");
var sourceInstance = Completed::Ferrule.Runtime.FerruleJson.ParseEmbedded(completedSchema, suppliedCompletionInput);
var targetInstance = CompletedMap.Execute(sourceInstance);
string targetBefore = RenderTyped(targetInstance)!.ToJsonString();
TextCase("contextual-direct-serializer", () => CompletedMap.SerializeX12(targetInstance, completeContext), suppliedCompletionExpected);
TextCase("contextual-direct-byte-serializer", () => utf8.GetString(CompletedMap.SerializeX12Bytes(targetInstance, completeContext)), suppliedCompletionExpected);
JsonCase("caller-instance-preserved", () => RenderTyped(targetInstance)!.ToJsonString(), targetBefore);
var withoutTrailers = new Completed::Ferrule.Runtime.FerruleGroup(((Completed::Ferrule.Runtime.FerruleGroup)targetInstance).Fields.Where(field => field.Name is not ("SE" or "GE" or "IEA")));
TextCase("missing-trailers-materialized", () => CompletedMap.SerializeX12(withoutTrailers, completeContext), suppliedCompletionExpected);

// Completion-only admission must bound retained envelope slots before cloning them.
var completionOnlyDescriptor = JsonNode.Parse(Descriptor(typeof(CompletedMap), "X12TargetDescriptor"))!.AsObject();
completionOnlyDescriptor["lexical_formats"] = new JsonArray();
foreach (string envelopeName in new[] { "ISA", "SE" })
{
    var rootGroup = (Completed::Ferrule.Runtime.FerruleGroup)targetInstance;
    rootGroup.TryGetField(envelopeName, out var envelopeInstance);
    var envelopeGroup = (Completed::Ferrule.Runtime.FerruleGroup)envelopeInstance!;
    var retainedNull = new Completed::Ferrule.Runtime.FerruleScalar(Completed::Ferrule.Runtime.FerruleValue.Null);
    var oversizedEnvelope = new Completed::Ferrule.Runtime.FerruleGroup(envelopeGroup.Fields.Concat(
        Enumerable.Range(0, Completed::Ferrule.Runtime.FerruleX12.MaximumNodes).Select(index =>
            new Completed::Ferrule.Runtime.FerruleField("UNDECLARED" + index, retainedNull))));
    var oversizedRoot = new Completed::Ferrule.Runtime.FerruleGroup(rootGroup.Fields.Select(field =>
        field.Name == envelopeName ? new Completed::Ferrule.Runtime.FerruleField(field.Name, oversizedEnvelope) : field));
    ExactError("completion-retained-" + envelopeName + "-slot-budget", () =>
        Completed::Ferrule.Runtime.FerruleX12.SerializeEmbedded(completionOnlyDescriptor.ToJsonString(), oversizedRoot, completeContext),
        "ResourceLimit", "X12 traversal limit exceeded.", null, null);
    JsonCase("completion-retained-" + envelopeName + "-caller-preserved", () => JsonSerializer.Serialize(new
    {
        fields = oversizedEnvelope.Fields.Count,
        referencesPreserved = oversizedEnvelope.Fields.Skip(envelopeGroup.Fields.Count).All(field => ReferenceEquals(field.Value, retainedNull))
    }), JsonSerializer.Serialize(new { fields = envelopeGroup.Fields.Count + Completed::Ferrule.Runtime.FerruleX12.MaximumNodes, referencesPreserved = true }));
}

File.WriteAllText("outcomes/summary.json", JsonSerializer.Serialize(new { passed, failed }));
Console.WriteLine(JsonSerializer.Serialize(new { passed, failed }));
if (failed != 0) Environment.ExitCode = 1;

string Descriptor(Type type, string name) => (string)type.GetField(name, BindingFlags.NonPublic | BindingFlags.Static)!.GetRawConstantValue()!;
string Projection(string expected)
{
    var node = JsonNode.Parse(expected)!;
    Prune(node);
    return node.ToJsonString();
    void Prune(JsonNode value)
    {
        if (value is JsonObject obj) foreach (var member in obj.ToArray()) { if (member.Value is null) obj.Remove(member.Key); else Prune(member.Value); }
        else if (value is JsonArray array) foreach (var member in array) if (member is not null) Prune(member);
    }
}
JsonNode? RenderTyped(object value)
{
    Type type = value.GetType();
    if (type.Name == "FerruleScalar")
    {
        object scalar = type.GetProperty("Value")!.GetValue(value)!;
        string kind = scalar.GetType().GetProperty("Kind")!.GetValue(scalar)!.ToString()!;
        string? property = kind switch { "String" => "StringValue", "Double" => "DoubleValue", "Int64" => "Int64Value", "Bool" => "BooleanValue", "Null" or "JsonNull" or "XmlNil" => null, _ => throw new InvalidOperationException("Unknown scalar kind") };
        return property is null ? null : JsonSerializer.SerializeToNode(scalar.GetType().GetProperty(property)!.GetValue(scalar));
    }
    if (type.Name == "FerruleGroup")
    {
        var result = new JsonObject();
        foreach (object field in (IEnumerable)type.GetProperty("Fields")!.GetValue(value)!) result.Add((string)field.GetType().GetProperty("Name")!.GetValue(field)!, RenderTyped(field.GetType().GetProperty("Value")!.GetValue(field)!));
        return result;
    }
    if (type.Name is "FerruleRepeated" or "FerruleMappedSequence")
    {
        var result = new JsonArray();
        foreach (object item in (IEnumerable)type.GetProperty("Items")!.GetValue(value)!) result.Add(RenderTyped(item));
        return result;
    }
    throw new InvalidOperationException("Unexpected instance shape");
}
JsonNode KindTree(object value)
{
    Type type = value.GetType();
    if (type.Name == "FerruleScalar")
    {
        object scalar = type.GetProperty("Value")!.GetValue(value)!;
        return JsonValue.Create(scalar.GetType().GetProperty("Kind")!.GetValue(scalar)!.ToString())!;
    }
    if (type.Name == "FerruleGroup")
    {
        var result = new JsonObject();
        foreach (object field in (IEnumerable)type.GetProperty("Fields")!.GetValue(value)!) result.Add((string)field.GetType().GetProperty("Name")!.GetValue(field)!, KindTree(field.GetType().GetProperty("Value")!.GetValue(field)!));
        return result;
    }
    var items = new JsonArray();
    foreach (object item in (IEnumerable)type.GetProperty("Items")!.GetValue(value)!) items.Add(KindTree(item));
    return items;
}
JsonNode ExpectedKinds(JsonNode? value)
{
    if (value is JsonObject fields) { var result = new JsonObject(); foreach (var field in fields) result.Add(field.Key, ExpectedKinds(field.Value)); return result; }
    if (value is JsonArray array) { var result = new JsonArray(); foreach (var item in array) result.Add(ExpectedKinds(item)); return result; }
    string kind = value is null ? "Null" : value.GetValueKind() switch { JsonValueKind.String => "String", JsonValueKind.Number => "Double", _ => throw new InvalidOperationException("Authored scalar oracle type") };
    return JsonValue.Create(kind)!;
}
void TypedCase(string name, Func<object> run, string expected)
{
    object? instance = null;
    JsonCase(name, () => { instance = run(); return RenderTyped(instance)!.ToJsonString(); }, expected);
    if (instance is not null) JsonCase(name + "-scalar-kinds", () => KindTree(instance).ToJsonString(), ExpectedKinds(JsonNode.Parse(expected)).ToJsonString());
}
void JsonCase(string name, Func<string> run, string expected) => ResultCase(name, run, expected, (actual, oracle) => JsonNode.DeepEquals(JsonNode.Parse(actual), JsonNode.Parse(oracle)));
void TextCase(string name, Func<string> run, string expected) => ResultCase(name, run, expected, (actual, oracle) => utf8.GetBytes(actual).AsSpan().SequenceEqual(utf8.GetBytes(oracle)));
void ResultCase(string name, Func<string> run, string expected, Func<string, string, bool> compare)
{
    string? actual = null;
    Exception? error = null;
    try { actual = run(); } catch (Exception caught) { error = caught; }
    if (actual is not null) File.WriteAllBytes($"outcomes/{name}.actual.bin", utf8.GetBytes(actual));
    File.WriteAllBytes($"outcomes/{name}.expected.bin", utf8.GetBytes(expected));
    File.WriteAllText($"outcomes/{name}.result.json", JsonSerializer.Serialize(new { exception = error?.GetType().FullName, message = error?.Message, returned = actual is not null }));
    bool success = error is null && actual is not null && compare(actual, expected);
    if (success) passed++; else { failed++; Console.Error.WriteLine($"{name}: complete original result retained"); }
}
void ExactError(string name, Action run, string category, string message, int? index, string? path)
{
    Exception? error = null;
    try { run(); } catch (Exception caught) { error = caught; }
    var expected = new { type = "Ferrule.Runtime.FerruleX12Exception", category, message, index, path };
    var actual = new { type = error?.GetType().FullName, category = error?.GetType().GetProperty("Error")?.GetValue(error)?.ToString(), message = error?.Message, index = error?.GetType().GetProperty("SegmentIndex")?.GetValue(error), path = error?.GetType().GetProperty("FieldPath")?.GetValue(error) };
    File.WriteAllText($"outcomes/{name}.expected.json", JsonSerializer.Serialize(expected));
    File.WriteAllText($"outcomes/{name}.actual.json", JsonSerializer.Serialize(actual));
    bool success = error is not null && JsonNode.DeepEquals(JsonSerializer.SerializeToNode(actual), JsonSerializer.SerializeToNode(expected));
    if (success) passed++; else { failed++; Console.Error.WriteLine($"{name}: complete original error retained"); }
}
void MappingError(string name, Action run)
{
    Exception? error = null;
    try { run(); } catch (Exception caught) { error = caught; }
    var expected = new { type = "Ferrule.Runtime.FerruleRuntimeException", category = "MappingFailure", message = "mapping failure rule 1: AUTHORED-FAILURE", rule = 1, mappingMessage = "AUTHORED-FAILURE" };
    var actual = new { type = error?.GetType().FullName, category = error?.GetType().GetProperty("Error")?.GetValue(error)?.ToString(), message = error?.Message, rule = error?.GetType().GetProperty("FailureRule")?.GetValue(error), mappingMessage = error?.GetType().GetProperty("MappingFailureMessage")?.GetValue(error) };
    File.WriteAllText($"outcomes/{name}.expected.json", JsonSerializer.Serialize(expected));
    File.WriteAllText($"outcomes/{name}.actual.json", JsonSerializer.Serialize(actual));
    if (error is not null && JsonNode.DeepEquals(JsonSerializer.SerializeToNode(actual), JsonSerializer.SerializeToNode(expected))) passed++; else { failed++; Console.Error.WriteLine($"{name}: complete original error retained"); }
}
