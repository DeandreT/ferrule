using Ferrule.Runtime;
using System.Text;
namespace Ferrule.Runtime.SmokeTests;
internal static partial class Program
{
    private static void XmlOptionalOccurrenceMetadata()
    {
        const string legacy = "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Value\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Maybe\",\"kind\":{\"kind\":\"group\",\"children\":[]}}]}}";
        var optional=legacy.Replace("\"name\":\"Value\"", "\"xml_optional\":true,\"name\":\"Value\"", StringComparison.Ordinal).Replace("\"name\":\"Maybe\"", "\"xml_optional\":true,\"name\":\"Maybe\"", StringComparison.Ordinal);
        foreach (var document in new[]{"{}", "{\"Value\":\"\",\"Maybe\":{}}", "{\"Value\":\"x\"}", "{\"Maybe\":{}}"})
        {
            var original=FerruleJson.Parse(legacy, document);var current=FerruleJson.Parse(optional,document);
            Equal(FerruleJson.Serialize(legacy,original),FerruleJson.Serialize(optional,current));
            Equal(FerruleJson.SerializeEmbedded(optional,current),Encoding.UTF8.GetString(FerruleJson.SerializeEmbeddedBytes(optional,current)));
            Equal(FerruleJson.SerializeEmbedded(optional,current),FerruleJson.SerializeEmbedded(optional,FerruleJson.ParseEmbeddedBytes(optional,Encoding.UTF8.GetBytes(document))));
            Equal(FerruleXml.SerializeEmbedded(1,legacy,original,false,false,null),FerruleXml.SerializeEmbedded(1,optional,current,false,false,null));
        }
        foreach (var role in new[]{"\"repeating\":true", "\"attribute\":true", "\"text\":true"})
        {
            var invalid=optional.Replace("\"name\":\"Value\"", "\"name\":\"Value\","+role,StringComparison.Ordinal);
            Error(FerruleRuntimeError.JsonBoundary,()=>FerruleJson.ParseEmbedded(invalid,"{}"));
            Error(FerruleRuntimeError.JsonBoundary,()=>FerruleJson.ParseEmbeddedBytes(invalid,Encoding.UTF8.GetBytes("{}")));
            Error(FerruleRuntimeError.JsonBoundary,()=>FerruleJson.SerializeEmbedded(invalid,Group()));
            Error(FerruleRuntimeError.JsonBoundary,()=>FerruleJson.SerializeEmbeddedBytes(invalid,Group()));
            Error(FerruleRuntimeError.XmlSerialization,()=>FerruleXml.SerializeEmbedded(1,invalid,Group(),false,false,null));
        }
        foreach (var value in new[]{"null","0","\"true\"","[]"})
        {
            var invalid=optional.Replace("\"xml_optional\":true", "\"xml_optional\":"+value,StringComparison.Ordinal);
            Error(FerruleRuntimeError.JsonBoundary,()=>FerruleJson.ParseEmbedded(invalid,"{}"));
            Error(FerruleRuntimeError.XmlSerialization,()=>FerruleXml.SerializeEmbedded(1,invalid,Group(),false,false,null));
        }
        var nilSchema=optional.Replace("\"name\":\"Value\"", "\"nillable\":true,\"name\":\"Value\"",StringComparison.Ordinal);
        var nil=Group(Field("Value",Scalar(FerruleValue.XmlNil)));
        Equal(FerruleXml.SerializeEmbedded(1,nilSchema,nil,false,false,null),FerruleXml.SerializeEmbedded(1,nilSchema.Replace("\"xml_optional\":true,","",StringComparison.Ordinal),nil,false,false,null));
    }
}
