using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void SourceIndependentRuntimeParameters()
    {
        var parameters = new FerruleRuntimeParameters(new[] {
            KeyValuePair.Create("filename", Text(" ./input-*.csv ")),
            KeyValuePair.Create("number", FerruleValue.FromInt64(42)),
            KeyValuePair.Create("null", FerruleValue.Null),
            KeyValuePair.Create("bad", FerruleValue.FromDouble(double.NaN)),
        });
        var execution = FerruleExecutionContext.WithParameters("mapping.ferrule", parameters);
        var source = Group(Field("filename", Scalar(Text("not-the-parameter.csv"))));
        var scope = ScopeContext.FromSource(source, execution);
        foreach (var (name, expected) in new[] {
            ("filename", Text(" ./input-*.csv ")),
            ("number", Text("42")),
            ("null", FerruleValue.Null),
        })
        {
            Equal(expected, execution.ResolveRuntimeParameter(17, name, FerruleScalarType.String));
            Equal(expected, scope.ResolveRuntimeParameter(17, name, FerruleScalarType.String));
        }
        foreach (var (name, kind) in new[] {
            ("missing", FerruleRuntimeError.MissingRuntimeParameter),
            ("bad", FerruleRuntimeError.RuntimeParameterType),
        })
        {
            var direct = Error(kind, () => execution.ResolveRuntimeParameter(19, name, FerruleScalarType.String));
            var bound = Error(kind, () => scope.ResolveRuntimeParameter(19, name, FerruleScalarType.String));
            Equal((uint?)19, direct.Node);
            Equal(name, direct.RuntimeParameter);
            Equal(bound.Message, direct.Message);
            Equal(bound.FoundKind, direct.FoundKind);
            Equal(bound.ExpectedScalarType, direct.ExpectedScalarType);
            if (name == "bad")
            {
                Equal(FerruleValueKind.Double, direct.FoundKind);
                Equal(FerruleScalarType.String, direct.ExpectedScalarType);
            }
            else
            {
                Equal<FerruleValueKind?>(null, direct.FoundKind);
                Equal<FerruleScalarType?>(null, direct.ExpectedScalarType);
            }
        }
        var absent = new FerruleExecutionContext("mapping.ferrule");
        var missingDirect = Error(FerruleRuntimeError.MissingRuntimeParameter,
            () => absent.ResolveRuntimeParameter(23, "filename", FerruleScalarType.String));
        var missingScope = Error(FerruleRuntimeError.MissingRuntimeParameter,
            () => ScopeContext.FromSource(source).ResolveRuntimeParameter(23, "filename", FerruleScalarType.String));
        Equal(missingScope.Message, missingDirect.Message);
        Throws<ArgumentNullException>(() => execution.ResolveRuntimeParameter(1, null!, FerruleScalarType.String));
    }
}
