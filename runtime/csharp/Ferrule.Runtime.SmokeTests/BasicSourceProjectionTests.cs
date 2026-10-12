using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void BasicSourceProjectionSnapshot()
    {
        var upper = Field("A", Scalar(Text("upper")));
        var lower = Field("a", Scalar(Text("lower")));
        var supplied = new[] { upper, lower };
        var slots = new OwnedCounter(4, FerruleOwnedSourceLimitKind.MetadataSlots);
        var projection = FerruleGroup.FromBasicSourceProjection(supplied, 2, slots);
        Equal<nuint>(4, slots.Used); Equal(2, projection.Fields.Count);
        Equal("A", projection.Fields[0].Name); Equal("a", projection.Fields[1].Name);
        Equal(true, ReferenceEquals(upper, projection.Fields[0]));
        Equal(true, ReferenceEquals(lower, projection.Fields[1]));
        supplied[0] = Field("changed", Scalar(Text("changed")));
        Equal("A", projection.Fields[0].Name);
        Equal(true, projection.TryGetField("A", out var a)); Equal(Text("upper"), ((FerruleScalar)a!).Value);
        Equal(true, projection.TryGetField("a", out var small)); Equal(Text("lower"), ((FerruleScalar)small!).Value);
        Equal(false, projection.TryGetField("changed", out _));
        Equal(false, projection.TryGetField("UNKNOWN", out var missing)); Equal(true, missing is null);
        ArgumentNullException? nullKey = null;
        try { projection.TryGetField(null!, out _); }
        catch (ArgumentNullException e) { nullKey = e; }
        Equal(true, nullKey is not null); Equal("key", nullKey!.ParamName);
        var emptySlots = new OwnedCounter(2, FerruleOwnedSourceLimitKind.MetadataSlots);
        var empty = FerruleGroup.FromBasicSourceProjection(Array.Empty<FerruleField>(), 0, emptySlots);
        Equal<nuint>(2, emptySlots.Used); Equal(0, empty.Fields.Count);
        Equal(false, empty.TryGetField("A", out _));
        OwnedRaw("T501-snapshot", new { names = projection.Fields.Select(f => f.Name).ToArray(),
            upper = ((FerruleScalar)a!).Value.StringValue, lower = ((FerruleScalar)small!).Value.StringValue,
            used = (ulong)slots.Used, emptyUsed = (ulong)emptySlots.Used });
    }

    private static void BasicSourceProjectionInputRefusals()
    {
        var a = Field("A", Scalar(Text("upper")));
        var duplicateSlots = new OwnedCounter(4, FerruleOwnedSourceLimitKind.MetadataSlots);
        Error(FerruleRuntimeError.DuplicateField,
            () => FerruleGroup.FromBasicSourceProjection(new[] { a, a }, 2, duplicateSlots));
        Equal<nuint>(0, duplicateSlots.Used);
        var nullSlots = new OwnedCounter(3, FerruleOwnedSourceLimitKind.MetadataSlots);
        Throws<ArgumentNullException>(() => FerruleGroup.FromBasicSourceProjection(new FerruleField[] { null! }, 1, nullSlots));
        Equal<nuint>(0, nullSlots.Used);
        var rootSlots = new OwnedCounter(4, FerruleOwnedSourceLimitKind.MetadataSlots);
        var rootLimit = OwnedError("T501-root-limit", FerruleOwnedSourceErrorKind.MetadataLimit,
            () => FerruleGroup.FromBasicSourceProjection(new[] { a, Field("B", Scalar(Text("b"))) }, 1, rootSlots));
        Equal<FerruleOwnedSourceLimitKind?>(FerruleOwnedSourceLimitKind.Roots, rootLimit.LimitKind);
        Equal<nuint?>(0, rootLimit.Used); Equal<nuint?>(2, rootLimit.Requested); Equal<nuint?>(1, rootLimit.Maximum);
        Equal<nuint>(0, rootSlots.Used);
        var ordering = OwnedError("T501-root-before-null", FerruleOwnedSourceErrorKind.MetadataLimit,
            () => FerruleGroup.FromBasicSourceProjection(new FerruleField[] { null!, null! }, 1, rootSlots));
        Equal<FerruleOwnedSourceLimitKind?>(FerruleOwnedSourceLimitKind.Roots, ordering.LimitKind);
        Equal<nuint?>(0, ordering.Used); Equal<nuint?>(2, ordering.Requested); Equal<nuint?>(1, ordering.Maximum);
        Equal<nuint>(0, rootSlots.Used);
        Throws<ArgumentNullException>(() => FerruleGroup.FromBasicSourceProjection(new[] { a }, 1, null!));
    }

    private static void BasicSourceProjectionExactSlotAdmission()
    {
        var fields = new[] { Field("A", Scalar(Text("a"))), Field("B", Scalar(Text("b"))) };
        var exact = new OwnedCounter(6, FerruleOwnedSourceLimitKind.MetadataSlots);
        exact.Charge(2);
        var group = FerruleGroup.FromBasicSourceProjection(fields, 2, exact);
        Equal(2, group.Fields.Count); Equal<nuint>(6, exact.Used);
        var below = new OwnedCounter(5, FerruleOwnedSourceLimitKind.MetadataSlots);
        below.Charge(2);
        var error = OwnedError("T501-slots-before-copy", FerruleOwnedSourceErrorKind.MetadataLimit,
            () => FerruleGroup.FromBasicSourceProjection(fields, 2, below));
        Equal<FerruleOwnedSourceLimitKind?>(FerruleOwnedSourceLimitKind.MetadataSlots, error.LimitKind);
        Equal<nuint?>(2, error.Used); Equal<nuint?>(4, error.Requested); Equal<nuint?>(5, error.Maximum);
        Equal<nuint>(2, below.Used);
        OwnedRaw("T501-exact-slots", new { admittedUsed = (ulong)exact.Used, refusedUsed = (ulong)below.Used });
    }

    private static void BasicSourceProjectionLegacyRoutes()
    {
        var first = Field("A", Scalar(Text("first")));
        var second = Field("A", Scalar(Text("second")));
        FerruleRuntimeException? duplicate = null;
        try { _ = new FerruleGroup(new[] { first, second }); }
        catch (FerruleRuntimeException e) { duplicate = e; }
        Equal(true, duplicate is not null);
        Equal(FerruleRuntimeError.DuplicateField, duplicate!.Error);
        Equal("Ferrule group field 'A' is duplicated.", duplicate.Message);
        var schema = FerruleGroup.FromSchemaFields(new[] { first, second });
        Equal(2, schema.Fields.Count); Equal(true, schema.TryGetField("A", out var value));
        Equal(Text("first"), ((FerruleScalar)value!).Value);
        var rebuilt = schema.RebuildFields(schema.Fields);
        Equal(2, rebuilt.Fields.Count); Equal(true, rebuilt.TryGetField("A", out var rebuiltValue));
        Equal(Text("first"), ((FerruleScalar)rebuiltValue!).Value);
        var origin = FerruleXmlTypeOrigin.Explicit("Type");
        var schemaAnnotated = schema.WithXmlTypeOrigin(origin);
        Equal(true, ReferenceEquals(origin, schemaAnnotated.XmlTypeOrigin));
        Equal(2, schemaAnnotated.CloneFields(schemaAnnotated.Fields).Fields.Count);
        var bounded = FerruleGroup.FromBasicSourceProjection(new[] { first }, 1,
            new OwnedCounter(3, FerruleOwnedSourceLimitKind.MetadataSlots));
        Equal(FerruleXmlTypeOriginKind.Unknown, bounded.XmlTypeOrigin.Kind);
        var annotated = bounded.WithXmlTypeOrigin(origin);
        Equal(true, ReferenceEquals(origin, annotated.XmlTypeOrigin));
        var clone = annotated.CloneFields(annotated.Fields);
        Equal(true, ReferenceEquals(origin, clone.XmlTypeOrigin));
        Equal("A", clone.Fields[0].Name); Equal(true, clone.TryGetField("A", out var clonedValue));
        Equal(Text("first"), ((FerruleScalar)clonedValue!).Value);
    }
}
