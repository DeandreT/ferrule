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
            ("finite doubles", FiniteDoubles),
            ("field order", FieldOrder),
            ("empty field names", EmptyFieldNames),
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

    private static void FiniteDoubles()
    {
        Equal(FerruleValue.FromDouble(12.5), FerruleValue.FromDouble(12.5));
        Error(FerruleRuntimeError.NonFiniteDouble, () => FerruleValue.FromDouble(double.NaN));
        Error(
            FerruleRuntimeError.NonFiniteDouble,
            () => FerruleValue.FromDouble(double.PositiveInfinity));
        Error(
            FerruleRuntimeError.NonFiniteDouble,
            () => FerruleValue.FromDouble(double.NegativeInfinity));
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

    private static FerruleScalar Scalar(FerruleValue value) => new(value);

    private static FerruleField Field(string name, FerruleInstance value) => new(name, value);

    private static FerruleGroup Group(params FerruleField[] fields) => new(fields);

    private static void Error(FerruleRuntimeError expected, Action action)
    {
        try
        {
            action();
        }
        catch (FerruleRuntimeException exception)
        {
            Equal(expected, exception.Error);
            return;
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
