using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void CollectionFindContexts()
    {
        var source = Group(Field(
            "Departments",
            Repeated(
                Department("Remote", Person("Ada")),
                Department("HQ", Person("Grace"), Person("Lin")))));
        var context = ScopeContext.FromSource(source);
        var people = context.CollectionFindItems("Departments", "People");

        Equal(3, people.Count);
        Equal(Text("Ada"), people[0].ResolveScalar("Name"));
        Equal(Text("Grace"), people[1].ResolveScalar("Name"));
        Equal(Text("Lin"), people[2].ResolveScalar("Name"));
        Equal(1L, people[0].Position("Departments"));
        Equal(2L, people[1].Position("Departments"));
        Equal(1L, people[1].Position("Departments", "People"));
        Equal(2L, people[2].Position("Departments", "People"));

        var repeatedRoot = ScopeContext.FromSource(Repeated(
            Scalar(Text("first")),
            Scalar(Text("second"))));
        var rootItems = repeatedRoot.CollectionFindItems();
        Equal(2, rootItems.Count);
        Equal(Text("second"), rootItems[1].ResolveScalar());
        Equal(2L, rootItems[1].Position());

        var documents = ScopeContext.FromSource(new FerruleDocumentSet(new[]
        {
            new FerruleDocument(
                "first.xml",
                Group(Field("Rows", Repeated(Scalar(Text("first")))))),
            new FerruleDocument(
                "second.xml",
                Group(Field("Rows", Repeated(Scalar(Text("second")))))),
        }));
        var documentRows = documents.CollectionFindItems("Rows");
        Equal(1, documentRows.Count);
        Equal(Text("first"), documentRows[0].ResolveScalar());
        Error(FerruleRuntimeError.MissingSourceField, () => documents.CollectionFindItems());

        var catalog = Group(Field(
            "Rows",
            Repeated(Person("Named first"), Person("Named second"))));
        var withNamedSource = ScopeContext.FromSources(
            Group(),
            new[] { Field("catalog", catalog) });
        var namedRows = withNamedSource.CollectionFindItems("catalog", "Rows");
        Equal(2, namedRows.Count);
        Equal(Text("Named second"), namedRows[1].ResolveScalar("Name"));
        Equal(2L, namedRows[1].Position("catalog", "Rows"));

        Error(
            FerruleRuntimeError.MissingSourceField,
            () => context.CollectionFindItems("Missing"));
        Throws<ArgumentNullException>(() => context.CollectionFindItems(null!));
        Throws<ArgumentNullException>(() => context.CollectionFindItems(new string[] { null! }));
    }

    private static FerruleGroup Department(string office, params FerruleInstance[] people) =>
        Group(
            Field("Office", Scalar(Text(office))),
            Field("People", Repeated(people)));

    private static FerruleGroup Person(string name) =>
        Group(Field("Name", Scalar(Text(name))));
}
