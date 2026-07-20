using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void NamedSourceContexts()
    {
        var catalog = Group(Field(
            "Customers",
            Repeated(
                Customer(1, "Ada"),
                Customer(2, "Lin"))));
        var settings = Group(Field("Prefix", Scalar(Text("extra"))));
        var primary = Group(
            Field("settings", Group(Field("Prefix", Scalar(Text("primary"))))),
            Field("Needle", Scalar(FerruleValue.FromInt64(2))));
        var execution = new FerruleExecutionContext("mapping.ferrule");
        var context = ScopeContext.FromSources(
            primary,
            new[]
            {
                Field("catalog", catalog),
                Field("settings", settings),
            },
            execution);

        Equal(2, context.Frames.Count);
        Equal("catalog,settings", string.Join(',', ((FerruleGroup)context.Frames[0]).Fields.Select(
            field => field.Name)));
        Equal(Text("primary"), context.ResolveScalar("settings", "Prefix"));
        Equal(Text("Ada"), context.ResolveScalar("catalog", "Customers", "Name"));
        Equal(Text("mapping.ferrule"), context.ResolveRuntimeValue(FerruleRuntimeValue.MappingFilePath));

        var customers = context.IterateSource("catalog", "Customers");
        Equal(2, customers.Count);
        Equal(Text("Ada"), customers[0].ResolveScalar("Name"));
        Equal(Text("Lin"), customers[1].ResolveScalar("Name"));

        var aggregate = context.AggregateItems("catalog", "Customers");
        Equal(2, aggregate.Count);
        Equal(Text("Lin"), aggregate[1].AggregateCurrentScalar("Name"));
        Equal(
            Text("Lin"),
            context.Lookup(
                new[] { "catalog", "Customers" },
                new[] { "Id" },
                FerruleValue.FromInt64(2),
                new[] { "Name" }));

        Equal(1, ScopeContext.FromSource(primary).Frames.Count);
        Error(
            FerruleRuntimeError.DuplicateField,
            () => ScopeContext.FromSources(
                primary,
                new[]
                {
                    Field("catalog", catalog),
                    Field("catalog", catalog),
                }));
        Throws<ArgumentNullException>(() => ScopeContext.FromSources(null!, Array.Empty<FerruleField>()));
        Throws<ArgumentNullException>(() => ScopeContext.FromSources(primary, null!));
    }

    private static FerruleGroup Customer(long id, string name) =>
        Group(
            Field("Id", Scalar(FerruleValue.FromInt64(id))),
            Field("Name", Scalar(Text(name))));
}
