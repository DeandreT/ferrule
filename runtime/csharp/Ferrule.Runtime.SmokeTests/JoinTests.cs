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

    private static void ComputedKeyInnerJoinContexts()
    {
        var customers = Group(
            Field("Customers", Repeated(
                Group(Field("Number", Scalar(Text("C1"))), Field("Name", Scalar(Text("Ada")))),
                Group(Field("Number", Scalar(Text("C2"))), Field("Name", Scalar(Text("Grace")))),
                Group(Field("Number", Scalar(Text("C2"))), Field("Name", Scalar(Text("Hopper")))),
                Group(Field("Number", Scalar(Text(""))), Field("Name", Scalar(Text("Blank")))),
                Group(Field("Number", Scalar(FerruleValue.FromInt64(7))), Field("Name", Scalar(Text("Seven")))))),
            Field("Orders", Repeated(
                Group(Field("Id", Scalar(Text("O1"))), Field("Customer", Scalar(Text("C2")))),
                Group(Field("Id", Scalar(Text("O2"))), Field("Customer", Scalar(Text("C1")))),
                Group(Field("Id", Scalar(Text("O3"))), Field("Customer", Scalar(Text("C2")))))));
        var context = ScopeContext.FromSources(customers, Array.Empty<FerruleField>());
        var customerSource = new FerruleJoinSource(new[] { "Customers" });
        var keyFirst = new FerruleJoinPlan(
            FerruleJoinSource.Computed,
            new[] { new FerruleJoinStage(customerSource, new[] { FerruleJoinKey.FromComputed(new[] { "Number" }) }) });
        var keySecond = new FerruleJoinPlan(
            customerSource,
            new[]
            {
                new FerruleJoinStage(
                    FerruleJoinSource.Computed,
                    new[] { new FerruleJoinKey(new[] { "Customers" }, new[] { "Number" }, Array.Empty<string>()) }),
            });
        string Names(IReadOnlyList<ScopeContext> tuples) => string.Join(',', tuples.Select(tuple =>
            $"{tuple.ResolveJoinScalar(81, new[] { "Customers" }, new[] { "Name" }).StringValue}@{tuple.JoinPosition(81)}"));
        foreach (var plan in new[] { keyFirst, keySecond })
        {
            Equal("Grace@1,Hopper@2", Names(context.InnerJoin(81, plan, Text("C2"))));
            // An empty string is a value; a string key matches an integer by text.
            Equal("Blank@1", Names(context.InnerJoin(81, plan, Text(""))));
            Equal("Seven@1", Names(context.InnerJoin(81, plan, Text("7"))));
            foreach (var nullLike in new[] { FerruleValue.Null, FerruleValue.JsonNull, FerruleValue.XmlNil })
            {
                Equal(0, context.InnerJoin(81, plan, nullLike).Count);
            }
            Throws<FerruleRuntimeException>(() => context.InnerJoin(
                81, plan, FerruleValue.FromDecimal(FerruleDecimal96.ParseExact("7"))));
            // A plan with a computed source needs its key, and only such a plan takes one.
            Throws<ArgumentException>(() => context.InnerJoin(81, plan));
        }
        Throws<ArgumentException>(() => context.InnerJoin(81, new FerruleJoinPlan(
            customerSource,
            new[] { new FerruleJoinStage(new FerruleJoinSource(new[] { "Orders" }), new[]
            {
                new FerruleJoinKey(new[] { "Customers" }, new[] { "Number" }, new[] { "Customer" }),
            }) }), Text("C2")));

        // The key as a middle stage's right source, then as the first source read again later.
        var orderSource = new FerruleJoinSource(new[] { "Orders" });
        var middle = new FerruleJoinPlan(customerSource, new[]
        {
            new FerruleJoinStage(
                FerruleJoinSource.Computed,
                new[] { new FerruleJoinKey(new[] { "Customers" }, new[] { "Number" }, Array.Empty<string>()) }),
            new FerruleJoinStage(
                orderSource,
                new[] { new FerruleJoinKey(new[] { "Customers" }, new[] { "Number" }, new[] { "Customer" }) }),
        });
        var first = new FerruleJoinPlan(FerruleJoinSource.Computed, new[]
        {
            new FerruleJoinStage(customerSource, new[] { FerruleJoinKey.FromComputed(new[] { "Number" }) }),
            new FerruleJoinStage(orderSource, new[] { FerruleJoinKey.FromComputed(new[] { "Customer" }) }),
        });
        string Orders(IReadOnlyList<ScopeContext> tuples) => string.Join(',', tuples.Select(tuple =>
            tuple.ResolveJoinScalar(81, new[] { "Orders" }, new[] { "Id" }).StringValue));
        Equal("O1,O3,O1,O3", Orders(context.InnerJoin(81, middle, Text("C2"))));
        Equal("O2", Orders(context.InnerJoin(81, first, Text("C1"))));
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

    private static FerruleGroup OwnedJoinDocument(string header, params (long Id, string Code)[] rows) =>
        Group(
            Field("Header", Group(Field("code", Scalar(Text(header))))),
            Field("Rows", Repeated(rows.Select(row => (FerruleInstance)Group(
                Field("id", Scalar(FerruleValue.FromInt64(row.Id))),
                Field("code", Scalar(Text(row.Code))))).ToArray())));

    private static readonly FerruleOwnedJoinSource PrimaryJoinRows =
        new(FerruleOwnedSourceOwner.Primary, new[] { "Rows" });
    private static readonly FerruleOwnedJoinSource LookupJoinRows =
        new(FerruleOwnedSourceOwner.Named("Lookup"), new[] { "Rows" });
    private static readonly FerruleOwnedJoinKey[] JoinById = { new(0, new[] { "id" }, new[] { "id" }) };

    private static string OwnedJoinSummary(FerruleBasicScopeContext tuple) => string.Join(",",
        tuple.OwnedJoinField(5, FerruleOwnedSourceOwner.Primary, new[] { "Rows" }, new[] { "code" }).StringValue,
        tuple.OwnedJoinField(5, FerruleOwnedSourceOwner.Named("Lookup"), new[] { "Rows" }, new[] { "code" }).StringValue,
        tuple.OwnedJoinSourcePosition(5, FerruleOwnedSourceOwner.Primary, new[] { "Rows" }),
        tuple.OwnedJoinSourcePosition(5, FerruleOwnedSourceOwner.Named("Lookup"), new[] { "Rows" }),
        tuple.OwnedJoinPosition(5));

    private static void OwnedInnerJoinContexts()
    {
        // Same-path sources join by full identity, left-major with duplicates.
        var primary = OwnedJoinDocument("H", (1, "p1"), (2, "p2"), (2, "p3"), (3, "p4"));
        var lookup = OwnedJoinDocument("l2", (2, "l1"), (1, "l2"), (2, "l3"), (4, "l4"));
        var root = BasicRoot(primary, new FerruleDeclaredNamedSourceRoot("Lookup", lookup));
        var tuples = root.OwnedInnerJoin(5, PrimaryJoinRows, new[] { new FerruleOwnedJoinStage(LookupJoinRows, JoinById) });
        Equal("p1,l2,1,2,1|p2,l1,2,1,2|p2,l3,2,3,3|p3,l1,3,1,4|p3,l3,3,3,5",
            string.Join("|", tuples.Select(OwnedJoinSummary)));
        Equal("p3,l1,3,1,1", OwnedJoinSummary(tuples[3].WithOwnedJoinPosition(5, 1)));
        Error(FerruleRuntimeError.MissingJoinContext, () => tuples[0].OwnedJoinPosition(6));
        Error(FerruleRuntimeError.MissingSourceField,
            () => tuples[0].OwnedJoinField(5, FerruleOwnedSourceOwner.Named("Lookup"), new[] { "Header" }, new[] { "code" }));

        // A singleton third stage keys on the Lookup row code.
        var header = new FerruleOwnedJoinSource(FerruleOwnedSourceOwner.Named("Lookup"), new[] { "Header", "code" }, singleton: true);
        var staged = root.OwnedInnerJoin(5, PrimaryJoinRows, new[]
        {
            new FerruleOwnedJoinStage(LookupJoinRows, JoinById),
            new FerruleOwnedJoinStage(header, new[] { new FerruleOwnedJoinKey(1, new[] { "code" }, Array.Empty<string>()) }),
        });
        Equal("p1,l2,1,2,1", string.Join("|", staged.Select(OwnedJoinSummary)));
        Equal(Text("l2"), staged[0].OwnedJoinField(5, header.Owner, header.Collection, Array.Empty<string>()));
        Equal((nuint)1, staged[0].OwnedJoinSourcePosition(5, header.Owner, header.Collection));

        // Null-like keys and empty sides never match; an unsupplied owner fails typed.
        var nulls = Group(Field("Rows", Repeated(
            Group(Field("id", Scalar(FerruleValue.Null)), Field("code", Scalar(Text("p1")))),
            Group(Field("id", Scalar(FerruleValue.JsonNull)), Field("code", Scalar(Text("p2")))))));
        var nullLookup = Group(Field("Rows", Repeated(
            Group(Field("id", Scalar(FerruleValue.Null)), Field("code", Scalar(Text("l1")))))));
        Equal(0, BasicRoot(nulls, new FerruleDeclaredNamedSourceRoot("Lookup", nullLookup))
            .OwnedInnerJoin(5, PrimaryJoinRows, new[] { new FerruleOwnedJoinStage(LookupJoinRows, JoinById) }).Count);
        Equal(0, BasicRoot(primary, new FerruleDeclaredNamedSourceRoot("Lookup", Group(Field("Rows", Repeated()))))
            .OwnedInnerJoin(5, PrimaryJoinRows, new[] { new FerruleOwnedJoinStage(LookupJoinRows, JoinById) }).Count);
        var unavailable = Error(FerruleRuntimeError.OwnedSource,
            () => BasicRoot(primary, new FerruleDeclaredNamedSourceRoot("Lookup", null))
                .OwnedInnerJoin(5, PrimaryJoinRows, new[] { new FerruleOwnedJoinStage(LookupJoinRows, JoinById) }));
        Equal(FerruleOwnedSourceErrorKind.SourceUnavailable, unavailable.OwnedSourceFailure!.Error);

        // An active owned row correlates its own source and anchors its lines.
        FerruleGroup Line(long order, string sku) =>
            Group(Field("order", Scalar(FerruleValue.FromInt64(order))), Field("sku", Scalar(Text(sku))));
        var orders = Group(Field("Orders", Repeated(
            Group(Field("id", Scalar(FerruleValue.FromInt64(7))), Field("Lines", Repeated(Line(7, "a"), Line(9, "b"), Line(7, "b")))),
            Group(Field("id", Scalar(FerruleValue.FromInt64(9))), Field("Lines", Repeated(Line(9, "a"), Line(9, "c")))))));
        var order = new FerruleOwnedJoinSource(FerruleOwnedSourceOwner.Primary, new[] { "Orders" });
        var lines = new FerruleOwnedJoinSource(FerruleOwnedSourceOwner.Primary, new[] { "Orders", "Lines" });
        var byOrder = new[] { new FerruleOwnedJoinStage(lines, new[] { new FerruleOwnedJoinKey(0, new[] { "id" }, new[] { "order" }) }) };
        var ordersRoot = BasicRoot(orders);
        var correlated = string.Join("/", ordersRoot.WalkOwnedSourceCohort(order.Owner, order.Collection).Select(row =>
            string.Join("|", row.Context.OwnedInnerJoin(5, order, byOrder).Select(tuple => string.Join(",",
                tuple.OwnedJoinField(5, lines.Owner, lines.Collection, new[] { "sku" }).StringValue,
                tuple.OwnedJoinSourcePosition(5, order.Owner, order.Collection),
                tuple.OwnedJoinSourcePosition(5, lines.Owner, lines.Collection))))));
        Equal("a,1,1|b,1,3/a,2,1|c,2,2", correlated);
        var flattened = ordersRoot.OwnedInnerJoin(5, lines, new[]
        {
            new FerruleOwnedJoinStage(order, new[] { new FerruleOwnedJoinKey(0, new[] { "order" }, new[] { "id" }) }),
        });
        Equal("1:1|2:2|3:1|1:2|2:2", string.Join("|", flattened.Select(tuple =>
            $"{tuple.OwnedJoinSourcePosition(5, lines.Owner, lines.Collection)}:{tuple.OwnedJoinSourcePosition(5, order.Owner, order.Collection)}")));
        Throws<ArgumentException>(() => new FerruleOwnedJoinStage(lines, Array.Empty<FerruleOwnedJoinKey>()));
    }

    // A joined aggregate reduces the tuples of one owned join planned in its
    // receiving context with the shared reducers. An active owned row
    // correlates only same-owner sources; a same-path named source keeps its
    // root walk.
    private static void OwnedJoinedAggregates()
    {
        FerruleGroup Order(long id) => Group(Field("id", Scalar(FerruleValue.FromInt64(id))));
        var primary = Group(Field("Orders", Repeated(Order(7), Order(9))));
        var lookup = Group(Field("Orders", Repeated(Order(9), Order(7), Order(9))));
        var primaryOrders = new FerruleOwnedJoinSource(FerruleOwnedSourceOwner.Primary, new[] { "Orders" });
        var lookupOrders = new FerruleOwnedJoinSource(FerruleOwnedSourceOwner.Named("Lookup"), new[] { "Orders" });
        var stages = new[] { new FerruleOwnedJoinStage(lookupOrders, JoinById) };
        FerruleValue Reduce(FerruleBasicScopeContext context, FerruleAggregateOperation operation) =>
            FerruleAggregates.Apply(operation, context.OwnedInnerJoin(8, primaryOrders, stages)
                .Select(tuple => FerruleValue.FromInt64(checked((long)tuple.OwnedJoinSourcePosition(8, lookupOrders.Owner, lookupOrders.Collection))))
                .ToArray(), Text(","));
        var root = BasicRoot(primary, new FerruleDeclaredNamedSourceRoot("Lookup", lookup));
        var perOrder = root.WalkOwnedSourceCohort(primaryOrders.Owner, primaryOrders.Collection).Select(row =>
            $"{Reduce(row.Context, FerruleAggregateOperation.Count).Int64Value}:{Reduce(row.Context, FerruleAggregateOperation.Join).StringValue}");
        Equal("1:2|2:1,3", string.Join("|", perOrder));
        Equal(Text("2,1,3"), Reduce(root, FerruleAggregateOperation.Join));

        // Empty sides reduce under the legacy empty policy.
        var empty = BasicRoot(primary, new FerruleDeclaredNamedSourceRoot("Lookup", Group(Field("Orders", Repeated()))));
        Equal(FerruleValue.FromInt64(0), Reduce(empty, FerruleAggregateOperation.Sum));
        Equal(FerruleValue.Null, Reduce(empty, FerruleAggregateOperation.Avg));
        Equal(Text(""), Reduce(empty, FerruleAggregateOperation.Join));
    }
}
