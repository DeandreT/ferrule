using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string XmlTypeSchema =
        "{\"name\":\"Address\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Name\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"State\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Zip\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}},{\"name\":\"Postcode\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}],\"alternatives\":[{\"name\":\"{urn:ferrule:types}Domestic\",\"members\":[\"Name\",\"State\",\"Zip\"],\"required\":[\"State\",\"Zip\"]},{\"name\":\"{urn:ferrule:types}International\",\"members\":[\"Name\",\"Postcode\"],\"required\":[\"Postcode\"]}]}}";

    private const string XmlRepeatingChoiceSchema =
        "{\"name\":\"Values\",\"xml_repeating_choices\":[{\"members\":[\"Code\",\"Amount\"]}],\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Code\",\"repeating\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Amount\",\"repeating\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}]}}";

    private const string XmlSingularChoiceSchema =
        "{\"name\":\"Value\",\"xml_repeating_choices\":[{\"required\":true,\"repeating\":false,\"members\":[\"Code\",\"Amount\"]}],\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Code\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Amount\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}]}}";

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

    private static void XmlRepeatingChoices()
    {
        const string mixed = "\u001fferrule-xml-mixed-content";
        const string value = "\u001fferrule-xml-mixed-value";
        var instance = Group(
            Field(
                "Code",
                Repeated(Scalar(Text("A")), Scalar(Text("B")))),
            Field(
                "Amount",
                Repeated(Scalar(FerruleValue.FromInt64(2)))),
            Field(
                mixed,
                Repeated(
                    Group(
                        Field("NodeName", Scalar(Text("Code"))),
                        Field(value, Scalar(Text("A")))),
                    Group(
                        Field("NodeName", Scalar(Text("Amount"))),
                        Field(value, Scalar(FerruleValue.FromInt64(2)))),
                    Group(
                        Field("NodeName", Scalar(Text("Code"))),
                        Field(value, Scalar(Text("B")))))));
        Equal(
            Text("<Values><Code>A</Code><Amount>2</Amount><Code>B</Code></Values>"),
            FerruleXml.Serialize(
                11,
                XmlRepeatingChoiceSchema,
                instance,
                false,
                false,
                null));

        Error(
            FerruleRuntimeError.XmlSerialization,
            () => FerruleXml.Serialize(
                12,
                XmlRepeatingChoiceSchema,
                Group(
                    Field(
                        mixed,
                        Repeated(
                            Group(
                                Field("NodeName", Scalar(Text("Missing"))),
                                Field(value, Scalar(Text("x"))))))),
                false,
                false,
                null));

        Equal(
            Text("<Value><Code>A</Code></Value>"),
            FerruleXml.Serialize(
                13,
                XmlSingularChoiceSchema,
                Group(
                    Field("Code", Scalar(Text("A"))),
                    Field("Amount", Scalar(FerruleValue.Null))),
                false,
                false,
                null));
        Error(
            FerruleRuntimeError.XmlSerialization,
            () => FerruleXml.Serialize(
                14,
                XmlSingularChoiceSchema,
                Group(
                    Field("Code", Scalar(Text("A"))),
                    Field("Amount", Scalar(FerruleValue.FromInt64(2)))),
                false,
                false,
                null));
        Equal(
            Text("<Value></Value>"),
            FerruleXml.Serialize(
                15,
                XmlSingularChoiceSchema,
                Group(
                    Field("Code", Scalar(FerruleValue.Null)),
                    Field("Amount", Scalar(FerruleValue.Null))),
                false,
                false,
                null));
    }
}
