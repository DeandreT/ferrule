using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void LookupContexts()
    {
        var source = Group(
            Field("Catalog", Repeated(
                Row(FerruleValue.FromInt64(1), Text("outer integer")),
                Row(FerruleValue.FromDouble(1), Text("outer double")))),
            Field("Rows", Repeated(
                Group(Field("Catalog", Repeated(
                    Row(FerruleValue.FromInt64(1), Text("inner first")),
                    Row(FerruleValue.FromInt64(1), Text("inner second")),
                    Row(FerruleValue.FromDouble(1), Text("inner double")),
                    Group(Field("Name", Scalar(Text("missing key")))),
                    Row(FerruleValue.Null, Text("explicit null")),
                    Group(Field("Id", Scalar(Text("missing value"))))))),
                Group(Field("Catalog", Scalar(Text("not a collection")))))));
        var root = ScopeContext.FromSource(source);
        var rows = root.IterateSource("Rows");

        Equal(
            Text("inner first"),
            rows[0].Lookup(
                new[] { "Catalog" },
                new[] { "Id" },
                FerruleValue.FromInt64(1),
                new[] { "Name" }));
        Equal(
            Text("inner double"),
            rows[0].Lookup(
                new[] { "Catalog" },
                new[] { "Id" },
                FerruleValue.FromDouble(1),
                new[] { "Name" }));
        Equal(
            Text("explicit null"),
            rows[0].Lookup(
                new[] { "Catalog" },
                new[] { "Id" },
                FerruleValue.Null,
                new[] { "Name" }));
        Equal(
            FerruleValue.Null,
            rows[0].Lookup(
                new[] { "Catalog" },
                new[] { "Id" },
                Text("missing value"),
                new[] { "Name" }));
        Equal(
            FerruleValue.Null,
            rows[0].Lookup(
                new[] { "Catalog" },
                new[] { "Id" },
                Text("absent"),
                new[] { "Name" }));

        // A non-repeated inner collision does not hide an outer collection.
        Equal(
            Text("outer integer"),
            rows[1].Lookup(
                new[] { "Catalog" },
                new[] { "Id" },
                FerruleValue.FromInt64(1),
                new[] { "Name" }));

        var nested = ScopeContext.FromSource(Group(Field(
            "Directory",
            Group(Field("Catalog", Repeated(
                Row(Text("A"), Text("nested"))))))));
        Equal(
            Text("nested"),
            nested.Lookup(
                new[] { "Directory", "Catalog" },
                new[] { "Id" },
                Text("A"),
                new[] { "Name" }));

        var scalarCollection = ScopeContext.FromSource(Repeated(Scalar(Text("first"))));
        Equal(
            Text("first"),
            scalarCollection.Lookup(
                Array.Empty<string>(),
                Array.Empty<string>(),
                Text("first"),
                Array.Empty<string>()));

        // Lookup follows exact fields; it must not flatten an intermediate repetition.
        var multiHop = ScopeContext.FromSource(Group(Field("Groups", Repeated(
            Group(Field("Catalog", Repeated(Row(Text("A"), Text("flattened")))))))));
        Error(
            FerruleRuntimeError.MissingSourceField,
            () => multiHop.Lookup(
                new[] { "Groups", "Catalog" },
                new[] { "Id" },
                Text("A"),
                new[] { "Name" }));

        Error(
            FerruleRuntimeError.MissingSourceField,
            () => root.Lookup(
                new[] { "Missing" },
                new[] { "Id" },
                FerruleValue.FromInt64(1),
                new[] { "Name" }));

        Throws<ArgumentNullException>(() => root.Lookup(
            null!,
            Array.Empty<string>(),
            FerruleValue.Null,
            Array.Empty<string>()));
        Throws<ArgumentNullException>(() => root.Lookup(
            new string[] { null! },
            Array.Empty<string>(),
            FerruleValue.Null,
            Array.Empty<string>()));
    }

    private static FerruleRepeated Repeated(params FerruleInstance[] items) => new(items);

    private static FerruleGroup Row(FerruleValue id, FerruleValue name) =>
        Group(Field("Id", Scalar(id)), Field("Name", Scalar(name)));
}
