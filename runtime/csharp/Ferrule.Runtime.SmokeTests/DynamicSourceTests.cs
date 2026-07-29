using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void DynamicSourceContexts()
    {
        var source = Group(
            Field(
                "Properties",
                Group(
                    Field("wanted", Scalar(Text("outer"))),
                    Field("explicit-null", Scalar(FerruleValue.JsonNull)),
                    Field("structural", Group()))),
            Field(
                "Rows",
                Repeated(
                    Group(Field("Properties", Group())),
                    Group(Field("Other", Scalar(Text("unrelated")))))));
        var context = ScopeContext.FromSource(source);

        Equal(
            Text("outer"),
            context.ResolveDynamicScalar(null, new[] { "Properties" }, Text("wanted")));
        Equal(
            FerruleValue.JsonNull,
            context.ResolveDynamicScalar(null, new[] { "Properties" }, Text("explicit-null")));
        Equal(
            FerruleValue.Null,
            context.ResolveDynamicScalar(null, new[] { "Properties" }, Text("structural")));
        Equal(
            FerruleValue.Null,
            context.ResolveDynamicScalar(null, new[] { "Properties" }, Text("missing")));
        Equal(
            FerruleValue.Null,
            context.ResolveDynamicScalar(
                null,
                new[] { "Properties" },
                FerruleValue.FromInt64(1)));

        var rows = context.IterateSource("Rows");
        Equal(
            FerruleValue.Null,
            rows[0].ResolveDynamicScalar(null, new[] { "Properties" }, Text("wanted")));
        Equal(
            Text("outer"),
            rows[1].ResolveDynamicScalar(null, new[] { "Properties" }, Text("wanted")));
        Equal(
            FerruleValue.Null,
            rows[1].ResolveDynamicScalar(
                new[] { "Rows" },
                new[] { "Properties" },
                Text("wanted")));

        var named = ScopeContext.FromSources(
            Group(),
            new[]
            {
                Field(
                    "Settings",
                    Group(Field("selected", Scalar(Text("named"))))),
            });
        Equal(
            Text("named"),
            named.ResolveDynamicScalar(null, new[] { "Settings" }, Text("selected")));
    }
}
