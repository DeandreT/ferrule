extern alias Order;
extern alias Shipment;
extern alias LfOrder;
extern alias LfShipment;
extern alias Identity;
extern alias Deep;

using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

using OrderMap = Order::Ferrule.Generated.GeneratedMapping;
using ShipmentMap = Shipment::Ferrule.Generated.GeneratedMapping;
using LfOrderMap = LfOrder::Ferrule.Generated.GeneratedMapping;
using LfShipmentMap = LfShipment::Ferrule.Generated.GeneratedMapping;
using IdentityMap = Identity::Ferrule.Generated.GeneratedMapping;
using DeepMap = Deep::Ferrule.Generated.GeneratedMapping;

string order = File.ReadAllText("order-940.x12");
string expectedOrder = File.ReadAllText("order-expected.json");
string shipment = File.ReadAllText("shipment-input.json");
string expectedShipment = File.ReadAllText("shipment-945-expected.x12");
string expectedIdentity = File.ReadAllText("identity-940-expected.x12");
string deep = File.ReadAllText("deep-940.x12");
var utf8 = new UTF8Encoding(false, true);
int passed = 0, failed = 0;
Directory.CreateDirectory("outcomes");
var orderContext = new Order::Ferrule.Runtime.FerruleExecutionContext("authored-order-example");
var shipmentContext = new Shipment::Ferrule.Runtime.FerruleExecutionContext("authored-shipment-example");

JsonCase("order-text", () => OrderMap.ExecuteX12ToJson(order), expectedOrder);
JsonCase("order-context", () => OrderMap.ExecuteX12ToJson(order, orderContext), expectedOrder);
JsonCase("order-bytes", () => utf8.GetString(OrderMap.ExecuteX12ToJsonBytes(utf8.GetBytes(order))), expectedOrder);
JsonCase("order-bytes-context", () => utf8.GetString(OrderMap.ExecuteX12ToJsonBytes(utf8.GetBytes(order), orderContext)), expectedOrder);
TextCase("shipment-text", () => ShipmentMap.ExecuteJsonToX12(shipment), expectedShipment);
TextCase("shipment-context", () => ShipmentMap.ExecuteJsonToX12(shipment, shipmentContext), expectedShipment);
TextCase("shipment-bytes", () => utf8.GetString(ShipmentMap.ExecuteJsonToX12Bytes(utf8.GetBytes(shipment))), expectedShipment);
TextCase("shipment-bytes-context", () => utf8.GetString(ShipmentMap.ExecuteJsonToX12Bytes(utf8.GetBytes(shipment), shipmentContext)), expectedShipment);
TextCase("shipment-repeatable", () => ShipmentMap.ExecuteJsonToX12(shipment), expectedShipment);
var identityContext = new Identity::Ferrule.Runtime.FerruleExecutionContext("authored-identity-example");
TextCase("x12-to-x12-text", () => IdentityMap.ExecuteX12ToX12(order), expectedIdentity);
TextCase("x12-to-x12-context", () => IdentityMap.ExecuteX12ToX12(order, identityContext), expectedIdentity);
TextCase("x12-to-x12-bytes", () => utf8.GetString(IdentityMap.ExecuteX12ToX12Bytes(utf8.GetBytes(order))), expectedIdentity);
TextCase("x12-to-x12-bytes-context", () => utf8.GetString(IdentityMap.ExecuteX12ToX12Bytes(utf8.GetBytes(order), identityContext)), expectedIdentity);
TextCase("deepest-admitted-descriptor-text", () => DeepMap.ExecuteX12ToX12(deep), deep);
TextCase("deepest-admitted-descriptor-bytes", () => utf8.GetString(DeepMap.ExecuteX12ToX12Bytes(utf8.GetBytes(deep))), deep);

string lfOrder = order.Replace("~\n", "\n", StringComparison.Ordinal);
string lfExpected = expectedShipment.Replace("~\n", "\n", StringComparison.Ordinal);
JsonCase("order-lf-autodiscover", () => OrderMap.ExecuteX12ToJson(lfOrder), expectedOrder);
JsonCase("order-lf-configured", () => LfOrderMap.ExecuteX12ToJson(lfOrder), expectedOrder);
TextCase("shipment-lf", () => LfShipmentMap.ExecuteJsonToX12(shipment), lfExpected);
JsonCase("order-compact", () => OrderMap.ExecuteX12ToJson(order.Replace("\n", "", StringComparison.Ordinal)), expectedOrder);
JsonCase("order-crlf-formatting", () => OrderMap.ExecuteX12ToJson(order.Replace("\n", "\r\n", StringComparison.Ordinal)), expectedOrder);
JsonCase("order-other-punctuation", () => OrderMap.ExecuteX12ToJson(order.Replace('*', '|').Replace(':', '>').Replace('~', '!')), expectedOrder);
JsonCase("order-unicode-field", () => OrderMap.ExecuteX12ToJson(order.Replace("Example Buyer", "Example Café", StringComparison.Ordinal)), expectedOrder.Replace("Example Buyer", "Example Café", StringComparison.Ordinal));
string noNotes = order.Replace("NTE*ADD*0002:7~\n", "", StringComparison.Ordinal).Replace("SE*21*", "SE*20*", StringComparison.Ordinal);
var noNotesExpected = JsonNode.Parse(expectedOrder)!.AsObject();
noNotesExpected["notes"] = new JsonArray();
JsonCase("order-omitted-optional-segment", () => OrderMap.ExecuteX12ToJson(noNotes), noNotesExpected.ToJsonString());

// Execute the existing typed and JSON entry points on the actual parsed source.
string sourceSchema = Descriptor(typeof(OrderMap), "SourceJsonSchema");
string targetSchema = Descriptor(typeof(OrderMap), "TargetJsonSchema");
var parsedOrder = OrderMap.ParseX12(order);
var parsedOrderBytes = OrderMap.ParseX12Bytes(utf8.GetBytes(order));
string parsedJson = Order::Ferrule.Runtime.FerruleJson.SerializeEmbedded(sourceSchema, parsedOrder);
JsonCase("existing-json-api", () => OrderMap.ExecuteJson(parsedJson), expectedOrder);
JsonCase("existing-typed-api", () => Order::Ferrule.Runtime.FerruleJson.SerializeEmbedded(targetSchema, OrderMap.Execute(parsedOrder)), expectedOrder);
JsonCase("existing-typed-api-bytes-source", () => Order::Ferrule.Runtime.FerruleJson.SerializeEmbedded(targetSchema, OrderMap.Execute(parsedOrderBytes)), expectedOrder);
string shipmentSourceSchema = Descriptor(typeof(ShipmentMap), "SourceJsonSchema");
var parsedShipment = Shipment::Ferrule.Runtime.FerruleJson.ParseEmbedded(shipmentSourceSchema, shipment);
var mappedShipment = ShipmentMap.Execute(parsedShipment);
TextCase("direct-serializer", () => ShipmentMap.SerializeX12(mappedShipment), expectedShipment);
TextCase("direct-byte-serializer", () => utf8.GetString(ShipmentMap.SerializeX12Bytes(mappedShipment)), expectedShipment);

ErrorCase("encoding-invalid-utf8", () => OrderMap.ParseX12Bytes([0xc3, 0x28]), "Encoding");
ErrorCase("encoding-unpaired-surrogate", () => OrderMap.ParseX12(order + "\ud800"), "Encoding");
ErrorCase("bom-refused", () => OrderMap.ParseX12("\ufeff" + order), "Syntax");
ErrorCase("leading-space-refused", () => OrderMap.ParseX12(" " + order), "Syntax");
ErrorCase("isa-width", () => OrderMap.ParseX12(order.Replace("DEMO-SENDER    ", "DEMO-SENDER   ", StringComparison.Ordinal)), "Envelope");
ErrorCase("version-unsupported", () => OrderMap.ParseX12(order.Replace("00401*", "00501*", StringComparison.Ordinal)), "UnsupportedProfile");
ErrorCase("group-version-unsupported", () => OrderMap.ParseX12(order.Replace("004010~", "005010~", StringComparison.Ordinal)), "UnsupportedProfile");
ErrorCase("trailer-count", () => OrderMap.ParseX12(order.Replace("SE*21*", "SE*20*", StringComparison.Ordinal)), "Envelope");
ErrorCase("transaction-control", () => OrderMap.ParseX12(order.Replace("SE*21*0007", "SE*21*0008", StringComparison.Ordinal)), "Envelope");
ErrorCase("group-control", () => OrderMap.ParseX12(order.Replace("GE*1*00123", "GE*1*123", StringComparison.Ordinal)), "Envelope");
ErrorCase("interchange-control", () => OrderMap.ParseX12(order.Replace("IEA*1*000000123", "IEA*1*000000124", StringComparison.Ordinal)), "Envelope");
ErrorCase("extra-element", () => OrderMap.ParseX12(order.Replace("W05*N*ORDER-00042*PO-0042", "W05*N*ORDER-00042*PO-0042*EXTRA", StringComparison.Ordinal)), "Schema");
ErrorCase("extra-component", () => OrderMap.ParseX12(order.Replace("0002:7", "0002:7:EXTRA", StringComparison.Ordinal)), "Schema");
ErrorCase("unexpected-segment", () => OrderMap.ParseX12(order.Replace("W76*5", "ZZZ*5", StringComparison.Ordinal)), "Schema");
ErrorCase("qualified-party", () => OrderMap.ParseX12(order.Replace("N1*BT*", "N1*XX*", StringComparison.Ordinal)), "Schema");
ErrorCase("required-empty", () => OrderMap.ParseX12(order.Replace("W05*N*ORDER-00042*", "W05*N**", StringComparison.Ordinal)), "Value");
ErrorCase("code-list", () => OrderMap.ParseX12(order.Replace("2.00*EA*", "2.00*ZZ*", StringComparison.Ordinal)), "Value");
ErrorCase("identifier-length", () => OrderMap.ParseX12(order.Replace("WIDGET-A/BLUE", new string('X', 31), StringComparison.Ordinal)), "Value");
ErrorCase("nonfinite-float", () => OrderMap.ParseX12(order.Replace("2.00*EA*", "NaN*EA*", StringComparison.Ordinal)), "Value");
ErrorCase("integer-overflow", () => OrderMap.ParseX12(order.Replace("0002:7", "0002:9223372036854775808", StringComparison.Ordinal)), "Value");
ErrorCase("configured-separator-mismatch", () => LfOrderMap.ParseX12(order), "Syntax");
ErrorCase("unterminated-last-segment", () => OrderMap.ParseX12(order.TrimEnd('\n', '~')), "Syntax");
ErrorCase("missing-caller-control", () => ShipmentMap.ExecuteJsonToX12(shipment.Replace("\"ISA13\": \"000000456\"", "\"ISA13\": \"\"", StringComparison.Ordinal)), "Envelope");
ErrorCase("invalid-output-delimiter", () => ShipmentMap.ExecuteJsonToX12(shipment.Replace("WIDGET-A/BLUE", "WIDGET:A/BLUE", StringComparison.Ordinal)), "Value");

// Segment-limit refusal occurs before envelope/schema checks of the oversized document.
string tooManySegments = order[..order.IndexOf("GS*", StringComparison.Ordinal)] + string.Concat(Enumerable.Repeat("NTE*ADD*0002:7~\n", 100_001));
ErrorCase("segment-resource-limit", () => OrderMap.ParseX12(tooManySegments), "ResourceLimit");
File.WriteAllText("outcomes/summary.json", JsonSerializer.Serialize(new { passed, failed }));
Console.WriteLine(JsonSerializer.Serialize(new { passed, failed }));
if (failed != 0) Environment.ExitCode = 1;

string Descriptor(Type type, string name)
    => (string)type.GetField(name, BindingFlags.NonPublic | BindingFlags.Static)!.GetRawConstantValue()!;

void JsonCase(string name, Func<string> run, string expected)
    => ResultCase(name, run, expected, (actual, oracle) => JsonNode.DeepEquals(JsonNode.Parse(actual), JsonNode.Parse(oracle)));
void TextCase(string name, Func<string> run, string expected)
    => ResultCase(name, run, expected, (actual, oracle) => utf8.GetBytes(actual).AsSpan().SequenceEqual(utf8.GetBytes(oracle)));
void ResultCase(string name, Func<string> run, string expected, Func<string, string, bool> compare)
{
    string? actual = null;
    Exception? exception = null;
    try { actual = run(); } catch (Exception error) { exception = error; }
    if (actual is not null) File.WriteAllBytes($"outcomes/{name}.output.bin", utf8.GetBytes(actual));
    File.WriteAllText($"outcomes/{name}.expected.txt", expected, utf8);
    File.WriteAllText($"outcomes/{name}.result.json", JsonSerializer.Serialize(new { exception = exception?.GetType().FullName, message = exception?.Message, error = Category(exception), produced = actual is not null }));
    // Original complete outcome and expected oracle exist before semantic comparison.
    bool success = exception is null && actual is not null && compare(actual, expected);
    if (success) passed++; else { failed++; Console.Error.WriteLine($"{name}: complete outcome retained; expected output differs."); }
}
void ErrorCase(string name, Action run, string expected)
{
    Exception? exception = null;
    try { run(); } catch (Exception error) { exception = error; }
    string? actual = Category(exception);
    File.WriteAllText($"outcomes/{name}.result.json", JsonSerializer.Serialize(new { exception = exception?.GetType().FullName, message = exception?.Message, error = actual, expected, returned = exception is null }));
    bool success = exception is not null && actual == expected;
    if (success) passed++; else { failed++; Console.Error.WriteLine($"{name}: expected {expected}, received {actual ?? "successful return"}."); }
}
string? Category(Exception? error) => error?.GetType().GetProperty("Error")?.GetValue(error)?.ToString();
