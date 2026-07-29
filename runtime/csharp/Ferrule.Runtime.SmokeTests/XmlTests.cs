using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string XmlTypeSchema =
        "{\"name\":\"Address\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Name\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"State\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Zip\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}},{\"name\":\"Postcode\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}],\"alternatives\":[{\"name\":\"{urn:ferrule:types}Domestic\",\"members\":[\"Name\",\"State\",\"Zip\"],\"required\":[\"State\",\"Zip\"]},{\"name\":\"{urn:ferrule:types}International\",\"members\":[\"Name\",\"Postcode\"],\"required\":[\"Postcode\"]}]}}";

    private static void XmlTypeAlternatives()
    {
        var domestic = FerruleXml.Serialize(
            7,
            XmlTypeSchema,
            Group(
                Field(
                    "\u001fferrule-xml-type",
                    Scalar(Text("{urn:ferrule:types}Domestic"))),
                Field("Name", Scalar(Text("Ada"))),
                Field("State", Scalar(Text("WA"))),
                Field("Zip", Scalar(FerruleValue.FromInt64(98101))),
                Field("Postcode", Scalar(FerruleValue.Null))),
            false,
            false,
            null);
        Equal(
            Text("<Address xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xmlns:ft=\"urn:ferrule:types\" xsi:type=\"ft:Domestic\"><Name>Ada</Name><State>WA</State><Zip>98101</Zip></Address>"),
            domestic);

        var international = FerruleXml.Serialize(
            8,
            XmlTypeSchema,
            Group(
                Field("Name", Scalar(Text("Ada"))),
                Field("State", Scalar(FerruleValue.Null)),
                Field("Zip", Scalar(FerruleValue.Null)),
                Field("Postcode", Scalar(Text("SW1A 1AA")))),
            false,
            false,
            null);
        Equal(
            Text("<Address xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xmlns:ft=\"urn:ferrule:types\" xsi:type=\"ft:International\"><Name>Ada</Name><Postcode>SW1A 1AA</Postcode></Address>"),
            international);

        Error(
            FerruleRuntimeError.XmlSerialization,
            () => FerruleXml.Serialize(
                9,
                XmlTypeSchema,
                Group(
                    Field(
                        "\u001fferrule-xml-type",
                        Scalar(Text("{urn:ferrule:types}Missing")))),
                false,
                false,
                null));
        Error(
            FerruleRuntimeError.XmlSerialization,
            () => FerruleXml.Serialize(
                10,
                XmlTypeSchema,
                Group(
                    Field(
                        "\u001fferrule-xml-type",
                        Scalar(Text("{urn:ferrule:types}Domestic"))),
                    Field("Name", Scalar(Text("Ada"))),
                    Field("State", Scalar(Text("WA"))),
                    Field("Zip", Scalar(FerruleValue.Null))),
                false,
                false,
                null));
    }
}
