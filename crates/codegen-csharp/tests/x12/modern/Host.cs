using System.Collections;
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

var utf8 = new UTF8Encoding(false, true);
JsonNode contract = JsonNode.Parse(File.ReadAllText("contract.json"))!;
var outcomes = new JsonArray();
int passed = 0, failed = 0;
try
{
foreach (JsonNode? entry in JsonNode.Parse(File.ReadAllText("libraries.json"))!.AsArray())
{
    string name = entry!["name"]!.GetValue<string>(), version = entry["version"]!.GetValue<string>();
    string direction = entry["direction"]!.GetValue<string>();
    string form = direction == "Custom" ? "custom" : direction == "Lf" ? "lf" : "default";
    string wire = File.ReadAllText($"{version}-{form}.x12");
    JsonNode values = JsonNode.Parse(File.ReadAllText($"{version}-{form}-values.json"))!;
    JsonNode schema = JsonNode.Parse(File.ReadAllText($"{version}-schema.json"))!;
    JsonNode typedExpected = Expected(schema, values);
    string json = Projection(values).ToJsonString();
    Assembly assembly = Assembly.LoadFrom(Path.GetFullPath($"{name}/bin/Release/net10.0/{name}.dll"));
    Type map = assembly.GetType("Ferrule.Generated.GeneratedMapping", throwOnError: true)!;
    Type runtime = assembly.GetType("Ferrule.Runtime.FerruleX12", throwOnError: true)!;
    string sourceDescriptor = File.ReadAllText($"{name}-source-descriptor.json");
    string targetDescriptor = File.ReadAllText($"{name}-target-descriptor.json");
    object Context(string? date = null) => assembly.GetType("Ferrule.Runtime.FerruleExecutionContext")!
        .GetConstructors().Single(ctor => ctor.GetParameters().Length == 4)
        .Invoke(["invented-profile", "invented-profile", date, null]);
    bool source = direction != "Target", target = direction != "Source" && direction != "Lenient";
    if (source)
    {
        ValueCase($"{name}-typed-text", () => Raw(Call(map, "ParseX12", wire)), typedExpected);
        ValueCase($"{name}-typed-bytes", () => Raw(Call(map, "ParseX12Bytes", utf8.GetBytes(wire))), typedExpected);
        if (!target)
        {
            ValueCase($"{name}-json", () => JsonNode.Parse((string)Call(map, "ExecuteX12ToJson", wire)!), Projection(values));
            ValueCase($"{name}-json-bytes", () => JsonNode.Parse(utf8.GetString((byte[])Call(map, "ExecuteX12ToJsonBytes", utf8.GetBytes(wire))!)), Projection(values));
            ValueCase($"{name}-json-context", () => JsonNode.Parse((string)Call(map, "ExecuteX12ToJson", wire, Context())!), Projection(values));
        }
    }
    if (target)
    {
        object parsed = Call(runtime, "ParseEmbedded", targetDescriptor, wire)!;
        ValueCase($"{name}-serialize", () => JsonValue.Create((string)Call(map, "SerializeX12", parsed)!), JsonValue.Create(wire));
        ValueCase($"{name}-serialize-bytes", () => JsonValue.Create(utf8.GetString((byte[])Call(map, "SerializeX12Bytes", parsed)!)), JsonValue.Create(wire));
        ValueCase($"{name}-serialize-context", () => JsonValue.Create((string)Call(map, "SerializeX12", parsed, Context())!), JsonValue.Create(wire));
        if (!source)
        {
            ValueCase($"{name}-json-to-wire", () => JsonValue.Create((string)Call(map, "ExecuteJsonToX12", json)!), JsonValue.Create(wire));
            ValueCase($"{name}-json-to-wire-bytes", () => JsonValue.Create(utf8.GetString((byte[])Call(map, "ExecuteJsonToX12Bytes", utf8.GetBytes(json))!)), JsonValue.Create(wire));
            ValueCase($"{name}-json-to-wire-context", () => JsonValue.Create((string)Call(map, "ExecuteJsonToX12", json, Context())!), JsonValue.Create(wire));
        }
        else
        {
            ValueCase($"{name}-wire-to-wire", () => JsonValue.Create((string)Call(map, "ExecuteX12ToX12", wire)!), JsonValue.Create(wire));
            ValueCase($"{name}-wire-to-wire-bytes", () => JsonValue.Create(utf8.GetString((byte[])Call(map, "ExecuteX12ToX12Bytes", utf8.GetBytes(wire))!)), JsonValue.Create(wire));
            ValueCase($"{name}-wire-to-wire-context", () => JsonValue.Create((string)Call(map, "ExecuteX12ToX12", wire, Context())!), JsonValue.Create(wire));
        }
        ValueCase($"{name}-caller-preserved", () => Raw(parsed), typedExpected);
    }
    if (version == "004010")
    {
        ExactError($"{name}-legacy-isa11", () => Call(map, "ParseX12", wire.Replace("*U*", "*^*", StringComparison.Ordinal)), "legacy_version_wire");
        JsonNode descriptor = JsonNode.Parse(sourceDescriptor)!;
        descriptor["separators"]!["repetition"] = "xx";
        ExactError($"{name}-legacy-repetition-metadata", () => Call(runtime, "ParseEmbedded", descriptor.ToJsonString(), wire), "legacy_rep_width");
        continue;
    }
    if (direction != "Both" && direction != "Lenient") continue;
    string errorWire = File.ReadAllText($"{version}-default.x12");
    ExactError($"{name}-wrong-isa-version", () => Call(map, "ParseX12", errorWire.Replace($"*{version[..5]}*", "*00401*", StringComparison.Ordinal)), "version_wire");
    ExactError($"{name}-wrong-group-version", () => Call(map, "ParseX12", errorWire.Replace($"*{version}~", "*004010~", StringComparison.Ordinal)), "version_wire");
    ExactError($"{name}-repeated-leaf", () => Call(map, "ParseX12", errorWire.Replace("ORDER-00073", "A^B", StringComparison.Ordinal)), "repetition_wire");
    string unknown = errorWire.Replace("NTE*", "ZZZ*A^B~\nNTE*", StringComparison.Ordinal).Replace("SE*5*", "SE*6*", StringComparison.Ordinal);
    ExactError($"{name}-unknown-repeated", () => Call(map, "ParseX12", unknown), "repetition_unknown_wire");
    ExactError($"{name}-bad-isa11", () => Call(map, "ParseX12", errorWire.Replace("*^*", "*U*", StringComparison.Ordinal)), "bad_isa11");
    ExactError($"{name}-colliding-isa11", () => Call(map, "ParseX12", errorWire.Replace("*^*", "*:*", StringComparison.Ordinal)), "bad_isa11");
    ExactError($"{name}-st03", () => Call(map, "ParseX12", errorWire.Replace("ST*940*0031~", "ST*940*0031*EXTRA~", StringComparison.Ordinal)), "st03");
    ExactError($"{name}-wrong-controls", () => Call(map, "ParseX12", errorWire.Replace("GE*1*00321", "GE*1*00322", StringComparison.Ordinal)), "controls");
    JsonNode configured = JsonNode.Parse(sourceDescriptor)!;
    configured["separators"] = new JsonObject { ["element"] = "*", ["component"] = ":", ["segment"] = "~", ["repetition"] = "+" };
    ExactError($"{name}-configured-repetition", () => Call(runtime, "ParseEmbedded", configured.ToJsonString(), errorWire), "configured_syntax");
    configured["separators"]!["repetition"] = null;
    ValueCase($"{name}-unconstrained-configured-repetition", () => Raw(Call(runtime, "ParseEmbedded", configured.ToJsonString(), errorWire)), Expected(schema, JsonNode.Parse(File.ReadAllText($"{version}-default-values.json"))!));
    JsonNode mismatched = JsonNode.Parse(sourceDescriptor)!;
    mismatched["version"] = version == "005010" ? "006040" : "005010";
    ExactError($"{name}-descriptor-schema-version", () => Call(runtime, "ParseEmbedded", mismatched.ToJsonString(), errorWire), "descriptor_schema_version");
    JsonNode malformed = configured.DeepClone();
    malformed["separators"]!["repetition"] = "ab";
    ExactError($"{name}-repetition-metadata-width", () => Call(runtime, "ParseEmbedded", malformed.ToJsonString(), errorWire), "descriptor_rep_width");
    malformed["separators"]!["repetition"] = "*";
    ExactError($"{name}-repetition-metadata-delimiter", () => Call(runtime, "ParseEmbedded", malformed.ToJsonString(), errorWire), "descriptor_rep_delimiter");
    foreach (var (fieldIndex, errorKey) in new[] { (10, "descriptor_fixed_isa11"), (15, "descriptor_fixed_isa16") })
    {
        JsonNode fixedSchema = schema.DeepClone();
        fixedSchema["kind"]!["children"]![0]!["kind"]!["children"]![fieldIndex]!["fixed"] = "";
        JsonNode fixedDescriptor = JsonNode.Parse(sourceDescriptor)!;
        fixedDescriptor["schema"] = fixedSchema.ToJsonString();
        ExactError($"{name}-empty-fixed-{fieldIndex}", () => Call(runtime, "ParseEmbedded", fixedDescriptor.ToJsonString(), errorWire), errorKey);
    }
    if (!target) continue;
    object targetInput = Call(runtime, "ParseEmbedded", targetDescriptor, errorWire)!;
    ExactError($"{name}-target-missing-repetition", () => Call(runtime, "SerializeEmbedded", configured.ToJsonString(), targetInput), "output_rep_missing");
    // The ordinary JSON API remains available on an X12/X12 companion. Build a
    // typed view through the same runtime descriptor without running a graph.
    object repeatedInput = ReplaceField(targetInput, "W05", "W0502", "A^B");
    ExactError($"{name}-typed-output-repetition", () => Call(map, "SerializeX12", repeatedInput), "output_repetition");
    JsonNode before = Raw(targetInput)!;
    foreach (bool missing in new[] { false, true })
    {
        object empty = ReplaceField(targetInput, "ISA", "ISA11", missing ? null : "");
        JsonNode emptyBefore = Raw(empty)!;
        ValueCase($"{name}-default-isa11-{missing}", () => JsonValue.Create((string)Call(map, "SerializeX12", empty)!), JsonValue.Create(errorWire));
        ValueCase($"{name}-default-keeps-caller-{missing}", () => Raw(empty), emptyBefore);
    }
    object conflict = ReplaceField(targetInput, "ISA", "ISA11", "+");
    ExactError($"{name}-conflicting-output-isa11", () => Call(map, "SerializeX12", conflict), "output_isa11");
    ValueCase($"{name}-original-keeps-caller", () => Raw(targetInput), before);
}
}
catch (Exception setupFailure)
{
    outcomes.Add(new JsonObject { ["label"] = "host-setup-failure", ["error"] = Error(setupFailure), ["pass"] = false });
    failed++;
}
finally { Flush(); }
Console.WriteLine(JsonSerializer.Serialize(new { passed, failed }));
return failed == 0 ? 0 : 1;

object? Call(Type type, string name, params object?[] arguments)
{
    MethodInfo method = type.GetMethods(BindingFlags.Public | BindingFlags.Static).Single(candidate =>
        candidate.Name == name && candidate.GetParameters().Length == arguments.Length);
    try { return method.Invoke(null, arguments); }
    catch (TargetInvocationException error) when (error.InnerException is not null) { throw error.InnerException; }
}
void ValueCase(string label, Func<JsonNode?> action, JsonNode? expected)
{
    JsonNode? actual = null; JsonNode? error = null;
    try { actual = action(); } catch (Exception failure) { error = Error(failure); }
    bool ok = error is null && JsonNode.DeepEquals(actual, expected);
    outcomes.Add(new JsonObject { ["label"] = label, ["actual"] = actual?.DeepClone(), ["error"] = error, ["expected"] = expected?.DeepClone(), ["pass"] = ok });
    if (ok) passed++; else failed++;
    Flush();
}
void ExactError(string label, Func<object?> action, string key)
{
    JsonNode? actual = null; JsonNode? error = null;
    try { actual = Capture(action()); } catch (Exception failure) { error = Error(failure); }
    JsonNode expected = contract["errors"]![key]!;
    bool ok = error is not null && error["type"]?.GetValue<string>() == "Ferrule.Runtime.FerruleX12Exception"
        && error["error"]?.GetValue<string>() == expected["error"]!.GetValue<string>()
        && error["message"]?.GetValue<string>() == expected["message"]!.GetValue<string>()
        && JsonNode.DeepEquals(error["segment_index"], expected["segment_index"])
        && JsonNode.DeepEquals(error["field_path"], expected["field_path"]);
    outcomes.Add(new JsonObject { ["label"] = label, ["actual"] = actual, ["error"] = error, ["expected"] = expected.DeepClone(), ["pass"] = ok });
    if (ok) passed++; else failed++;
    Flush();
}
void Flush() => File.WriteAllText("complete-outcomes.original.json", outcomes.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
JsonNode Error(Exception error) => new JsonObject {
    ["type"] = error.GetType().FullName, ["message"] = error.Message, ["full_original"] = error.ToString(),
    ["error"] = Property(error, "Error")?.ToString(), ["segment_index"] = JsonSerializer.SerializeToNode(Property(error, "SegmentIndex")),
    ["field_path"] = JsonSerializer.SerializeToNode(Property(error, "FieldPath")),
};
object? Property(object value, string name) => value.GetType().GetProperty(name)?.GetValue(value);
JsonNode? Capture(object? value) => value switch { null => null, string text => JsonValue.Create(text), byte[] bytes => JsonValue.Create(Convert.ToBase64String(bytes)), _ => Raw(value) };
JsonNode? Raw(object? instance)
{
    if (instance is null) return null;
    if (instance.GetType().Name == "FerruleGroup")
    {
        var fields = new JsonArray();
        foreach (object field in (IEnumerable)Property(instance, "Fields")!)
            fields.Add(new JsonObject { ["name"] = (string)Property(field, "Name")!, ["value"] = Raw(Property(field, "Value")) });
        return new JsonObject { ["kind"] = "group", ["fields"] = fields };
    }
    if (instance.GetType().Name == "FerruleScalar")
    {
        object value = Property(instance, "Value")!;
        string kind = Property(value, "Kind")!.ToString()!;
        object? payload = kind switch { "String" => Property(value, "StringValue"), "Int64" => Property(value, "Int64Value"), "Double" => Property(value, "DoubleValue"), "Bool" => Property(value, "BooleanValue"), _ => null };
        return new JsonObject { ["kind"] = "scalar", ["type"] = kind, ["value"] = JsonSerializer.SerializeToNode(payload) };
    }
    return new JsonObject { ["unexpected_instance_type"] = instance.GetType().FullName, ["original"] = instance.ToString() };
}
JsonNode Expected(JsonNode schema, JsonNode? literal)
{
    if (schema["kind"]!["kind"]!.GetValue<string>() == "scalar")
    {
        string kind = literal is null ? "Null" : schema["kind"]!["ty"]!.GetValue<string>() switch { "string" => "String", "int" => "Int64", "float" => "Double", _ => "Unspecified" };
        return new JsonObject { ["kind"] = "scalar", ["type"] = kind, ["value"] = literal?.DeepClone() };
    }
    var fields = new JsonArray();
    foreach (JsonNode? child in schema["kind"]!["children"]!.AsArray())
    {
        string name = child!["name"]!.GetValue<string>();
        fields.Add(new JsonObject { ["name"] = name, ["value"] = Expected(child, literal?[name]) });
    }
    return new JsonObject { ["kind"] = "group", ["fields"] = fields };
}
JsonNode Projection(JsonNode value)
{
    JsonNode copy = value.DeepClone();
    void Visit(JsonNode selected)
    {
        if (selected is JsonObject fields)
            foreach (var pair in fields.ToArray()) { if (pair.Value is null) fields.Remove(pair.Key); else Visit(pair.Value); }
        else if (selected is JsonArray items) foreach (JsonNode? item in items) if (item is not null) Visit(item);
    }
    Visit(copy); return copy;
}
object ReplaceField(object instance, string segment, string field, string? text)
{
    Assembly assembly = instance.GetType().Assembly;
    Type fieldType = assembly.GetType("Ferrule.Runtime.FerruleField")!;
    Type groupType = assembly.GetType("Ferrule.Runtime.FerruleGroup")!;
    object Group(IEnumerable<(string Name, object Value)> entries)
    {
        var materialized = entries.ToArray(); Array fields = Array.CreateInstance(fieldType, materialized.Length);
        for (int index = 0; index < fields.Length; index++) fields.SetValue(Activator.CreateInstance(fieldType, materialized[index].Name, materialized[index].Value), index);
        return Activator.CreateInstance(groupType, fields)!;
    }
    var outer = new List<(string, object)>();
    foreach (object original in (IEnumerable)Property(instance, "Fields")!)
    {
        string name = (string)Property(original, "Name")!; object value = Property(original, "Value")!;
        if (name == segment)
        {
            var inner = new List<(string, object)>();
            foreach (object member in (IEnumerable)Property(value, "Fields")!)
            {
                string memberName = (string)Property(member, "Name")!;
                if (memberName == field)
                {
                    if (text is null) continue;
                    object scalarValue = Call(assembly.GetType("Ferrule.Runtime.FerruleValue")!, "FromString", text)!;
                    inner.Add((memberName, Activator.CreateInstance(assembly.GetType("Ferrule.Runtime.FerruleScalar")!, scalarValue)!));
                }
                else inner.Add((memberName, Property(member, "Value")!));
            }
            value = Group(inner);
        }
        outer.Add((name, value));
    }
    return Group(outer);
}
