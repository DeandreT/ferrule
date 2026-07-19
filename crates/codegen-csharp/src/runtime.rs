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

pub(crate) const SOURCES: [(&str, &str); 7] = [
    (
        "Runtime/FerruleRuntimeException.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleRuntimeException.cs"),
    ),
    (
        "Runtime/FerruleValue.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleValue.cs"),
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
        "Runtime/FerruleAggregates.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/FerruleAggregates.cs"),
    ),
    (
        "Runtime/ScopeContext.cs",
        include_str!("../../../runtime/csharp/Ferrule.Runtime/ScopeContext.cs"),
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
