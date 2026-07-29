pub(crate) const PROJECT: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <Deterministic>true</Deterministic>
    <InvariantGlobalization>true</InvariantGlobalization>
    <RootNamespace>Ferrule.Generated</RootNamespace>
    <AssemblyName>Ferrule.Generated</AssemblyName>
    <EnableDefaultCompileItems>false</EnableDefaultCompileItems>
  </PropertyGroup>
  <ItemGroup>
    <Compile Include="GeneratedMapping.cs" />
    <Compile Include="GeneratedTargetBuilder.cs" />
    <Compile Include="Runtime/**/*.cs" />
  </ItemGroup>
</Project>
"#;

pub(crate) const SOURCES: [(&str, &str); 41] = [
    (
        "Runtime/FerruleRuntimeException.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleRuntimeException.cs"),
    ),
    (
        "Runtime/FerruleExecutionContext.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleExecutionContext.cs"),
    ),
    (
        "Runtime/FerruleFailures.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleFailures.cs"),
    ),
    (
        "Runtime/FerruleDynamicDocuments.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleDynamicDocuments.cs"),
    ),
    (
        "Runtime/FerruleDynamicTargets.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleDynamicTargets.cs"),
    ),
    (
        "Runtime/FerruleDynamicSources.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleDynamicSources.cs"),
    ),
    (
        "Runtime/FerruleValue.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleValue.cs"),
    ),
    (
        "Runtime/FerruleValueMaps.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleValueMaps.cs"),
    ),
    (
        "Runtime/FerruleUserFunctions.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleUserFunctions.cs"),
    ),
    (
        "Runtime/FerruleInstance.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleInstance.cs"),
    ),
    (
        "Runtime/Json/FerruleJson.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/Json/FerruleJson.cs"),
    ),
    (
        "Runtime/Json/FerruleJson.AllowedValues.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/Json/FerruleJson.AllowedValues.cs"),
    ),
    (
        "Runtime/Json/FerruleJson.MultipleOf.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/Json/FerruleJson.MultipleOf.cs"),
    ),
    (
        "Runtime/Json/FerruleJson.ObjectOpenness.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/Json/FerruleJson.ObjectOpenness.cs"),
    ),
    (
        "Runtime/Json/FerruleJson.Patterns.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/Json/FerruleJson.Patterns.cs"),
    ),
    (
        "Runtime/Json/FerruleJson.PropertyCounts.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/Json/FerruleJson.PropertyCounts.cs"),
    ),
    (
        "Runtime/Json/FerruleJson.UniqueItems.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/Json/FerruleJson.UniqueItems.cs"),
    ),
    (
        "Runtime/FerruleFunctions.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleFunctions.cs"),
    ),
    (
        "Runtime/FerruleDelimitedText.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleDelimitedText.cs"),
    ),
    (
        "Runtime/FerruleFunctions.Json.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleFunctions.Json.cs"),
    ),
    (
        "Runtime/FerruleFunctions.Numeric.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleFunctions.Numeric.cs"),
    ),
    (
        "Runtime/FerruleFunctions.FormatNumber.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleFunctions.FormatNumber.cs"),
    ),
    (
        "Runtime/FerruleFunctions.DateTime.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleFunctions.DateTime.cs"),
    ),
    (
        "Runtime/FerruleFunctions.DateTimeAdd.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleFunctions.DateTimeAdd.cs"),
    ),
    (
        "Runtime/FerruleFunctions.DateTimePictures.cs",
        include_str!(
            "../../../runtime/csharp/Ferrule.Runtime/FerruleFunctions.DateTimePictures.cs"
        ),
    ),
    (
        "Runtime/FerruleFunctions.DateTimeFormatting.cs",
        include_str!(
            "../../../runtime/csharp/Ferrule.Runtime/FerruleFunctions.DateTimeFormatting.cs"
        ),
    ),
    (
        "Runtime/FerruleFunctions.EdifactDateTime.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleFunctions.EdifactDateTime.cs"),
    ),
    (
        "Runtime/FerruleFunctions.Strings.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleFunctions.Strings.cs"),
    ),
    (
        "Runtime/FerruleFunctions.Regex.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleFunctions.Regex.cs"),
    ),
    (
        "Runtime/FerruleJoins.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleJoins.cs"),
    ),
    (
        "Runtime/FerruleGrouping.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleGrouping.cs"),
    ),
    (
        "Runtime/FerruleAggregates.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleAggregates.cs"),
    ),
    (
        "Runtime/FerruleSequences.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleSequences.cs"),
    ),
    (
        "Runtime/FerruleRecursiveFilter.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleRecursiveFilter.cs"),
    ),
    (
        "Runtime/FerrulePathHierarchy.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerrulePathHierarchy.cs"),
    ),
    (
        "Runtime/FerruleAdjacencyTree.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleAdjacencyTree.cs"),
    ),
    (
        "Runtime/ScopeContext.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/ScopeContext.cs"),
    ),
    (
        "Runtime/ScopeContext.CollectionFind.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/ScopeContext.CollectionFind.cs"),
    ),
    (
        "Runtime/ScalarPathResolver.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/ScalarPathResolver.cs"),
    ),
    (
        "Runtime/FerruleXml.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleXml.cs"),
    ),
    (
        "Runtime/FerruleXmlMixedContent.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleXmlMixedContent.cs"),
    ),
];

pub(crate) const TARGET_BUILDER: &str = r#"namespace Ferrule.Generated;

[global::System.Flags]
internal enum TargetScalarDomain
{
    String = 1 << 0,
    Int64 = 1 << 1,
    Double = 1 << 2,
    Bool = 1 << 3,
}

internal static class TargetBuilder
{
    internal static global::Ferrule.Runtime.FerruleInstance Scalar(
        global::Ferrule.Runtime.FerruleValue value,
        TargetScalarDomain targetDomain) =>
        new global::Ferrule.Runtime.FerruleScalar(Adapt(value, targetDomain));

    internal static global::Ferrule.Runtime.FerruleInstance RepeatedScalar(
        global::System.Collections.Generic.IEnumerable<global::Ferrule.Runtime.FerruleValue> values,
        TargetScalarDomain targetDomain)
    {
        global::System.ArgumentNullException.ThrowIfNull(values);
        var items = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleInstance>();
        foreach (var sourceValue in values)
        {
            var value = Adapt(sourceValue, targetDomain);
            if (value.Kind != global::Ferrule.Runtime.FerruleValueKind.Null)
            {
                items.Add(new global::Ferrule.Runtime.FerruleScalar(value));
            }
        }

        return new global::Ferrule.Runtime.FerruleRepeated(items);
    }

    private static global::Ferrule.Runtime.FerruleValue Adapt(
        global::Ferrule.Runtime.FerruleValue value,
        TargetScalarDomain targetDomain)
    {
        var actualDomain = ValueDomain(value.Kind);
        if (actualDomain.HasValue &&
            (targetDomain & actualDomain.Value) != 0)
        {
            return value;
        }

        if ((targetDomain & TargetScalarDomain.Int64) != 0 &&
            value.Kind == global::Ferrule.Runtime.FerruleValueKind.Double)
        {
            var number = value.DoubleValue;
            if (global::System.Math.Truncate(number) == number &&
                number >= (double)long.MinValue &&
                number < -(double)long.MinValue)
            {
                return global::Ferrule.Runtime.FerruleValue.FromInt64((long)number);
            }
        }
        else if ((targetDomain & TargetScalarDomain.Double) != 0 &&
                 value.Kind == global::Ferrule.Runtime.FerruleValueKind.Int64)
        {
            var integer = value.Int64Value;
            var number = (double)integer;
            if (number >= (double)long.MinValue &&
                number < -(double)long.MinValue &&
                (long)number == integer)
            {
                return global::Ferrule.Runtime.FerruleValue.FromDouble(number);
            }
        }

        return value;
    }

    private static TargetScalarDomain? ValueDomain(
        global::Ferrule.Runtime.FerruleValueKind kind) =>
        kind switch
        {
            global::Ferrule.Runtime.FerruleValueKind.String => TargetScalarDomain.String,
            global::Ferrule.Runtime.FerruleValueKind.Int64 => TargetScalarDomain.Int64,
            global::Ferrule.Runtime.FerruleValueKind.Double => TargetScalarDomain.Double,
            global::Ferrule.Runtime.FerruleValueKind.Bool => TargetScalarDomain.Bool,
            _ => null,
        };
}
"#;
