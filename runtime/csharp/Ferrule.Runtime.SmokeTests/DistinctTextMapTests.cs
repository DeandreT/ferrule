using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void DistinctTextMapLimits()
    {
        static ScopeContext Context(params FerruleValue[] keys) => ScopeContext.FromSource(new FerruleGroup(new[]
        {
            new FerruleField("Rows", new FerruleRepeated(keys.Select(key => new FerruleGroup(new[]
            { new FerruleField("Key", new FerruleScalar(key)) }))))
        }));
        static void Check(bool condition) { if (!condition) throw new InvalidOperationException("Distinct text map limit check failed."); }
        int calls=0;
        FerruleValue Map(ScopeContext _) { calls++; return FerruleValue.FromString("mapped"); }
        var two=Context(FerruleValue.FromString("A"),FerruleValue.FromString("B"));
        try { two.DistinctTextMapLast(42,new[]{"Rows"},new[]{"Key"},Map,maximumKeys:1); throw new InvalidOperationException("Missing key limit."); }
        catch(FerruleRuntimeException error) when(error.Error==FerruleRuntimeError.DistinctTextMapLimit && error.Node==42) { Check(calls==0); }
        var bytes=Context(FerruleValue.FromString("é"),FerruleValue.FromString("b"));
        try { bytes.DistinctTextMapLast(42,new[]{"Rows"},new[]{"Key"},Map,maximumKeyBytes:2); throw new InvalidOperationException("Missing byte limit."); }
        catch(FerruleRuntimeException error) when(error.Error==FerruleRuntimeError.DistinctTextMapLimit) { Check(calls==0); }
        Check(bytes.DistinctTextMapLast(42,new[]{"Rows"},new[]{"Key"},Map,maximumKeys:2,maximumKeyBytes:3).StringValue=="mapped" && calls==2);
        calls=0;
        var empty=Context(FerruleValue.Null,FerruleValue.FromString(""),FerruleValue.FromString(""));
        Check(empty.DistinctTextMapLast(42,new[]{"Rows"},new[]{"Key"},Map,maximumKeys:1,maximumKeyBytes:0).StringValue=="mapped" && calls==1);
        calls=0;
        try { Context(FerruleValue.JsonNull).DistinctTextMapLast(42,new[]{"Rows"},new[]{"Key"},Map); throw new InvalidOperationException("JSON null was treated as absence."); }
        catch(FerruleRuntimeException error) when(error.Error==FerruleRuntimeError.DistinctTextMapKey) { Check(calls==0); }
    }
}
