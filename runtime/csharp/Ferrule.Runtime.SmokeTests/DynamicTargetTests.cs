using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void DynamicTargetConstruction()
    {
        Equal("name", FerruleDynamicTargets.PropertyName(7, Text("name")));
        var type = Error(
            FerruleRuntimeError.DynamicPropertyName,
            () => FerruleDynamicTargets.PropertyName(7, FerruleValue.FromInt64(1)));
        Equal((uint?)7, type.Node);
        Equal((FerruleValueKind?)FerruleValueKind.Int64, type.FoundKind);

        var fields = new List<FerruleField> { Field("prior", Scalar(FerruleValue.Null)) };
        var fixedCollision = Error(
            FerruleRuntimeError.DuplicateDynamicProperty,
            () => FerruleDynamicTargets.Insert(
                fields,
                new[] { "fixed" },
                "fixed",
                Scalar(FerruleValue.Null)));
        Equal("fixed", fixedCollision.Detail);
        var duplicate = Error(
            FerruleRuntimeError.DuplicateDynamicProperty,
            () => FerruleDynamicTargets.Insert(
                fields,
                Array.Empty<string>(),
                "prior",
                Scalar(FerruleValue.Null)));
        Equal("prior", duplicate.Detail);

        var merged = FerruleDynamicTargets.Merge(new FerruleInstance[]
        {
            Group(Field("first", Scalar(FerruleValue.FromInt64(1)))),
            Group(Field("second", Scalar(FerruleValue.FromInt64(2)))),
        });
        Equal("first,second", string.Join(',', merged.Fields.Select(field => field.Name)));
        _ = Error(
            FerruleRuntimeError.InvalidDynamicPropertyFragment,
            () => FerruleDynamicTargets.Merge(new[] { Scalar(FerruleValue.Null) }));
    }
}
