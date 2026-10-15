using System.Collections;
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

var utf8 = new UTF8Encoding(false, true);
var outcomes = new JsonArray();
int passed = 0, failed = 0, caseCount = 0;
try
{
    JsonNode contract = Read("contract.json");
    string name = Read("library.json")["name"]!.GetValue<string>();
    string version = contract["version"]!.GetValue<string>();
    JsonNode schema = Read($"{version}-schema.json");
    Assembly assembly = Assembly.LoadFrom(Path.GetFullPath($"{name}/bin/Release/net10.0/{name}.dll"));
    Type map = assembly.GetType("Ferrule.Generated.GeneratedMapping", true)!;
    string input = File.ReadAllText($"{version}-input.x12");
    JsonNode typed = Expected(schema, Read($"{version}-input-values.json"));
    string nativeJson = File.ReadAllText("native-output.json");

    // Ordinary input reads both acknowledgment branches as native's empty collections.
    ValueCase("parse-text", () => Raw(Call(map, "ParseX12", input)), typed);
    ValueCase("parse-bytes", () => Raw(Call(map, "ParseX12Bytes", utf8.GetBytes(input))), typed);
    ValueCase("map-json-text", () => JsonNode.Parse((string)Call(map, "ExecuteX12ToJson", input)!), JsonNode.Parse(nativeJson));
    ValueCase("map-json-bytes", () => JsonNode.Parse(utf8.GetString((byte[])Call(map, "ExecuteX12ToJsonBytes", utf8.GetBytes(input))!)),
        JsonNode.Parse(nativeJson));

    foreach (JsonNode? selected in Read("cases.json").AsArray())
    {
        caseCount++;
        string label = selected!["label"]!.GetValue<string>();
        string wire = File.ReadAllText(selected["input"]!.GetValue<string>());
        JsonNode expected = selected["expected_error"]!;
        ExactError($"{label}-parse-text", () => Call(map, "ParseX12", wire), expected);
        ExactError($"{label}-parse-bytes", () => Call(map, "ParseX12Bytes", utf8.GetBytes(wire)), expected);
        ExactError($"{label}-map-json", () => Call(map, "ExecuteX12ToJson", wire), expected);
    }
}
catch (Exception failure)
{
    outcomes.Add(new JsonObject { ["label"] = "host-setup-failure", ["error"] = Error(failure), ["pass"] = false });
    failed++;
}
finally { Flush(); }
Console.WriteLine(JsonSerializer.Serialize(new { passed, failed, cases = caseCount }));
return failed == 0 ? 0 : 1;

JsonNode Read(string path) => JsonNode.Parse(File.ReadAllText(path))!;
object? Property(object value, string name) => value.GetType().GetProperty(name)?.GetValue(value);
object? Call(Type type, string name, params object?[] arguments)
{
    MethodInfo method = type.GetMethods(BindingFlags.Public | BindingFlags.Static)
        .Single(candidate => candidate.Name == name && candidate.GetParameters().Length == arguments.Length);
    try { return method.Invoke(null, arguments); }
    catch (TargetInvocationException failure) when (failure.InnerException is not null) { throw failure.InnerException; }
}
JsonNode Error(Exception error) => new JsonObject {
    ["type"] = error.GetType().FullName, ["message"] = error.Message, ["full_original"] = error.ToString(),
    ["error"] = Property(error, "Error")?.ToString(), ["segment_index"] = JsonSerializer.SerializeToNode(Property(error, "SegmentIndex")),
    ["field_path"] = JsonSerializer.SerializeToNode(Property(error, "FieldPath")),
};
void Flush() => File.WriteAllText("complete-outcomes.original.json", outcomes.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
void ValueCase(string label, Func<JsonNode?> action, JsonNode? expected)
{
    JsonNode? actual = null, error = null;
    try { actual = action(); } catch (Exception failure) { error = Error(failure); }
    bool ok = error is null && JsonNode.DeepEquals(actual, expected);
    outcomes.Add(new JsonObject { ["label"] = label, ["actual"] = actual?.DeepClone(), ["error"] = error,
        ["expected"] = expected?.DeepClone(), ["pass"] = ok });
    if (ok) passed++; else failed++;
    Flush();
}
void ExactError(string label, Func<object?> action, JsonNode expected)
{
    JsonNode? actual = null, error = null;
    try { actual = Raw(action()); } catch (Exception failure) { error = Error(failure); }
    bool ok = error is not null && error["type"]?.GetValue<string>() == "Ferrule.Runtime." + expected["type"]!.GetValue<string>()
        && error["error"]?.GetValue<string>() == expected["error"]!.GetValue<string>()
        && error["message"]?.GetValue<string>() == expected["message"]!.GetValue<string>()
        && JsonNode.DeepEquals(error["segment_index"], expected["segment_index"])
        && JsonNode.DeepEquals(error["field_path"], expected["field_path"]);
    outcomes.Add(new JsonObject { ["label"] = label, ["actual"] = actual, ["error"] = error,
        ["expected"] = expected.DeepClone(), ["pass"] = ok });
    if (ok) passed++; else failed++;
    Flush();
}
JsonNode? Raw(object? value)
{
    if (value is null) return null;
    if (value is string text) return JsonValue.Create(text);
    string type = value.GetType().Name;
    if (type == "FerruleGroup")
    {
        var fields = new JsonArray();
        foreach (object field in (IEnumerable)Property(value, "Fields")!)
            fields.Add(new JsonObject { ["name"] = (string)Property(field, "Name")!, ["value"] = Raw(Property(field, "Value")) });
        return new JsonObject { ["kind"] = "Group", ["fields"] = fields };
    }
    if (type == "FerruleRepeated")
    {
        var items = new JsonArray();
        foreach (object item in (IEnumerable)Property(value, "Items")!) items.Add(Raw(item));
        return new JsonObject { ["kind"] = "Repeated", ["items"] = items };
    }
    if (type == "FerruleScalar")
    {
        object scalar = Property(value, "Value")!;
        string kind = Property(scalar, "Kind")!.ToString()!;
        JsonNode? payload = kind switch {
            "Null" => null, "String" => JsonValue.Create((string)Property(scalar, "StringValue")!),
            "Int64" => JsonValue.Create((long)Property(scalar, "Int64Value")!),
            "Double" => JsonValue.Create((double)Property(scalar, "DoubleValue")!),
            _ => JsonValue.Create(scalar.ToString()),
        };
        return new JsonObject { ["kind"] = "Scalar", ["scalar_kind"] = kind == "Int64" ? "Int" : kind == "Double" ? "Float" : kind, ["value"] = payload };
    }
    return new JsonObject { ["kind"] = type, ["full_original"] = value.ToString() };
}
JsonNode Expected(JsonNode schema, JsonNode? value, bool item = false)
{
    if (!item && schema["repeating"]?.GetValue<bool>() == true)
        return new JsonObject { ["kind"] = "Repeated", ["items"] = new JsonArray(value!.AsArray().Select(child => Expected(schema, child, true)).ToArray()) };
    JsonNode kind = schema["kind"]!;
    if (kind["kind"]!.GetValue<string>() == "group")
    {
        var fields = new JsonArray();
        foreach (JsonNode? child in kind["children"]!.AsArray())
            if (value!.AsObject().TryGetPropertyValue(child!["name"]!.GetValue<string>(), out JsonNode? supplied))
                fields.Add(new JsonObject { ["name"] = child["name"]!.DeepClone(), ["value"] = Expected(child, supplied) });
        return new JsonObject { ["kind"] = "Group", ["fields"] = fields };
    }
    if (value is null) return new JsonObject { ["kind"] = "Scalar", ["scalar_kind"] = "Null", ["value"] = null };
    return kind["ty"]!.GetValue<string>() switch {
        "string" => new JsonObject { ["kind"] = "Scalar", ["scalar_kind"] = "String", ["value"] = JsonValue.Create(value.GetValue<string>()) },
        "int" => new JsonObject { ["kind"] = "Scalar", ["scalar_kind"] = "Int", ["value"] = JsonValue.Create(value.GetValue<long>()) },
        "float" => new JsonObject { ["kind"] = "Scalar", ["scalar_kind"] = "Float", ["value"] = JsonValue.Create(value.GetValue<double>()) },
        _ => throw new InvalidOperationException("invented scalar oracle"),
    };
}
