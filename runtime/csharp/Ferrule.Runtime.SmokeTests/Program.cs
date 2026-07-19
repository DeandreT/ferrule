using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static int Main()
    {
        var tests = new (string Name, Action Run)[]
        {
            ("group traversal", GroupTraversal),
            ("copy current group", CopyCurrentGroup),
            ("repeated first item", RepeatedFirstItem),
            ("document first item", DocumentFirstItem),
            ("mapped sequence remains non-scalar", MappedSequenceIsNotRepeated),
            ("typed resolver errors", TypedResolverErrors),
            ("null and XML nil", NullAndXmlNil),
            ("double domain", DoubleDomain),
            ("value maps", ValueMaps),
            ("runtime execution context", RuntimeExecutionContext),
            ("field order", FieldOrder),
            ("empty field names", EmptyFieldNames),
            ("scalar functions", ScalarFunctions),
            ("portable scalar function batch A", ScalarFunctionBatchA),
            ("portable scalar function batch B", ScalarFunctionBatchB),
            ("typed function errors", TypedFunctionErrors),
            ("scope source iteration", ScopeSourceIteration),
            ("aggregate source contexts", AggregateSourceContexts),
            ("aggregate reductions", AggregateReductions),
            ("aggregate numeric precision", AggregateNumericPrecision),
            ("typed aggregate errors", TypedAggregateErrors),
            ("sequence value ordering", SequenceValueOrdering),
            ("stable multi-key sorting", StableMultiKeySorting),
            ("typed item counts", TypedItemCounts),
            ("ordered sequence windows", OrderedSequenceWindows),
            ("generated sequence values", GeneratedSequenceValues),
            ("generated sequence contexts", GeneratedSequenceContexts),
            ("lazy generated sequence contexts", LazyGeneratedSequenceContexts),
            ("typed generated sequence errors", TypedGeneratedSequenceErrors),
            ("recursive generated sequence", RecursiveGeneratedSequence),
        };

        foreach (var test in tests)
        {
            test.Run();
            Console.WriteLine($"PASS {test.Name}");
        }

        Console.WriteLine($"All {tests.Length} smoke tests passed.");
        return 0;
    }

    private static void GroupTraversal()
    {
        var root = Group(
            Field("Company", Group(Field("Name", Scalar(FerruleValue.FromString("Ferrule"))))));

        Equal(
            FerruleValue.FromString("Ferrule"),
            ScalarPathResolver.Resolve(root, "Company", "Name"));
        Equal(FerruleValue.FromInt64(7), ScalarPathResolver.Resolve(Scalar(FerruleValue.FromInt64(7))));
    }

    private static void CopyCurrentGroup()
    {
        var source = Group(
            Field("Null", Scalar(FerruleValue.Null)),
            Field("Nil", Scalar(FerruleValue.XmlNil)),
            Field("Rows", new FerruleRepeated(new[]
            {
                Group(Field("Value", Scalar(Text("row")))),
            })),
            Field("Mapped", new FerruleMappedSequence(new[]
            {
                Scalar(Text("mapped")),
            })),
            Field("Documents", new FerruleDocumentSet(new[]
            {
                new FerruleDocument(
                    "logical.xml",
                    Group(Field("Value", Scalar(Text("document")))),
                    "/resolved/logical.xml"),
            })));

        var copy = ScopeContext.FromSource(source).CopyCurrentGroup();

        Equal(false, ReferenceEquals(source, copy));
        Equal(
            "Null,Nil,Rows,Mapped,Documents",
            string.Join(',', copy.Fields.Select(field => field.Name)));
        Equal(FerruleValue.Null, ScalarPathResolver.Resolve(copy, "Null"));
        Equal(FerruleValue.XmlNil, ScalarPathResolver.Resolve(copy, "Nil"));
        Equal(Text("row"), ScalarPathResolver.Resolve(copy, "Rows", "Value"));
        Equal(false, ReferenceEquals(source.Fields[2].Value, copy.Fields[2].Value));
        var documents = (FerruleDocumentSet)copy.Fields[4].Value;
        Equal("logical.xml", documents.Documents[0].Path);
        Equal("/resolved/logical.xml", documents.Documents[0].ResolvedSourcePath);

        var error = Error(
            FerruleRuntimeError.CopyCurrentSourceRequiresGroup,
            () => ScopeContext.FromSource(Scalar(Text("not a group"))).CopyCurrentGroup());
        Equal("scalar", error.Detail);
    }

    private static void RepeatedFirstItem()
    {
        var root = Group(Field(
            "Rows",
            new FerruleRepeated(new FerruleInstance[]
            {
                Group(Field("Id", Scalar(FerruleValue.FromInt64(11)))),
                Group(Field("Id", Scalar(FerruleValue.FromInt64(22)))),
            })));

        Equal(FerruleValue.FromInt64(11), ScalarPathResolver.Resolve(root, "Rows", "Id"));
        Equal(
            FerruleValue.Null,
            ScalarPathResolver.Resolve(Group(Field("Rows", new FerruleRepeated([]))), "Rows", "Id"));
    }

    private static void DocumentFirstItem()
    {
        var root = new FerruleDocumentSet(new[]
        {
            new FerruleDocument(
                "first.xml",
                Group(Field("Code", Scalar(FerruleValue.FromString("first"))))),
            new FerruleDocument(
                "second.xml",
                Group(Field("Code", Scalar(FerruleValue.FromString("second"))))),
        });

        Equal(FerruleValue.FromString("first"), ScalarPathResolver.Resolve(root, "Code"));
    }

    private static void MappedSequenceIsNotRepeated()
    {
        var sequence = new FerruleMappedSequence(new FerruleInstance[]
        {
            Scalar(FerruleValue.FromBoolean(true)),
            Scalar(FerruleValue.FromBoolean(false)),
        });

        Error(FerruleRuntimeError.MissingSourceField, () => ScalarPathResolver.Resolve(sequence));
    }

    private static void TypedResolverErrors()
    {
        var root = Group(Field("Nested", Group(Field("Value", Scalar(FerruleValue.Null)))));

        Error(
            FerruleRuntimeError.MissingSourceField,
            () => ScalarPathResolver.Resolve(root, "Missing"));
        Error(
            FerruleRuntimeError.MissingSourceField,
            () => ScalarPathResolver.Resolve(root, "Nested"));
        Error(
            FerruleRuntimeError.MissingSourceField,
            () => ScalarPathResolver.Resolve(root, "Nested", "Value", "Child"));
    }

    private static void NullAndXmlNil()
    {
        Equal(FerruleValueKind.Null, FerruleValue.Null.Kind);
        Equal(FerruleValueKind.XmlNil, FerruleValue.XmlNil.Kind);
        NotEqual(FerruleValue.Null, FerruleValue.XmlNil);

        var root = Group(
            Field("Absent", Scalar(FerruleValue.Null)),
            Field("PresentNil", Scalar(FerruleValue.XmlNil)));
        Equal(FerruleValue.Null, ScalarPathResolver.Resolve(root, "Absent"));
        Equal(FerruleValue.XmlNil, ScalarPathResolver.Resolve(root, "PresentNil"));
    }

    private static void DoubleDomain()
    {
        Equal(FerruleValue.FromDouble(12.5), FerruleValue.FromDouble(12.5));
        Equal(FerruleValue.FromDouble(0.0), FerruleValue.FromDouble(-0.0));
        Equal(
            FerruleValue.FromDouble(0.0).GetHashCode(),
            FerruleValue.FromDouble(-0.0).GetHashCode());
        Equal(true, double.IsNaN(FerruleValue.FromDouble(double.NaN).DoubleValue));
        Equal(
            double.PositiveInfinity,
            FerruleValue.FromDouble(double.PositiveInfinity).DoubleValue);
        Equal(
            double.NegativeInfinity,
            FerruleValue.FromDouble(double.NegativeInfinity).DoubleValue);
    }

    private static void FieldOrder()
    {
        var group = Group(
            Field("third", Scalar(FerruleValue.FromInt64(3))),
            Field("first", Scalar(FerruleValue.FromInt64(1))),
            Field("second", Scalar(FerruleValue.FromInt64(2))));

        Equal("third,first,second", string.Join(',', group.Fields.Select(field => field.Name)));
    }

    private static void EmptyFieldNames()
    {
        var root = Group(Field(string.Empty, Scalar(FerruleValue.FromString("empty"))));
        Equal(FerruleValue.FromString("empty"), ScalarPathResolver.Resolve(root, string.Empty));
    }

    private static void ScalarFunctions()
    {
        CallEquals(FerruleValue.FromBoolean(false), "and", Bool(true), Bool(false));
        CallEquals(FerruleValue.FromBoolean(true), "or", Bool(false), Bool(true));
        CallEquals(FerruleValue.FromBoolean(false), "not", Bool(true));
        CallEquals(FerruleValue.FromBoolean(false), "exists", FerruleValue.Null);
        CallEquals(FerruleValue.FromBoolean(true), "exists", FerruleValue.XmlNil);
        CallEquals(FerruleValue.FromBoolean(true), "is_empty", Text(string.Empty));
        CallEquals(FerruleValue.FromBoolean(true), "starts_with", Text("ferrule"), Text("fer"));
        CallEquals(FerruleValue.FromBoolean(true), "contains", Text("ferrule"), Text("rul"));

        CallEquals(
            FerruleValue.FromInt64(42),
            "add",
            FerruleValue.FromInt64(20),
            FerruleValue.FromInt64(10),
            Text(" 12 "));
        CallEquals(
            FerruleValue.FromInt64(42),
            "subtract",
            FerruleValue.FromInt64(50),
            FerruleValue.FromInt64(5),
            FerruleValue.FromInt64(3));
        CallEquals(
            FerruleValue.FromDouble(1.35),
            "multiply",
            Text("0.09"),
            FerruleValue.FromInt64(15));
        CallEquals(
            FerruleValue.FromDouble(2.5),
            "divide",
            FerruleValue.FromInt64(5),
            FerruleValue.FromInt64(2));

        CallEquals(FerruleValue.FromBoolean(true), "equal", FerruleValue.FromInt64(7), FerruleValue.FromDouble(7.0));
        CallEquals(FerruleValue.FromBoolean(false), "not_equal", FerruleValue.Null, FerruleValue.Null);
        CallEquals(FerruleValue.FromBoolean(true), "less_than", Text("2007"), FerruleValue.FromInt64(2008));
        CallEquals(FerruleValue.FromBoolean(true), "greater_than", Bool(true), Bool(false));
        CallEquals(FerruleValue.FromBoolean(true), "less_or_equal", FerruleValue.FromInt64(7), FerruleValue.FromInt64(7));
        CallEquals(FerruleValue.FromBoolean(true), "greater_or_equal", FerruleValue.FromInt64(8), FerruleValue.FromInt64(7));
        CallEquals(FerruleValue.FromBoolean(true), "less_than", Text("\uE000"), Text("\U00010000"));

        var infinite = FerruleFunctions.Call(
            "divide",
            new[] { FerruleValue.FromDouble(double.MaxValue), FerruleValue.FromDouble(double.Epsilon) });
        Equal(FerruleValueKind.Double, infinite.Kind);
        Equal(true, double.IsPositiveInfinity(infinite.DoubleValue));
    }

    private static void TypedFunctionErrors()
    {
        var arity = Error(
            FerruleRuntimeError.FunctionArity,
            () => FerruleFunctions.Call("not", Array.Empty<FerruleValue>()));
        Equal("not", arity.Function);
        Equal(1, arity.ExpectedArity);
        Equal(0, arity.ActualArity);

        var type = Error(
            FerruleRuntimeError.FunctionType,
            () => FerruleFunctions.Call("not", new[] { FerruleValue.FromInt64(1) }));
        Equal("not", type.Function);
        Equal(FerruleValueKind.Int64, type.FoundKind);
        Error(
            FerruleRuntimeError.DivideByZero,
            () => FerruleFunctions.Call(
                "divide",
                new[] { FerruleValue.FromInt64(1), FerruleValue.FromInt64(0) }));
        var overflow = Error(
            FerruleRuntimeError.IntegerOverflow,
            () => FerruleFunctions.Call(
                "add",
                new[] { FerruleValue.FromInt64(long.MaxValue), FerruleValue.FromInt64(1) }));
        Equal("add", overflow.Function);

        var unknown = Error(
            FerruleRuntimeError.UnknownFunction,
            () => FerruleFunctions.Call("missing", Array.Empty<FerruleValue>()));
        Equal("missing", unknown.Function);

        var notBoolean = Error(
            FerruleRuntimeError.NotABool,
            () => FerruleFunctions.RequireBoolean(Text("true"), 1));
        Equal((uint)1, notBoolean.Node);
        Equal(FerruleValueKind.String, notBoolean.FoundKind);
    }

    private static void ScopeSourceIteration()
    {
        var item = (string sku) => Group(Field("Sku", Scalar(Text(sku))));
        var order = (string customer, string code, FerruleInstance[] items) => Group(
            Field("Customer", Scalar(Text(customer))),
            Field("OrderCode", Scalar(Text(code))),
            Field("Items", new FerruleRepeated(items)));
        var row = (string name, FerruleInstance[] orders) => Group(
            Field("RootName", Scalar(Text(name))),
            Field("Orders", new FerruleRepeated(orders)));
        var source = new FerruleRepeated(new FerruleInstance[]
        {
            row("first-root", new FerruleInstance[]
            {
                order("Ada", "A", new FerruleInstance[] { item("A-1"), item("A-2") }),
                order("Lin", "B", new FerruleInstance[] { item("B-1") }),
            }),
            row("second-root", new FerruleInstance[]
            {
                order("Grace", "C", new FerruleInstance[] { item("C-1") }),
            }),
        });

        var context = ScopeContext.FromSource(source);
        var candidates = context.IterateSource("Orders", "Items");
        Equal(4, candidates.Count);
        Equal(Text("A-1"), candidates[0].ResolveScalar("Sku"));
        Equal(Text("Ada"), candidates[1].ResolveScalar("Customer"));
        Equal(Text("B"), candidates[2].ResolveScalar("Orders", "OrderCode"));
        Equal(Text("B-1"), candidates[2].ResolveScalar("Orders", "Items", "Sku"));
        Equal(Text("second-root"), candidates[3].ResolveScalar("RootName"));
        Equal(2L, candidates[1].Position(new[] { "Items" }));
        Equal(1L, candidates[2].Position(Array.Empty<string>()));
        Equal(1L, candidates[2].Position(new[] { "Missing" }));
        Equal(
            Text("Lin"),
            candidates[2].ResolveScalarInFrame(
                new[] { "Source", "Orders" },
                new[] { "Customer" }));
        Equal(
            Text("B-1"),
            candidates[2].ResolveScalarInFrame(
                new[] { "Orders", "Items" },
                new[] { "Sku" }));
        Error(
            FerruleRuntimeError.MissingSourceField,
            () => candidates[2].ResolveScalarInFrame(
                new[] { "Missing" },
                new[] { "Customer" }));

        var compacted = candidates[2].WithCompactedPosition(3);
        Equal(1L, candidates[2].Position(new[] { "Items" }));
        Equal(3L, compacted.Position(new[] { "Items" }));
        Equal(2L, compacted.Position(new[] { "Orders" }));

        var rows = context.IterateSource();
        Equal(2, rows.Count);
        Equal(Text("first-root"), rows[0].ResolveScalar("RootName"));
        Equal(1, ScopeContext.FromSource(Group()).IterateSource().Count);
        Equal(
            1,
            ScopeContext.FromSource(new FerruleMappedSequence(new[] { item("mapped") }))
                .IterateSource()
                .Count);

        var terminal = ScopeContext
            .FromSource(Group(Field("Value", Scalar(FerruleValue.FromInt64(7)))))
            .IterateSource("Value");
        Equal(1, terminal.Count);
        Equal(FerruleValue.FromInt64(7), terminal[0].ResolveScalar());
        var terminalGroup = ScopeContext
            .FromSource(Group(Field("Address", Group(Field("City", Scalar(Text("Paris")))))))
            .IterateSource("Address");
        Equal(1, terminalGroup.Count);
        Equal(Text("Paris"), terminalGroup[0].ResolveScalar("City"));
        Equal(0, context.IterateSource("Missing").Count);
        Equal(
            0,
            ScopeContext.FromSource(Group(Field("Empty", new FerruleRepeated([]))))
                .IterateSource("Empty")
                .Count);

        var shadow = ScopeContext.FromSource(Group(
            Field("Options", new FerruleRepeated(new[]
            {
                Group(Field("Name", Scalar(Text("outer")))),
            })),
            Field("Rows", new FerruleRepeated(new[]
            {
                Group(Field("Options", new FerruleRepeated([]))),
            }))));
        var shadowedItem = shadow.IterateSource("Rows")[0];
        Equal(FerruleValue.Null, shadowedItem.ResolveScalar("Options", "Name"));
        Error(
            FerruleRuntimeError.MissingSourceField,
            () => shadowedItem.ResolveScalar("Missing"));

        var documents = ScopeContext.FromSource(new FerruleDocumentSet(new[]
        {
            new FerruleDocument(
                "a.xml",
                Group(
                    Field("Document", Scalar(Text("a"))),
                    Field("Rows", new FerruleRepeated(new[] { item("doc-a") })))),
            new FerruleDocument(
                "b.xml",
                Group(
                    Field("Document", Scalar(Text("b"))),
                    Field("Rows", new FerruleRepeated(new[] { item("doc-b") })))),
        })).IterateSource("Rows");
        Equal(2, documents.Count);
        Equal(Text("a"), documents[0].ResolveScalar("Document"));
        Equal(Text("doc-b"), documents[1].ResolveScalar("Rows", "Sku"));
    }

    private static void AggregateSourceContexts()
    {
        var contact = (string name) => Group(
            Field("Name", Scalar(Text(name))),
            Field("Nested", new FerruleRepeated(new[] { Scalar(Text($"nested-{name}")) })));
        var office = (string code, params FerruleInstance[] contacts) => Group(
            Field("Code", Scalar(Text(code))),
            Field("Contacts", new FerruleRepeated(contacts)));
        var source = Group(
            Field("Outside", Scalar(Text("outer"))),
            Field("Offices", new FerruleRepeated(new FerruleInstance[]
            {
                office("A", contact("Ana"), contact("Bo")),
                office("B", contact("Cy")),
            })));

        var context = ScopeContext.FromSource(source);
        var contacts = context.AggregateItems("Offices", "Contacts");
        Equal(3, contacts.Count);
        Equal(Text("Ana"), contacts[0].AggregateCurrentScalar("Name"));
        Equal(Text("Cy"), contacts[2].AggregateCurrentScalar("Name"));
        Equal(1L, contacts[0].Position(new[] { "Offices" }));
        Equal(2L, contacts[1].Position(new[] { "Contacts" }));
        Equal(2L, contacts[2].Position(new[] { "Offices" }));
        Equal(1L, contacts[2].Position(new[] { "Contacts" }));
        Equal(FerruleValue.Null, contacts[0].AggregateCurrentScalar("Outside"));
        Equal(FerruleValue.Null, contacts[0].AggregateCurrentScalar("Nested"));
        Equal(FerruleValue.Null, contacts[0].AggregateCurrentScalar("Missing"));
        Equal(0, context.AggregateItems("Missing").Count);
        Equal(0, context.AggregateItems().Count);

        var rows = new FerruleRepeated(new FerruleInstance[]
        {
            Group(Field("Value", Scalar(FerruleValue.FromInt64(11)))),
            Group(Field("Value", Scalar(FerruleValue.FromInt64(22)))),
        });
        var rowContext = ScopeContext.FromSource(rows);
        var allRows = rowContext.AggregateItems();
        Equal(2, allRows.Count);
        Equal(FerruleValue.FromInt64(22), allRows[1].AggregateCurrentScalar("Value"));
        Equal(2L, allRows[1].Position(Array.Empty<string>()));

        var scalarItems = ScopeContext
            .FromSource(new FerruleRepeated(new[]
            {
                Scalar(Text("one")),
                Scalar(Text("two")),
            }))
            .AggregateItems();
        Equal(Text("two"), scalarItems[1].AggregateCurrentScalar());

        var currentRow = rowContext.IterateSource()[0];
        var rowsFromCurrentItem = currentRow.AggregateItems();
        Equal(2, rowsFromCurrentItem.Count);
        Equal(FerruleValue.FromInt64(22), rowsFromCurrentItem[1].AggregateCurrentScalar("Value"));

        var documents = ScopeContext.FromSource(new FerruleDocumentSet(new[]
        {
            new FerruleDocument("first.xml", Group(Field("Value", Scalar(Text("first"))))),
            new FerruleDocument("second.xml", Group(Field("Value", Scalar(Text("second"))))),
        })).AggregateItems();
        Equal(2, documents.Count);
        Equal(Text("second"), documents[1].AggregateCurrentScalar("Value"));
    }

    private static void AggregateReductions()
    {
        Equal(
            FerruleValue.FromInt64(3),
            Aggregate(
                FerruleAggregateOperation.Count,
                FerruleValue.Null,
                FerruleValue.XmlNil,
                Text("ignored")));
        Equal(
            FerruleValue.FromDouble(2.5),
            Aggregate(
                FerruleAggregateOperation.Sum,
                Text("2"),
                FerruleValue.FromDouble(0.5),
                Text("not numeric"),
                FerruleValue.Null,
                FerruleValue.XmlNil,
                Bool(true)));
        Equal(
            FerruleValue.FromDouble(2.5),
            Aggregate(
                FerruleAggregateOperation.Avg,
                FerruleValue.FromInt64(2),
                Text("3"),
                Text("ignored")));
        Equal(
            Text("a||true|2|100000000000000000000|b"),
            FerruleAggregates.Apply(
                FerruleAggregateOperation.Join,
                new[]
                {
                    Text("a"),
                    FerruleValue.Null,
                    FerruleValue.XmlNil,
                    Bool(true),
                    FerruleValue.FromInt64(2),
                    FerruleValue.FromDouble(1e20),
                    Text("b"),
                },
                Text("|")));
        Equal(
            Text("ab"),
            FerruleAggregates.Apply(
                FerruleAggregateOperation.Join,
                new[] { Text("a"), Text("b") }));

        var items = new[] { Text("first"), Text("second"), Text("third") };
        Equal(
            Text("second"),
            FerruleAggregates.Apply(
                FerruleAggregateOperation.ItemAt,
                items,
                FerruleValue.FromDouble(1.5)));
        Equal(
            Text("third"),
            FerruleAggregates.Apply(
                FerruleAggregateOperation.ItemAt,
                items,
                FerruleValue.FromDouble(2.5)));
        Equal(
            Text("second"),
            FerruleAggregates.Apply(FerruleAggregateOperation.ItemAt, items, Text(" 2 ")));
        Equal(
            FerruleValue.Null,
            FerruleAggregates.Apply(FerruleAggregateOperation.ItemAt, items, Text("2.0")));
        Equal(
            FerruleValue.Null,
            FerruleAggregates.Apply(
                FerruleAggregateOperation.ItemAt,
                items,
                FerruleValue.FromDouble(double.PositiveInfinity)));

        var empty = Array.Empty<FerruleValue>();
        Equal(FerruleValue.FromInt64(0), FerruleAggregates.Apply(FerruleAggregateOperation.Count, empty));
        Equal(FerruleValue.FromInt64(0), FerruleAggregates.Apply(FerruleAggregateOperation.Sum, empty));
        Equal(FerruleValue.Null, FerruleAggregates.Apply(FerruleAggregateOperation.Avg, empty));
        Equal(FerruleValue.Null, FerruleAggregates.Apply(FerruleAggregateOperation.Min, empty));
        Equal(FerruleValue.Null, FerruleAggregates.Apply(FerruleAggregateOperation.Max, empty));
        Equal(Text(string.Empty), FerruleAggregates.Apply(FerruleAggregateOperation.Join, empty));
        Equal(FerruleValue.Null, FerruleAggregates.Apply(FerruleAggregateOperation.ItemAt, empty));
    }

    private static void AggregateNumericPrecision()
    {
        Equal(
            FerruleValue.FromInt64(9_007_199_254_740_993),
            Aggregate(
                FerruleAggregateOperation.Sum,
                FerruleValue.FromInt64(9_007_199_254_740_992),
                FerruleValue.FromInt64(1)));
        Equal(
            FerruleValue.FromDouble(double.MaxValue),
            Aggregate(
                FerruleAggregateOperation.Avg,
                FerruleValue.FromDouble(double.MaxValue),
                FerruleValue.FromDouble(double.MaxValue)));
        Equal(
            FerruleValue.FromDouble(double.MaxValue),
            Aggregate(
                FerruleAggregateOperation.Sum,
                FerruleValue.FromDouble(double.MaxValue),
                FerruleValue.FromDouble(double.MaxValue),
                FerruleValue.FromDouble(-double.MaxValue)));
        Equal(
            FerruleValue.FromDouble(11.375),
            Aggregate(
                FerruleAggregateOperation.Avg,
                13.6, 15.6, 16.2, 10.0, 7.3, 7.6, 13.6, 7.1));

        var mixed = new[]
        {
            FerruleValue.FromInt64(9_007_199_254_740_993),
            FerruleValue.FromDouble(9_007_199_254_740_992.0),
        };
        Equal(
            FerruleValue.FromDouble(9_007_199_254_740_992.0),
            FerruleAggregates.Apply(FerruleAggregateOperation.Min, mixed));
        Equal(
            FerruleValue.FromInt64(9_007_199_254_740_993),
            FerruleAggregates.Apply(FerruleAggregateOperation.Max, mixed));
        Equal(
            FerruleValue.FromDouble(2.5),
            Aggregate(FerruleAggregateOperation.Min, Text("10"), Text("2.5")));
        Equal(
            FerruleValue.FromInt64(10),
            Aggregate(FerruleAggregateOperation.Max, Text("10"), Text("2.5")));

        foreach (var operation in new[] { FerruleAggregateOperation.Min, FerruleAggregateOperation.Max })
        {
            var negativeFirst = FerruleAggregates.Apply(
                operation,
                new[]
                {
                    FerruleValue.FromDouble(-0.0),
                    FerruleValue.FromDouble(0.0),
                    FerruleValue.FromInt64(0),
                });
            Equal(
                BitConverter.DoubleToInt64Bits(-0.0),
                BitConverter.DoubleToInt64Bits(negativeFirst.DoubleValue));

            var positiveFirst = FerruleAggregates.Apply(
                operation,
                new[]
                {
                    FerruleValue.FromDouble(0.0),
                    FerruleValue.FromDouble(-0.0),
                    FerruleValue.FromInt64(0),
                });
            Equal(
                BitConverter.DoubleToInt64Bits(0.0),
                BitConverter.DoubleToInt64Bits(positiveFirst.DoubleValue));
        }
    }

    private static void TypedAggregateErrors()
    {
        var overflow = Error(
            FerruleRuntimeError.AggregateIntegerOverflow,
            () => Aggregate(
                FerruleAggregateOperation.Sum,
                FerruleValue.FromInt64(long.MaxValue),
                FerruleValue.FromInt64(1)));
        Equal(FerruleAggregateOperation.Sum, overflow.AggregateOperation);

        foreach (var operation in new[]
        {
            FerruleAggregateOperation.Sum,
            FerruleAggregateOperation.Avg,
            FerruleAggregateOperation.Min,
            FerruleAggregateOperation.Max,
        })
        {
            var direct = Error(
                FerruleRuntimeError.AggregateNonFinite,
                () => Aggregate(operation, FerruleValue.FromDouble(double.PositiveInfinity)));
            Equal(operation, direct.AggregateOperation);
            var lexical = Error(
                FerruleRuntimeError.AggregateNonFinite,
                () => Aggregate(operation, Text("inf")));
            Equal(operation, lexical.AggregateOperation);
        }

        var produced = Error(
            FerruleRuntimeError.AggregateNonFinite,
            () => Aggregate(
                FerruleAggregateOperation.Sum,
                FerruleValue.FromDouble(double.MaxValue),
                FerruleValue.FromDouble(double.MaxValue)));
        Equal(FerruleAggregateOperation.Sum, produced.AggregateOperation);

        var nan = Error(
            FerruleRuntimeError.AggregateNonFinite,
            () => Aggregate(FerruleAggregateOperation.Avg, Text("NaN")));
        Equal(FerruleAggregateOperation.Avg, nan.AggregateOperation);
    }

    private static void SequenceValueOrdering()
    {
        Equal(-1, FerruleSequences.CompareValues(FerruleValue.Null, Text("value")));
        Equal(1, FerruleSequences.CompareValues(FerruleValue.XmlNil, FerruleValue.Null));
        Equal(null, FerruleSequences.CompareValues(FerruleValue.XmlNil, FerruleValue.XmlNil));
        Equal(null, FerruleSequences.CompareValues(FerruleValue.FromDouble(double.NaN), FerruleValue.FromDouble(1)));
        Equal(0, FerruleSequences.CompareValues(FerruleValue.FromDouble(-0.0), FerruleValue.FromInt64(0)));
        Equal(
            1,
            FerruleSequences.CompareValues(
                FerruleValue.FromInt64(9_007_199_254_740_993),
                FerruleValue.FromDouble(9_007_199_254_740_992.0)));
        Equal(
            -1,
            FerruleSequences.CompareValues(
                FerruleValue.FromInt64(long.MaxValue),
                FerruleValue.FromDouble((double)long.MaxValue)));
        Equal(-1, FerruleSequences.CompareValues(Text("\uE000"), Text("\U00010000")));
        Equal(-1, FerruleSequences.CompareValues(Bool(false), Bool(true)));
        Equal(null, FerruleSequences.CompareValues(Text("1"), FerruleValue.FromInt64(1)));
    }

    private static void StableMultiKeySorting()
    {
        var candidates = new[]
        {
            new SortCandidate("A", FerruleValue.Null, Text("unused")),
            new SortCandidate("B", FerruleValue.FromInt64(5), Text("\uE000")),
            new SortCandidate("C", FerruleValue.FromDouble(5.0), Text("a")),
            new SortCandidate("D", FerruleValue.FromInt64(3), Text("unused")),
            new SortCandidate("E", FerruleValue.FromInt64(5), Text("a")),
            new SortCandidate("F", FerruleValue.FromInt64(5), Text("\U00010000")),
        };
        var evaluations = new List<string>();
        var sorted = FerruleSequences.StableSort(
            candidates,
            new FerruleSortKey<SortCandidate>[]
            {
                new(candidate =>
                {
                    evaluations.Add($"{candidate.Name}:score");
                    return candidate.Score;
                }, Descending: true),
                new(candidate =>
                {
                    evaluations.Add($"{candidate.Name}:tie");
                    return candidate.Tie;
                }),
            });
        Equal("C,E,B,F,D,A", string.Join(',', sorted.Select(candidate => candidate.Name)));
        Equal(
            "A:score,A:tie,B:score,B:tie,C:score,C:tie,D:score,D:tie,E:score,E:tie,F:score,F:tie",
            string.Join(',', evaluations));

        var incomparable = FerruleSequences.StableSort(
            candidates.Take(3).ToArray(),
            new[]
            {
                new FerruleSortKey<SortCandidate>(
                    _ => FerruleValue.XmlNil),
            });
        Equal("A,B,C", string.Join(',', incomparable.Select(candidate => candidate.Name)));
        Equal(0, FerruleSequences.StableSort(candidates, Array.Empty<FerruleSortKey<SortCandidate>>()).Count - candidates.Length);
    }

    private static void TypedItemCounts()
    {
        Equal(7UL, FerruleSequences.ItemCount(1, FerruleValue.FromInt64(7)));
        Equal(0UL, FerruleSequences.ItemCount(2, FerruleValue.FromInt64(-7)));
        Equal(2UL, FerruleSequences.ItemCount(3, FerruleValue.FromDouble(2.9)));
        Equal(0UL, FerruleSequences.ItemCount(4, FerruleValue.FromDouble(-2.9)));
        Equal((ulong)long.MaxValue, FerruleSequences.ItemCount(5, FerruleValue.FromDouble(double.MaxValue)));
        Equal(42UL, FerruleSequences.ItemCount(6, Text("  +42 ")));

        var boolean = Error(
            FerruleRuntimeError.NotAnItemCount,
            () => FerruleSequences.ItemCount(91, Bool(true)));
        Equal((uint)91, boolean.Node);
        Equal(FerruleValueKind.Bool, boolean.FoundKind);
        foreach (var invalid in new[]
        {
            FerruleValue.Null,
            FerruleValue.XmlNil,
            FerruleValue.FromDouble(double.NaN),
            FerruleValue.FromDouble(double.PositiveInfinity),
            Text("2.0"),
            Text(""),
        })
        {
            var exception = Error(
                FerruleRuntimeError.NotAnItemCount,
                () => FerruleSequences.ItemCount(92, invalid));
            Equal((uint)92, exception.Node);
            Equal(invalid.Kind, exception.FoundKind);
        }
    }

    private static void OrderedSequenceWindows()
    {
        var items = Enumerable.Range(1, 8).ToArray();
        WindowEquals("3,4,5,6,7,8", items, FerruleSequenceWindow.SkipFirst(2));
        WindowEquals("1,2", items, FerruleSequenceWindow.First(2));
        WindowEquals("3,4,5,6,7,8", items, FerruleSequenceWindow.From(3));
        WindowEquals("3,4", items, FerruleSequenceWindow.FromTo(3, 4));
        WindowEquals("7,8", items, FerruleSequenceWindow.Last(2));
        WindowEquals("1,2,3,4,5,6,7,8", items, FerruleSequenceWindow.From(0));
        WindowEquals(string.Empty, items, FerruleSequenceWindow.FromTo(3, 1));
        WindowEquals(string.Empty, items, FerruleSequenceWindow.SkipFirst(ulong.MaxValue));
        WindowEquals("1,2,3,4,5,6,7,8", items, FerruleSequenceWindow.Last(ulong.MaxValue));
        WindowEquals(
            "3,4,5",
            items,
            FerruleSequenceWindow.SkipFirst(2),
            FerruleSequenceWindow.First(3));
        WindowEquals(
            "3",
            items,
            FerruleSequenceWindow.First(3),
            FerruleSequenceWindow.SkipFirst(2));
        WindowEquals(string.Empty, Array.Empty<int>(), FerruleSequenceWindow.Last(2));
    }

    private static void GeneratedSequenceValues()
    {
        Equal(
            "a||b|",
            string.Join('|', FerruleSequences.Tokenize(Text("a,,b,"), Text(",")).Select(ValueText)));
        Equal(
            string.Empty,
            string.Join('|', FerruleSequences.TokenizeByLength(Text(string.Empty), FerruleValue.FromInt64(2))));
        Equal(
            "aé|🙂z",
            string.Join('|', FerruleSequences.TokenizeByLength(Text("aé🙂z"), FerruleValue.FromDouble(2.9)).Select(ValueText)));
        Equal(
            "1,2,3",
            string.Join(',', FerruleSequences.GenerateRange(null, FerruleValue.FromInt64(3)).Select(ValueText)));
        Equal(
            "-2,-1,0",
            string.Join(',', FerruleSequences.GenerateRange(Text("-2.0"), FerruleValue.FromDouble(0.0)).Select(ValueText)));
        Equal(
            0,
            FerruleSequences.GenerateRange(FerruleValue.FromInt64(3), FerruleValue.FromInt64(2)).Count);
    }

    private static void GeneratedSequenceContexts()
    {
        var parent = ScopeContext.FromSource(Group(Field("Parent", Scalar(Text("outer")))));
        var contexts = parent.IterateGenerated(new[] { Text("first"), Text("second") });
        Equal(2, contexts.Count);
        Equal(Text("first"), contexts[0].ResolveScalar());
        Equal(Text("outer"), contexts[0].ResolveScalar("Parent"));
        Equal(1L, contexts[0].Position());
        Equal(2L, contexts[1].Position());
        Equal(7L, contexts[1].WithCompactedPosition(7).Position());
    }

    private static void LazyGeneratedSequenceContexts()
    {
        var parent = ScopeContext.FromSource(Group(Field("Parent", Scalar(Text("outer")))));
        var yielded = 0;
        foreach (var context in parent.EnumerateGenerated(new FirstOnlyValues()))
        {
            Equal(Text("first"), context.ResolveScalar());
            Equal(Text("outer"), context.ResolveScalar("Parent"));
            Equal(1L, context.Position());
            yielded++;
            break;
        }
        Equal(1, yielded);
    }

    private static void TypedGeneratedSequenceErrors()
    {
        var wrongType = Error(
            FerruleRuntimeError.FunctionType,
            () => FerruleSequences.Tokenize(FerruleValue.XmlNil, Text(",")));
        Equal("tokenize", wrongType.Function);
        Equal(FerruleValueKind.XmlNil, wrongType.FoundKind);

        var delimiter = Error(
            FerruleRuntimeError.FunctionInvalidArgument,
            () => FerruleSequences.Tokenize(Text("a"), Text(string.Empty)));
        Equal("requires a non-empty delimiter", delimiter.Detail);
        Error(
            FerruleRuntimeError.FunctionInvalidArgument,
            () => FerruleSequences.TokenizeByLength(Text("abc"), Text("2.0")));

        var tooLarge = Error(
            FerruleRuntimeError.GeneratedSequenceTooLarge,
            () => FerruleSequences.GenerateRange(
                FerruleValue.FromInt64(long.MinValue),
                FerruleValue.FromInt64(long.MaxValue)));
        Equal((UInt128)ulong.MaxValue + 1, tooLarge.RequestedItems);
        Equal((UInt128)FerruleSequences.MaximumGeneratedSequenceItems, tooLarge.MaximumItems);
    }

    private static void RecursiveGeneratedSequence()
    {
        static FerruleInstance DirectoryNode(
            string name,
            IReadOnlyList<string> files,
            IReadOnlyList<FerruleInstance> children) => Group(
                Field("name", Scalar(Text(name))),
                Field(
                    "file",
                    new FerruleRepeated(files.Select(file =>
                        Group(Field("name", Scalar(Text(file))))))),
                Field("directory", new FerruleRepeated(children)));

        var source = DirectoryNode(
            "root",
            new[] { "top.txt", "second.txt" },
            new[]
            {
                DirectoryNode(
                    "child",
                    new[] { "nested.txt" },
                    Array.Empty<FerruleInstance>()),
            });
        var values = FerruleSequences.RecursiveCollect(
            ScopeContext.FromSource(source),
            Array.Empty<string>(),
            new[] { "directory" },
            new[] { "name" },
            new[] { "file" },
            new[] { "name" },
            string.Empty,
            "\\");

        Equal(3, values.Count);
        Equal(Text("\\root\\top.txt"), values[0]);
        Equal(Text("\\root\\second.txt"), values[1]);
        Equal(Text("\\root\\child\\nested.txt"), values[2]);
        Equal(string.Empty, FerruleSequences.RecursiveCollectArgumentText(FerruleValue.Null));
        Equal("false", FerruleSequences.RecursiveCollectArgumentText(Bool(false)));
        Error(
            FerruleRuntimeError.FunctionType,
            () => FerruleSequences.RecursiveCollectArgumentText(FerruleValue.XmlNil));
        Throws<ArgumentNullException>(() => FerruleSequences.RecursiveCollect(
            ScopeContext.FromSource(source),
            new string[] { null! },
            new[] { "directory" },
            new[] { "name" },
            new[] { "file" },
            new[] { "name" },
            string.Empty,
            "\\"));
    }

    private static string ValueText(FerruleValue value) => value.ToString();

    private static void WindowEquals(
        string expected,
        IReadOnlyList<int> items,
        params FerruleSequenceWindow[] windows) =>
        Equal(expected, string.Join(',', FerruleSequences.ApplyWindows(items, windows)));

    private static FerruleValue Aggregate(
        FerruleAggregateOperation operation,
        params FerruleValue[] values) =>
        FerruleAggregates.Apply(operation, values);

    private static FerruleValue Aggregate(
        FerruleAggregateOperation operation,
        params double[] values) =>
        FerruleAggregates.Apply(
            operation,
            values.Select(FerruleValue.FromDouble).ToArray());

    private static void CallEquals(
        FerruleValue expected,
        string function,
        params FerruleValue[] arguments) =>
        Equal(expected, FerruleFunctions.Call(function, arguments));

    private static FerruleValue Bool(bool value) => FerruleValue.FromBoolean(value);

    private static FerruleValue Text(string value) => FerruleValue.FromString(value);

    private static FerruleScalar Scalar(FerruleValue value) => new(value);

    private static FerruleField Field(string name, FerruleInstance value) => new(name, value);

    private static FerruleGroup Group(params FerruleField[] fields) => new(fields);

    private static FerruleRuntimeException Error(FerruleRuntimeError expected, Action action)
    {
        try
        {
            action();
        }
        catch (FerruleRuntimeException exception)
        {
            Equal(expected, exception.Error);
            return exception;
        }

        throw new InvalidOperationException($"Expected Ferrule runtime error {expected}.");
    }

    private static void Throws<TException>(Action action)
        where TException : Exception
    {
        try
        {
            action();
        }
        catch (TException)
        {
            return;
        }

        throw new InvalidOperationException($"Expected {typeof(TException).Name}.");
    }

    private static void Equal<T>(T expected, T actual)
    {
        if (!EqualityComparer<T>.Default.Equals(expected, actual))
        {
            throw new InvalidOperationException($"Expected '{expected}', found '{actual}'.");
        }
    }

    private static void NotEqual<T>(T left, T right)
    {
        if (EqualityComparer<T>.Default.Equals(left, right))
        {
            throw new InvalidOperationException($"Expected '{left}' and '{right}' to differ.");
        }
    }

    private sealed record SortCandidate(
        string Name,
        FerruleValue Score,
        FerruleValue Tie);

    private sealed class FirstOnlyValues : IReadOnlyList<FerruleValue>
    {
        public int Count => 2;

        public FerruleValue this[int index] => index switch
        {
            0 => Text("first"),
            _ => throw new InvalidOperationException("lazy enumeration read a later value"),
        };

        public IEnumerator<FerruleValue> GetEnumerator()
        {
            yield return this[0];
            yield return this[1];
        }

        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() =>
            GetEnumerator();
    }
}
