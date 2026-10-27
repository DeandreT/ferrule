using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void DocumentCollections()
    {
        var first = new FerruleDocument("first.xml", Group(Field("Name", Scalar(Text("first")))));
        var last = new FerruleDocument("last.xml", Group(Field("Name", Scalar(Text("last")))));
        var result = FerruleDynamicDocuments.Collect(new FerruleInstance[]
        {
            new FerruleDocumentSet(new[] { first }),
            new FerruleDocumentSet(Array.Empty<FerruleDocument>()),
            new FerruleDocumentSet(new[] { last }),
        });
        Equal(2, result.Documents.Count);
        Equal(true, ReferenceEquals(first, result.Documents[0]));
        Equal(true, ReferenceEquals(last, result.Documents[1]));
        Equal(0, FerruleDynamicDocuments.Collect(Array.Empty<FerruleInstance>()).Documents.Count);
        var wrong = Error(FerruleRuntimeError.InvalidDocumentCollectionItem,
            () => FerruleDynamicDocuments.Collect(new[] { Scalar(Text("wrong")) }));
        Equal("a scalar", wrong.FoundInstance);
        var late = new InvalidOperationException("late collection failure");
        IEnumerable<FerruleInstance> Failed()
        {
            yield return new FerruleDocumentSet(new[] { first });
            throw late;
        }
        try
        {
            FerruleDynamicDocuments.Collect(Failed());
            throw new Exception("A partial document list escaped a late failure.");
        }
        catch (InvalidOperationException actual)
        {
            Equal(true, ReferenceEquals(late, actual));
        }
    }
}
