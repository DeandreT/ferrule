using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static FerruleSourceContractViolation SourceContractFailure(FerruleSourceContractReason expected, Action action)
    {
        var error = Error(FerruleRuntimeError.SourceContractViolation, action);
        Equal(expected, error.SourceContractViolation!.Reason);
        return error.SourceContractViolation;
    }

    private static void ClosedPrimarySourceShapes()
    {
        var plan = new FerrulePrimarySourceContract(new[] {
            FerrulePrimarySourceShape.Group("Input", false, 1, 4),
            FerrulePrimarySourceShape.Group("Row", true, 2),
            FerrulePrimarySourceShape.Group("Cell", true, 3),
            FerrulePrimarySourceShape.Scalar("Value", FerruleScalarType.String, false),
            FerrulePrimarySourceShape.Scalar("Text", FerruleScalarType.String, true),
        });
        var empty = new FerruleRepeated([]);
        plan.Validate(Group(Field("Row", new FerruleRepeated([Group(Field("Cell", empty))])), Field("Text", Scalar(FerruleValue.XmlNil))), 0, 0);
        SourceContractFailure(FerruleSourceContractReason.MissingRepeatedSlot,
            () => plan.Validate(Group(Field("Row", new FerruleRepeated([Group()]))), 0, 0));
        SourceContractFailure(FerruleSourceContractReason.WrongShape,
            () => plan.Validate(Group(Field("Row", Scalar(FerruleValue.Null))), 0, 0));
        SourceContractFailure(FerruleSourceContractReason.WrongScalarDomain,
            () => plan.Validate(Group(Field("Row", empty), Field("Text", Scalar(FerruleValue.JsonNull))), 0, 0));
        var duplicate = FerruleGroup.FromSchemaFields([Field("Row", empty), Field("Row", empty)]);
        SourceContractFailure(FerruleSourceContractReason.DuplicateField, () => plan.Validate(duplicate, 0, 0));
        var failure = SourceContractFailure(FerruleSourceContractReason.UnexpectedField,
            () => plan.Validate(Group(Field("private-host-name", Scalar(Text("secret")))), 0, 0));
        Equal(0, failure.Path.Count);
        SourceContractFailure(FerruleSourceContractReason.UnexpectedBinding, () => plan.Validate(Scalar(Text("secret")), 2, 1));
        SourceContractFailure(FerruleSourceContractReason.UnexpectedParameter, () => plan.Validate(Scalar(Text("secret")), 0, 1));
        foreach (var instance in new FerruleInstance[] { Scalar(FerruleValue.Null), empty, new FerruleMappedSequence([]), new FerruleDocumentSet([]) })
            SourceContractFailure(FerruleSourceContractReason.WrongShape, () => plan.Validate(instance, 0, 0));
    }

    private static void ClosedPrimarySourceUnicodeAndWork()
    {
        const string bmp = "X\uff21";
        const string supplementary = "X\ud800\udc00";
        var plan = new FerrulePrimarySourceContract(new[] {
            FerrulePrimarySourceShape.Group("Input", false, 1, 2),
            FerrulePrimarySourceShape.Scalar(bmp, FerruleScalarType.String, false),
            FerrulePrimarySourceShape.Scalar(supplementary, FerruleScalarType.String, false),
        });
        plan.Validate(Group(Field(supplementary, Scalar(Text("B"))), Field(bmp, Scalar(Text("A")))), 0, 0);
        SourceContractFailure(FerruleSourceContractReason.UnexpectedField,
            () => plan.Validate(Group(Field("X\uff22", Scalar(Text("secret")))), 0, 0));
        SourceContractFailure(FerruleSourceContractReason.DuplicateField,
            () => plan.Validate(FerruleGroup.FromSchemaFields([Field(bmp, Scalar(Text("A"))), Field(bmp, Scalar(Text("A")))]), 0, 0));
        var nodes = new List<FerrulePrimarySourceShape> { FerrulePrimarySourceShape.Group("Input", false, 1),
            FerrulePrimarySourceShape.Group("Row", true, Enumerable.Range(2, 2000).ToArray()) };
        for (var i = 0; i < 2000; i++) nodes.Add(FerrulePrimarySourceShape.Scalar($"F{i:D4}", FerruleScalarType.String, false));
        var wide = new FerrulePrimarySourceContract(nodes);
        var shared = Group();
        wide.Validate(Group(Field("Row", new FerruleRepeated(Enumerable.Repeat(shared, 499)))), 0, 0);
        SourceContractFailure(FerruleSourceContractReason.ResourceLimit,
            () => wide.Validate(Group(Field("Row", new FerruleRepeated(Enumerable.Repeat(shared, 500)))), 0, 0));
        var longName = "X" + string.Concat(Enumerable.Repeat("\ud800\udc00", 1024));
        SourceContractFailure(FerruleSourceContractReason.ResourceLimit, () => new FerrulePrimarySourceContract(new[] {
            FerrulePrimarySourceShape.Group("I", false, 1),
            FerrulePrimarySourceShape.Scalar(longName, FerruleScalarType.String, false),
        }));
        var deep = new List<FerrulePrimarySourceShape>();
        for (var i = 0; i < 257; i++) deep.Add(FerrulePrimarySourceShape.Group("N", false, i + 1));
        deep.Add(FerrulePrimarySourceShape.Scalar("Value", FerruleScalarType.String, false));
        SourceContractFailure(FerruleSourceContractReason.ResourceLimit, () => new FerrulePrimarySourceContract(deep));
        SourceContractFailure(FerruleSourceContractReason.ResourceLimit, () => new FerrulePrimarySourceContract(
            new FerrulePrimarySourceShape[FerrulePrimarySourceContract.MaximumShapeNodes + 1]));
    }
}
