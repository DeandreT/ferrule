using System.Text;
using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static object EdiLeaf(string name, string type = "string", bool repeating = false, string? fixedValue = null) =>
        new { name, kind = new { kind = "scalar", ty = type }, repeating, @fixed = fixedValue };
    private static object EdiGroup(string name, object[] children, bool repeating = false) =>
        new { name, kind = new { kind = "group", children }, repeating };
    private static object EdiRule(string[] path, uint minimum, uint maximum, params string[] codes) =>
        new { path, min_chars = minimum, max_chars = maximum, allowed_values = codes };
    private static FerruleEdi EdiSource(object[] children, object[] rules, string family = "Edifact", object? layout = null, object[]? implied = null) =>
        new(JsonSerializer.Serialize(EdiGroup("Document", children)), JsonSerializer.Serialize(new
        {
            lenient_segments = true, edi_value_constraints = rules, idoc = layout,
            edi_implied_decimals = implied ?? [],
        }), family, input: true);
    private static FerruleEdiException EdiRefusal(FerruleEdiError expected, Action action)
    {
        try { action(); }
        catch (FerruleEdiException error) { Equal(expected, error.Code); return error; }
        throw new Exception($"Expected EDI {expected} refusal.");
    }
    private static FerruleValue EdiAt(FerruleInstance value, params string[] path)
    {
        foreach (string part in path) value = ((FerruleGroup)value).Fields.Single(field => field.Name == part).Value;
        return ((FerruleScalar)value).Value;
    }
    private static void EdiSourceLexicalConstraints()
    {
        foreach (var (type, raw, expected) in new[] { ("int", "0001", 1d), ("float", "01.00", 1d), ("float", "1234", 12.34d) })
        {
            object[] implied = raw == "1234" ? [new { path = new[] { "DTL", "Value" }, places = 2 }] : [];
            var codec = EdiSource([EdiGroup("DTL", [EdiLeaf("Value", type)])], [EdiRule(["DTL", "Value"], (uint)raw.Length, (uint)raw.Length, raw)], implied: implied);
            var parsed = EdiAt(codec.Parse(Encoding.UTF8.GetBytes($"UNH+1'DTL+{raw}'")), "DTL", "Value");
            Equal(type == "int" ? FerruleValueKind.Int64 : FerruleValueKind.Double, parsed.Kind);
            Equal(expected, type == "int" ? (double)parsed.Int64Value : parsed.DoubleValue);
            Equal(FerruleValueKind.Null, EdiAt(codec.Parse("UNH+1'DTL+'"u8.ToArray()), "DTL", "Value").Kind);
            var invalid = EdiSource([EdiGroup("DTL", [EdiLeaf("Value", type)])], [EdiRule(["DTL", "Value"], 1, (uint)raw.Length - 1, "1")], implied: implied);
            var error = EdiRefusal(FerruleEdiError.Constraint, () => invalid.Parse(Encoding.UTF8.GetBytes($"UNH+1'DTL+{raw}'")));
            Equal("DTL/Value", error.Field);
            Equal("DTL/Value", error.Location);
            Equal(2, error.ValidationReport!.Issues.Count);
            Equal(FerruleEdiConstraintViolation.TooLong, error.ValidationReport.Issues[0].Violation);
            Equal(FerruleEdiConstraintViolation.NotAllowed, error.ValidationReport.Issues[1].Violation);
        }
        var integer = EdiSource([EdiGroup("DTL", [EdiLeaf("Value", "int")])], [EdiRule(["DTL", "Value"], 1, 3)]);
        EdiRefusal(FerruleEdiError.Constraint, () => integer.Parse("UNH+1'DTL+999999999999999999999999'"u8.ToArray()));
        EdiRefusal(FerruleEdiError.Value, () => integer.Parse("UNH+1'DTL+bad'"u8.ToArray()));
        Equal(1L, EdiAt(integer.Parse("UNH+1'DTL+001'"u8.ToArray()), "DTL", "Value").Int64Value);
        EdiRefusal(FerruleEdiError.Constraint, () => integer.Parse("UNH+1'DTL+0001'"u8.ToArray()));
        Equal(2L, EdiAt(integer.Parse("UNH+1'DTL+002'"u8.ToArray()), "DTL", "Value").Int64Value);
        foreach (string empty in new[] { "UNH+1'DTL'", "UNH+1'DTL+'" })
            Equal(FerruleValueKind.Null, EdiAt(integer.Parse(Encoding.UTF8.GetBytes(empty)), "DTL", "Value").Kind);
        var lexical = new FerruleEdi(JsonSerializer.Serialize(EdiGroup("Document", [EdiGroup("DTL", [EdiLeaf("Value")])])),
            """{"lenient_segments":true,"edi_lexical_formats":[{"path":["DTL","Value"],"kind":"compact_date6"}],"edi_value_constraints":[{"path":["DTL","Value"],"min_chars":7,"max_chars":7,"allowed_values":["notdate"]}]}""", "Edifact", input: true);
        Equal("notdate", EdiAt(lexical.Parse("UNH+1'DTL+notdate'"u8.ToArray()), "DTL", "Value").StringValue);
        var priority = EdiSource([EdiGroup("DTL", [EdiLeaf("Code", "int"), EdiLeaf("Text")])], [EdiRule(["DTL", "Text"], 2, 2)]);
        EdiRefusal(FerruleEdiError.Value, () => priority.Parse("UNH+1'DTL+bad+X'"u8.ToArray()));
        var ordered = EdiSource([EdiGroup("DTL", [EdiLeaf("Code"), EdiLeaf("Text")])], [EdiRule(["DTL", "Text"], 2, 2), EdiRule(["DTL", "Code"], 2, 2)]);
        var orderedReport = EdiRefusal(FerruleEdiError.Constraint, () => ordered.Parse("UNH+1'DTL+X+Y'"u8.ToArray())).ValidationReport!;
        Equal("DTL/Code", orderedReport.Issues[0].Location);
        Equal("DTL/Text", orderedReport.Issues[1].Location);
        var owners = EdiSource([
            EdiGroup("DTL_1", [EdiLeaf("Kind", fixedValue: "A"), EdiLeaf("Code")]),
            EdiGroup("DTL_2", [EdiLeaf("Kind", fixedValue: "B"), EdiLeaf("Code")]),
        ], [EdiRule(["DTL_1", "Code"], 1, 1, "X"), EdiRule(["DTL_2", "Code"], 1, 1, "Y")]);
        Equal("Y", EdiAt(owners.Parse("UNH+1'DTL+A+X'DTL+B+Y'"u8.ToArray()), "DTL_2", "Code").StringValue);
    }

    private static void EdiSourceProjectionAndFamilies()
    {
        var cases = new[] {
            ("Edifact", "UNH+1'DTL+A?+🙂'", "A+🙂"),
            ("Tradacoms", "STX=ANA'DTL=A?+🙂'", "A+🙂"),
            ("Hl7", "MSH|^~\\&|APP\rDTL|A\\T\\🙂\r", "A&🙂"),
            ("X12", "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *261025*1200*^*00501*000000001*0*P*:~DTL*A🙂~", "A🙂"),
        };
        foreach (var (family, wire, lexical) in cases)
        {
            var codec = EdiSource([EdiGroup("DTL", [EdiLeaf("Value")])], [EdiRule(["DTL", "Value"], (uint)lexical.EnumerateRunes().Count(), (uint)lexical.EnumerateRunes().Count(), lexical)], family);
            Equal(lexical, EdiAt(codec.Parse(Encoding.UTF8.GetBytes(wire)), "DTL", "Value").StringValue);
            var invalid = EdiSource([EdiGroup("DTL", [EdiLeaf("Value")])], [EdiRule(["DTL", "Value"], 1, 1)], family);
            EdiRefusal(FerruleEdiError.Constraint, () => invalid.Parse(Encoding.UTF8.GetBytes(wire)));
        }
        var composite = EdiSource([EdiGroup("DTL", [EdiGroup("Pair", [EdiLeaf("First")])])], [EdiRule(["DTL", "Pair", "First"], 1, 1, "A")]);
        Equal("A", EdiAt(composite.Parse("UNB+UNOC:4'ZZZ+IGNORED'DTL+A:UNPROJECTED*EXTRA'"u8.ToArray()), "DTL", "Pair", "First").StringValue);
        var scalarComposite = EdiSource([EdiGroup("DTL", [EdiLeaf("Value")], true)], [EdiRule(["DTL", "Value"], 3, 3, "A:B", "C/D")]);
        // Each interchange has its own component separator, including string projections.
        var parsed = (FerruleRepeated)((FerruleGroup)scalarComposite.Parse("UNH+1'DTL+A:B'UNT+2+1'UNA/+.? !UNH+2!DTL+C/D!UNT+2+2!"u8.ToArray())).Fields[0].Value;
        Equal("A:B", EdiAt(parsed.Items[0], "Value").StringValue);
        Equal("C/D", EdiAt(parsed.Items[1], "Value").StringValue);
        var hl7Composite = EdiSource([EdiGroup("DTL", [EdiGroup("Composite", [EdiGroup("Sub", [EdiLeaf("Value")])])])], [EdiRule(["DTL", "Composite", "Sub", "Value"], 3, 3, "A&B")], "Hl7");
        Equal("A&B", EdiAt(hl7Composite.Parse("MSH|^~\\&|APP\rDTL|A\\T\\B&SKIPPED^EXTRA\r"u8.ToArray()), "DTL", "Composite", "Sub", "Value").StringValue);
        var layout = new { segments = new[] { new { name = "DTL", fields = new[] { new { name = "Value", first_byte = 5, last_byte = 12 } } } } };
        var idoc = EdiSource([EdiGroup("DTL", [EdiLeaf("Value")])], [EdiRule(["DTL", "Value"], 2, 2, "é🙂")], "Idoc", layout);
        Equal("é🙂", EdiAt(idoc.Parse(Encoding.UTF8.GetBytes("DTL \té🙂 \0\r\n")), "DTL", "Value").StringValue);
        EdiRefusal(FerruleEdiError.Constraint, () => idoc.Parse("DTL TOOLONG \r\n"u8.ToArray()));
        EdiRefusal(FerruleEdiError.Format, () => idoc.Parse([68, 84, 76, 32, 0xc3, 32]));
        var qualifiers = EdiSource([
            EdiGroup("DTL_1", [EdiLeaf("Kind", fixedValue: "A"), EdiLeaf("Code")]),
            EdiGroup("DTL_2", [EdiLeaf("Kind", fixedValue: "B"), EdiLeaf("Code")], true),
        ], [EdiRule(["DTL_2", "Code"], 2, 2, "OK")]);
        Equal("IGNORED", EdiAt(qualifiers.Parse("UNH+1'DTL+Z+TOOLONG'DTL+A+IGNORED'DTL+B+OK'"u8.ToArray()), "DTL_1", "Code").StringValue);
        var qualifiedError = EdiRefusal(FerruleEdiError.Constraint, () => qualifiers.Parse("UNH+1'DTL+A+IGNORED'DTL+B+BAD'"u8.ToArray()));
        Equal("DTL_2/Code", qualifiedError.Field);
        Equal("DTL_2[1]/Code", qualifiedError.Location);
    }

    private static void EdiSourceMetadataLocationsAndBounds()
    {
        object[] children = [EdiGroup("Rows", [EdiGroup("DTL", [EdiLeaf("Code", repeating: true)])], true)];
        var codec = EdiSource(children, [EdiRule(["Rows", "DTL", "Code"], 2, 2, "AA")]);
        var error = EdiRefusal(FerruleEdiError.Constraint, () => codec.Parse("UNB+UNOC:4'DTL+AA*B'DTL+LONG'"u8.ToArray()));
        Equal(4, error.ValidationReport!.Issues.Count);
        Equal("Rows/DTL/Code", error.Field);
        Equal("Rows[1]/DTL/Code[2]", error.ValidationReport.Issues[0].Location);
        Equal("Rows[2]/DTL/Code[1]", error.ValidationReport.Issues[2].Location);
        Equal(false, error.ValidationReport.Truncated);
        Equal(false, error.Message.Contains("LONG", StringComparison.Ordinal));
        Equal(0, ((FerruleRepeated)((FerruleGroup)codec.Parse("UNH+1'"u8.ToArray())).Fields[0].Value).Items.Count);
        foreach (object[] rules in new[] {
            new[] { EdiRule(["Rows"], 0, 2) }, new[] { EdiRule(["Rows", "Missing"], 0, 2) },
            new[] { EdiRule(["Rows", "DTL", "Code"], 3, 2) }, new[] { EdiRule(["Rows", "DTL", "Code"], 0, 0) },
            new[] { EdiRule(["Rows", "DTL", "Code"], 0, 2), EdiRule(["Rows", "DTL", "Code"], 0, 2) },
            new object[] { new { path = new[] { "Rows", "DTL", "Code" }, min_chars = -1, max_chars = 2 } },
            new object[] { new { path = new[] { "Rows", "DTL", "Code" }, min_chars = 0, max_chars = 2, allowed_values = new[] { 1 } } },
        }) EdiRefusal(FerruleEdiError.Descriptor, () => EdiSource(children, rules));
        var codes = Enumerable.Range(0, 4096).Select(code => code.ToString("D4", System.Globalization.CultureInfo.InvariantCulture)).ToArray();
        object[] codeFields = [EdiGroup("DTL", [EdiLeaf("Value")])];
        foreach (var allowed in new[] { codes, codes.Concat(codes).ToArray() })
        {
            var bounded = EdiSource(codeFields, [EdiRule(["DTL", "Value"], 4, 4, allowed)]);
            Equal("4095", EdiAt(bounded.Parse("UNH+1'DTL+4095'"u8.ToArray()), "DTL", "Value").StringValue);
            var refusal = EdiRefusal(FerruleEdiError.Constraint, () => bounded.Parse("UNH+1'DTL+9999'"u8.ToArray())).ValidationReport!;
            Equal(1, refusal.Issues.Count);
            Equal((int?)4096, refusal.Issues[0].AllowedCount);
        }
        EdiRefusal(FerruleEdiError.Descriptor, () => EdiSource(codeFields, [EdiRule(["DTL", "Value"], 4, 4, codes.Append("4096").ToArray())]));
        var many = "UNB+UNOC:4'" + string.Concat(Enumerable.Repeat("DTL+B'", 5_001));
        var report = EdiRefusal(FerruleEdiError.Constraint, () => codec.Parse(Encoding.UTF8.GetBytes(many))).ValidationReport!;
        Equal(10_000, report.Issues.Count);
        Equal(true, report.Truncated);
        Equal("Rows[5000]/DTL/Code[1]", report.Issues[^1].Location);
        Equal(true, report.ToString().Length < 2_000);
        var exact = "UNB+UNOC:4'" + string.Concat(Enumerable.Repeat("DTL+B'", 5_000)) + "DTL+AA'";
        var atCap = EdiRefusal(FerruleEdiError.Constraint, () => codec.Parse(Encoding.UTF8.GetBytes(exact))).ValidationReport!;
        Equal(10_000, atCap.Issues.Count);
        Equal(false, atCap.Truncated);
        Equal("Rows[5000]/DTL/Code[1]", atCap.Issues[^1].Location);
    }
}
