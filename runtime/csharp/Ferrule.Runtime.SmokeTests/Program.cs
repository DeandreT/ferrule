using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static class Program
{
    private static int Main()
    {
        var tests = new (string Name, Action Run)[]
        {
            ("group traversal", GroupTraversal),
            ("repeated first item", RepeatedFirstItem),
            ("document first item", DocumentFirstItem),
            ("mapped sequence remains non-scalar", MappedSequenceIsNotRepeated),
            ("typed resolver errors", TypedResolverErrors),
            ("null and XML nil", NullAndXmlNil),
            ("double domain", DoubleDomain),
            ("field order", FieldOrder),
            ("empty field names", EmptyFieldNames),
            ("scalar functions", ScalarFunctions),
            ("typed function errors", TypedFunctionErrors),
            ("scope source iteration", ScopeSourceIteration),
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
}
