using Ferrule.Generated;
using Ferrule.Runtime;

var source = Group(
    Field(
        "Orders",
        Repeated(
            Order("Lin", 19.5, "USD"),
            Order("Ada", 12.25, "USD"),
            Order("Ignore", 0.0, "USD"),
            Order("Ada", 30.0, "EUR"))));

var target = RequireGroup(GeneratedMapping.Execute(source), "mapping result");
var invoices = RequireRepeated(RequireField(target, "Invoices"), "Target.Invoices");

var expected = new[]
{
    new ExpectedInvoice(1, "Ada", "ADA / 30.00 EUR", 30.0),
    new ExpectedInvoice(2, "Ada", "ADA / 12.25 USD", 12.25),
    new ExpectedInvoice(3, "Lin", "LIN / 19.50 USD", 19.5),
};

Equal(expected.Length, invoices.Items.Count, "invoice count");
for (var index = 0; index < expected.Length; index += 1)
{
    var invoice = RequireGroup(invoices.Items[index], $"Target.Invoices[{index}]");
    var actual = new ExpectedInvoice(
        RequireInt64(invoice, "Sequence"),
        RequireString(invoice, "Customer"),
        RequireString(invoice, "Display"),
        RequireDouble(invoice, "Amount"));

    Equal(expected[index], actual, $"invoice {index + 1}");
    Console.WriteLine($"{actual.Sequence}: {actual.Display}");
}

Console.WriteLine("C# generated mapping example passed.");

static FerruleGroup Order(string customer, double amount, string currency) =>
    Group(
        Field("Customer", Scalar(FerruleValue.FromString(customer))),
        Field("Amount", Scalar(FerruleValue.FromDouble(amount))),
        Field("Currency", Scalar(FerruleValue.FromString(currency))));

static FerruleValue RequireScalar(FerruleGroup group, string fieldName)
{
    var instance = RequireField(group, fieldName);
    if (instance is FerruleScalar scalar)
    {
        return scalar.Value;
    }

    throw new InvalidOperationException($"Field '{fieldName}' is not a scalar.");
}

static long RequireInt64(FerruleGroup group, string fieldName)
{
    var value = RequireScalar(group, fieldName);
    if (value.Kind == FerruleValueKind.Int64)
    {
        return value.Int64Value;
    }

    throw new InvalidOperationException($"Field '{fieldName}' is not an Int64 value.");
}

static double RequireDouble(FerruleGroup group, string fieldName)
{
    var value = RequireScalar(group, fieldName);
    if (value.Kind == FerruleValueKind.Double)
    {
        return value.DoubleValue;
    }

    throw new InvalidOperationException($"Field '{fieldName}' is not a Double value.");
}

static string RequireString(FerruleGroup group, string fieldName)
{
    var value = RequireScalar(group, fieldName);
    if (value.Kind == FerruleValueKind.String)
    {
        return value.StringValue;
    }

    throw new InvalidOperationException($"Field '{fieldName}' is not a String value.");
}

static FerruleInstance RequireField(FerruleGroup group, string fieldName)
{
    if (group.TryGetField(fieldName, out var value))
    {
        return value;
    }

    throw new InvalidOperationException($"Required field '{fieldName}' is missing.");
}

static FerruleGroup RequireGroup(FerruleInstance instance, string location) =>
    instance as FerruleGroup
    ?? throw new InvalidOperationException($"{location} is not a group.");

static FerruleRepeated RequireRepeated(FerruleInstance instance, string location) =>
    instance as FerruleRepeated
    ?? throw new InvalidOperationException($"{location} is not a repeated value.");

static void Equal<T>(T expected, T actual, string location)
    where T : IEquatable<T>
{
    if (!expected.Equals(actual))
    {
        throw new InvalidOperationException(
            $"Unexpected {location}: expected '{expected}', found '{actual}'.");
    }
}

static FerruleField Field(string name, FerruleInstance value) => new(name, value);
static FerruleScalar Scalar(FerruleValue value) => new(value);
static FerruleGroup Group(params FerruleField[] fields) => new(fields);
static FerruleRepeated Repeated(params FerruleInstance[] items) => new(items);

internal sealed record ExpectedInvoice(
    long Sequence,
    string Customer,
    string Display,
    double Amount);
