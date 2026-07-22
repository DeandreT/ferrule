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

pub(crate) const SOURCES: [(&str, &str); 22] = [
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
        "Runtime/FerruleFunctions.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleFunctions.cs"),
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
        "Runtime/FerruleFunctions.EdifactDateTime.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleFunctions.EdifactDateTime.cs"),
    ),
    (
        "Runtime/FerruleFunctions.Strings.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleFunctions.Strings.cs"),
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
];

pub(crate) const TARGET_BUILDER: &str = r#"namespace Ferrule.Generated;

internal enum TargetScalarType
{
    String,
    Int64,
    Double,
    Bool,
}

internal static class TargetBuilder
{
    internal static global::Ferrule.Runtime.FerruleInstance Scalar(
        global::Ferrule.Runtime.FerruleValue value,
        TargetScalarType targetType) =>
        new global::Ferrule.Runtime.FerruleScalar(AdaptNumeric(value, targetType));

    internal static global::Ferrule.Runtime.FerruleInstance RepeatedScalar(
        global::System.Collections.Generic.IEnumerable<global::Ferrule.Runtime.FerruleValue> values,
        TargetScalarType targetType)
    {
        global::System.ArgumentNullException.ThrowIfNull(values);
        var items = new global::System.Collections.Generic.List<global::Ferrule.Runtime.FerruleInstance>();
        foreach (var sourceValue in values)
        {
            var value = AdaptNumeric(sourceValue, targetType);
            if (value.Kind != global::Ferrule.Runtime.FerruleValueKind.Null)
            {
                items.Add(new global::Ferrule.Runtime.FerruleScalar(value));
            }
        }

        return new global::Ferrule.Runtime.FerruleRepeated(items);
    }

    private static global::Ferrule.Runtime.FerruleValue AdaptNumeric(
        global::Ferrule.Runtime.FerruleValue value,
        TargetScalarType targetType)
    {
        if (targetType == TargetScalarType.Int64 &&
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
        else if (targetType == TargetScalarType.Double &&
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
}
"#;
