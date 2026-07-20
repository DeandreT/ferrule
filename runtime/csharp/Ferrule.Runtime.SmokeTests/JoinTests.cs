using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void InnerJoinContexts()
    {
        var source = Group(
            Field("A", Repeated(
                JoinRow("A1", FerruleValue.FromInt64(1), "X"),
                JoinRow("A2", FerruleValue.FromInt64(1), "X"),
                JoinRow("A null", FerruleValue.Null, "X"),
                JoinRow("A nil", FerruleValue.XmlNil, "X"))),
            Field("B", Repeated(
                JoinRow("B1", FerruleValue.FromDouble(1), "X", "aid"),
                JoinRow("B2", FerruleValue.FromInt64(1), "X", "aid"),
                JoinRow("B null", FerruleValue.Null, "X", "aid"),
                JoinRow("B nil", FerruleValue.XmlNil, "X", "aid"))),
            Field("CustomerNr", Scalar(Text("B"))),
            Field("Customers", Repeated(
                Group(Field("Number", Scalar(Text("A"))), Field("Name", Scalar(Text("Ada")))),
                Group(Field("Number", Scalar(Text("B"))), Field("Name", Scalar(Text("Grace")))))));
        var catalog = Group(Field("C", Repeated(
            JoinRow("C1", FerruleValue.FromInt64(0), "X"),
            JoinRow("C2", FerruleValue.FromInt64(0), "X"))));
        var context = ScopeContext.FromSources(source, new[] { Field("catalog", catalog) });
        var plan = new FerruleJoinPlan(
            new FerruleJoinSource(new[] { "A" }),
            new[]
            {
                new FerruleJoinStage(
                    new FerruleJoinSource(new[] { "B" }),
                    new[]
                    {
                        new FerruleJoinKey(new[] { "A" }, new[] { "id" }, new[] { "aid" }),
                        new FerruleJoinKey(new[] { "A" }, new[] { "code" }, new[] { "code" }),
                    }),
                new FerruleJoinStage(
                    new FerruleJoinSource(new[] { "catalog", "C" }),
                    new[]
                    {
                        new FerruleJoinKey(
                            new[] { "B" },
                            new[] { "code" },
                            new[] { "code" }),
                    }),
            });

        var joined = context.InnerJoin(71, plan);
        Equal(8, joined.Count);
        Equal("A1:B1:C1,A1:B1:C2,A1:B2:C1,A1:B2:C2,A2:B1:C1,A2:B1:C2,A2:B2:C1,A2:B2:C2",
            string.Join(',', joined.Select(row => string.Join(':',
                JoinLabel(row, 71, "A"),
                JoinLabel(row, 71, "B"),
                JoinLabel(row, 71, "catalog", "C")))));
        for (var index = 0; index < joined.Count; index++)
        {
            Equal(index + 1L, joined[index].JoinPosition(71));
        }
        Equal(1L, joined[0].Position("A"));
        Equal(1L, joined[0].Position("B"));
        Equal(1L, joined[0].Position("catalog", "C"));
        Equal(2L, joined[5].Position("A"));
        Equal(1L, joined[5].Position("B"));
        Equal(2L, joined[5].Position("catalog", "C"));

        var compact = joined[5].WithCompactedPosition(3);
        Equal(3L, compact.JoinPosition(71));
        Equal(2L, compact.Position("A"));
        Equal(1L, compact.Position("B"));
        Equal(2L, compact.Position("catalog", "C"));
        Error(
            FerruleRuntimeError.MissingSourceField,
            () => compact.ResolveJoinScalar(72, new[] { "A" }, new[] { "label" }));
        Error(
            FerruleRuntimeError.MissingSourceField,
            () => compact.ResolveJoinScalar(71, new[] { "C" }, new[] { "label" }));
        var missingJoin = Error(FerruleRuntimeError.MissingJoinContext, () => compact.JoinPosition(72));
        Equal(72UL, missingJoin.Join);

        var singletonPlan = new FerruleJoinPlan(
            new FerruleJoinSource(
                new[] { "CustomerNr" },
                FerruleJoinSourceCardinality.Singleton),
            new[]
            {
                new FerruleJoinStage(
                    new FerruleJoinSource(new[] { "Customers" }),
                    new[]
                    {
                        new FerruleJoinKey(
                            new[] { "CustomerNr" },
                            Array.Empty<string>(),
                            new[] { "Number" }),
                    }),
            });
        var singleton = context.InnerJoin(72, singletonPlan);
        Equal(1, singleton.Count);
        Equal(Text("B"), singleton[0].ResolveJoinScalar(
            72,
            new[] { "CustomerNr" },
            Array.Empty<string>()));
        Equal(Text("Grace"), singleton[0].ResolveJoinScalar(
            72,
            new[] { "Customers" },
            new[] { "Name" }));

        Throws<ArgumentException>(() => new FerruleJoinPlan(
            new FerruleJoinSource(new[] { "A" }),
            Array.Empty<FerruleJoinStage>()));
        Throws<ArgumentException>(() => new FerruleJoinStage(
            new FerruleJoinSource(new[] { "B" }),
            Array.Empty<FerruleJoinKey>()));
    }

    private static FerruleGroup JoinRow(
        string label,
        FerruleValue id,
        string code,
        string idField = "id") =>
        Group(
            Field("label", Scalar(Text(label))),
            Field(idField, Scalar(id)),
            Field("code", Scalar(Text(code))));

    private static string JoinLabel(
        ScopeContext context,
        ulong join,
        params string[] collection) =>
        context.ResolveJoinScalar(join, collection, new[] { "label" }).StringValue;
}
