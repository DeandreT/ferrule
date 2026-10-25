using System.Globalization;
using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public sealed partial class FerruleEdi
{
    private sealed record OutputSegment(string Id, List<string> Elements);
    public byte[] Serialize(FerruleInstance source, string? currentDatetime)
    {
        ArgumentNullException.ThrowIfNull(source);
        foreach (var lexical in Array(_options, "edi_lexical_formats"))
        {
            string[] path = Array(lexical, "path").Select(value => value.GetString()!).ToArray();
            source = Transform(source, path, 0, value => {
                if (value.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull) return value;
                JsonElement kind = lexical.GetProperty("kind");
                string name = kind.ValueKind == JsonValueKind.String ? kind.GetString()! : kind.EnumerateObject().Single().Name;
                JsonElement details = kind.ValueKind == JsonValueKind.Object ? kind.EnumerateObject().Single().Value : default;
                return FormatLexical(value, name,
                    details.ValueKind == JsonValueKind.Object && details.TryGetProperty("min_digits", out var min) ? min.GetByte() : (byte)0,
                    details.ValueKind == JsonValueKind.Object && details.TryGetProperty("max_digits", out var max) ? max.GetByte() : (byte)0,
                    details.ValueKind == JsonValueKind.Object && details.TryGetProperty("max_chars", out var chars) ? chars.GetByte() : (byte)0);
            });
        }
        foreach (var constraint in Array(_options, "edi_value_constraints"))
        {
            string[] path = Array(constraint, "path").Select(value => value.GetString()!).ToArray();
            _ = Transform(source, path, 0, value => {
                if (value.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull) return value;
                string text = value.Kind == FerruleValueKind.Double ? FerruleValueMaps.RustFloatText(value.DoubleValue) : value.ToString();
                int count = text.EnumerateRunes().Count();
                string[] allowed = Array(constraint, "allowed_values").Select(item => item.GetString()!).ToArray();
                if (count < constraint.GetProperty("min_chars").GetInt32() || count > constraint.GetProperty("max_chars").GetInt32() || allowed.Length > 0 && !allowed.Contains(text, StringComparer.Ordinal))
                    throw Fail(FerruleEdiError.Constraint, "EDI output value violates its length or code list.");
                return value;
            });
        }
        ValidateValue(_root, source);
        if (_kind == "Idoc") return WriteIdoc(source);
        Syntax syntax = OutputSyntax(source);
        var segments = new List<OutputSegment>();
        Collect(_root, source, true, ref syntax, segments);
        if (_options.TryGetProperty("edi_autocomplete", out var autocomplete) && autocomplete.ValueKind == JsonValueKind.Object)
            segments = Complete(segments, autocomplete, currentDatetime, syntax);
        var output = new StringBuilder();
        int bytes = 0;
        bool completeIsa = HasCompleteIsa(_root, true);
        foreach (var segment in segments)
        {
            var elements = segment.Elements;
            if (_kind == "X12" && segment.Id == "ISA") EncodeIsa(elements, syntax);
            if (_kind == "X12" && completeIsa && segment.Id == "IEA" && elements.Count > 1 && elements[1].Length is > 0 and < 9 && elements[1].All(char.IsAsciiDigit)) elements[1] = elements[1].PadLeft(9, '0');
            if (segment.Id != "ISA") while (elements.Count > 0 && elements[^1].Length == 0) elements.RemoveAt(elements.Count - 1);
            string text;
            if (_kind == "Hl7" && segment.Id is "FHS" or "BHS" or "MSH") text = segment.Id + string.Concat(elements.Take(2)) + (elements.Count > 2 ? syntax.Element + string.Join(syntax.Element, elements.Skip(2)) : "");
            else text = segment.Id + (elements.Count == 0 ? "" : (_kind == "Tradacoms" ? '=' : syntax.Element) + string.Join(syntax.Element, elements));
            text += syntax.Terminator + (_kind == "Hl7" ? "" : "\n");
            bytes = checked(bytes + Utf8.GetByteCount(text));
            if (bytes > MaximumBytes) throw Fail(FerruleEdiError.Limit, "EDI output exceeds 64 MiB.");
            output.Append(text);
        }
        byte[] result = Utf8.GetBytes(output.ToString());
        if (_strictEnvelope) ValidateEnvelope(result);
        return result;
    }

    private static FerruleValue FormatLexical(FerruleValue value, string kind, byte minimum, byte maximum, byte characters)
    {
        try { return FerruleX12.FormatDocumentLexical(value, kind, minimum, maximum, characters); }
        catch (FerruleX12Exception) { throw Fail(FerruleEdiError.Value, "EDI lexical value cannot be formatted."); }
    }

    private static void ValidateValue(Node node, FerruleInstance value, bool item = false)
    {
        if (node.Repeating && !item)
        {
            if (value is not FerruleRepeated repeated) throw Fail(FerruleEdiError.Schema, "Expected repeating EDI value.");
            foreach (var member in repeated.Items) ValidateValue(node, member, true);
        }
        else if (node.Type is not null)
        {
            if (value is not FerruleScalar scalar) throw Fail(FerruleEdiError.Schema, "Expected EDI scalar.");
            _ = ScalarText(node, scalar.Value);
        }
        else
        {
            if (value is not FerruleGroup group) throw Fail(FerruleEdiError.Schema, "Expected EDI group.");
            foreach (var field in group.Fields)
            {
                Node child = node.Children.FirstOrDefault(child => child.Name == field.Name) ?? throw Fail(FerruleEdiError.Schema, "Undeclared EDI field.");
                ValidateValue(child, field.Value);
            }
        }
    }

    private static string ScalarText(Node node, FerruleValue value)
    {
        bool missing = value.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull || value.Kind == FerruleValueKind.String && value.StringValue.Length == 0;
        if (missing) return node.Fixed ?? "";
        string text = value.Kind switch {
            FerruleValueKind.Double when double.IsFinite(value.DoubleValue) => FerruleValueMaps.RustFloatText(value.DoubleValue),
            FerruleValueKind.String or FerruleValueKind.Int64 or FerruleValueKind.Bool => value.ToString(),
            _ => throw Fail(FerruleEdiError.Value, "Invalid EDI scalar value.") };
        if (node.Type == "int")
        {
            if (value.Kind is not (FerruleValueKind.Int64 or FerruleValueKind.String) || !long.TryParse(text.Trim(), NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out long integer)) throw Fail(FerruleEdiError.Value, "Invalid EDI integer.");
            text = integer.ToString(CultureInfo.InvariantCulture);
        }
        if (node.Type == "float")
        {
            if (value.Kind == FerruleValueKind.Bool || !double.TryParse(text.Trim(), NumberStyles.Float, CultureInfo.InvariantCulture, out double number) || !double.IsFinite(number) || value.Kind == FerruleValueKind.Int64 && (number < long.MinValue || number >= 9223372036854775808d || (long)number != value.Int64Value)) throw Fail(FerruleEdiError.Value, "Invalid EDI number.");
            text = FerruleValueMaps.RustFloatText(number);
        }
        if (node.Type == "bool" && value.Kind != FerruleValueKind.Bool && (value.Kind != FerruleValueKind.String || text.Trim() is not ("true" or "false"))) throw Fail(FerruleEdiError.Value, "Invalid EDI boolean.");
        if (node.Type == "bool") text = text.Trim();
        if (node.Fixed is { } fixedText)
        {
            bool equal = node.Type switch {
                "float" => double.TryParse(fixedText, NumberStyles.Float, CultureInfo.InvariantCulture, out double fixedNumber) && double.Parse(text, CultureInfo.InvariantCulture) == fixedNumber,
                "int" => long.TryParse(fixedText, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out long fixedInteger) && long.Parse(text, CultureInfo.InvariantCulture) == fixedInteger,
                "bool" => text == fixedText.Trim(),
                _ => text == fixedText };
            if (!equal) throw Fail(FerruleEdiError.Value, "EDI fixed value mismatch.");
            return fixedText;
        }
        if (Utf8.GetByteCount(text) > MaximumBytes) throw Fail(FerruleEdiError.Limit, "EDI field exceeds document limit.");
        return text;
    }

    private Syntax OutputSyntax(FerruleInstance source)
    {
        if (_kind == "Hl7") return new('|', '^', '\r', '\\', '~', '&');
        if (_kind != "X12") return new('+', ':', '\'', '?', DefaultEdifactV4() ? '*' : null);
        char element = '*', component = ':', terminator = '~'; char? repetition = '^', release = null;
        bool explicitSyntax = _options.TryGetProperty("x12_separators", out var configured) && configured.ValueKind == JsonValueKind.Object;
        if (explicitSyntax)
        {
            element = Text(configured, "element")![0]; component = Text(configured, "component")![0]; terminator = Text(configured, "segment")![0];
            repetition = Text(configured, "repetition")?.Single(); release = Text(configured, "release")?.Single();
        }
        string? version = Text(_options, "x12_interchange_version");
        bool foundIsa = false;
        FindVersion(_root, source, true);
        if (foundIsa && version is { Length: 5 } && string.CompareOrdinal(version, "00501") >= 0) {
            if (repetition is null && explicitSyntax) throw Fail(FerruleEdiError.Value, "Modern X12 requires a repetition separator.");
            repetition ??= '^';
        } else if (foundIsa) repetition = null;
        return new(element, component, terminator, release, repetition);
        void FindVersion(Node node, FerruleInstance? value, bool root = false)
        {
            if (foundIsa || value is null) return;
            if (value is FerruleRepeated repeated) { foreach (var item in repeated.Items) FindVersion(node, item, root); return; }
            if (!root && Id(node.Name) == "ISA")
            {
                foundIsa = true;
                Node? field = node.Children.ElementAtOrDefault(11);
                string found = field is null ? "" : ScalarText(field, (Field(value, field.Name) as FerruleScalar)?.Value ?? FerruleValue.Null);
                if (found.Length > 0) {
                    string? selected = Text(_options, "x12_interchange_version");
                    if (selected is not null && found != selected) throw Fail(FerruleEdiError.Value, "ISA12 conflicts with the selected interchange version.");
                    version = found;
                }
                if (version is null || version.Length != 5 || !version.All(char.IsAsciiDigit)) throw Fail(FerruleEdiError.Value, "Invalid ISA12 version.");
                if (string.CompareOrdinal(version, "00501") < 0) {
                    Node? standardField = node.Children.ElementAtOrDefault(10);
                    string standard = standardField is null ? "" : ScalarText(standardField, (Field(value, standardField.Name) as FerruleScalar)?.Value ?? FerruleValue.Null);
                    if (standard.Length == 0 || !standard.All(char.IsLetterOrDigit)) throw Fail(FerruleEdiError.Value, "Legacy ISA11 must be alphanumeric.");
                }
                return;
            }
            foreach (var child in node.Children) FindVersion(child, Field(value, child.Name));
        }
    }

    private bool DefaultEdifactV4() => _kind == "Edifact" && _options.TryGetProperty("edi_autocomplete", out var completion)
        && completion.ValueKind == JsonValueKind.Object && completion.TryGetProperty("edifact", out var options)
        && IsEdifactV4(Text(options, "syntax_version"));

    private void Collect(Node node, FerruleInstance value, bool root, ref Syntax syntax, List<OutputSegment> output)
    {
        if (value is FerruleRepeated repeated) { foreach (var item in repeated.Items) Collect(node, item, root, ref syntax, output); return; }
        if (!root && Id(node.Name) is { } id)
        {
            if (_kind == "Hl7" && (id is "FHS" or "BHS" or "MSH") && node.Children.Length < 2)
                throw Fail(FerruleEdiError.Schema, "HL7 headers require separator and encoding fields.");
            if (_kind == "Edifact" && id == "UNB")
            {
                Node? composite = node.Children.FirstOrDefault();
                Node? version = composite?.Children.ElementAtOrDefault(1);
                string? text = version is null || composite is null ? null : ScalarText(version, (Field(Field(value, composite.Name), version.Name) as FerruleScalar)?.Value ?? FerruleValue.Null);
                syntax = syntax with { Repetition = IsEdifactV4(text, DefaultEdifactV4()) ? '*' : null };
            }
            var elements = new List<string>();
            for (int at = 0; at < node.Children.Length; at++)
            {
                var child = node.Children[at]; var item = Field(value, child.Name);
                if (_kind == "Hl7" && id is "FHS" or "BHS" or "MSH" && at < 2)
                {
                    string expected = at == 0 ? "|" : "^~\\&";
                    string found = ScalarText(child, (item as FerruleScalar)?.Value ?? FerruleValue.Null);
                    if (found.Length > 0 && found != expected) throw Fail(FerruleEdiError.Value, "HL7 header separator mismatch.");
                    elements.Add(expected); continue;
                }
                char? allowed = id == "ISA" ? at == 10 ? syntax.Repetition : at == 15 ? syntax.Component : null : null;
                string? fallback = id == "ISA" ? at == 10 ? syntax.Repetition?.ToString() : at == 11 ? Text(_options, "x12_interchange_version") : at == 15 ? syntax.Component.ToString() : null : null;
                elements.Add(WriteElement(child, item, syntax, allowed, fallback));
            }
            bool completing = _options.TryGetProperty("edi_autocomplete", out var completion) && completion.ValueKind == JsonValueKind.Object;
            if (_kind == "X12" && id == "ISA" && !completing) EncodeIsa(elements, syntax);
            output.Add(new(id, elements));
        }
        else foreach (var child in node.Children) if (Field(value, child.Name) is { } item) Collect(child, item, false, ref syntax, output);
    }

    private string WriteElement(Node node, FerruleInstance? value, Syntax syntax, char? allowed = null, string? fallback = null, int depth = 0)
    {
        if (node.Repeating && depth > 0) throw Fail(FerruleEdiError.Schema, "EDI components cannot repeat.");
        if (value is FerruleRepeated repeated)
        {
            if (syntax.Repetition is null) throw Fail(FerruleEdiError.Schema, "Dialect has no element repetition separator.");
            return string.Join(syntax.Repetition?.ToString() ?? "", repeated.Items.Select(item => WriteSingle(node, item, syntax, allowed, fallback, depth)));
        }
        return WriteSingle(node, value, syntax, allowed, fallback, depth);
    }
    private string WriteSingle(Node node, FerruleInstance? value, Syntax syntax, char? allowed, string? fallback, int depth)
    {
        if (node.Type is not null)
        {
            string text = ScalarText(node, (value as FerruleScalar)?.Value ?? FerruleValue.Null);
            if (text.Length == 0) text = fallback ?? "";
            if (allowed is { } delimiter && text.Length > 0 && text != delimiter.ToString()) throw Fail(FerruleEdiError.Value, "ISA separator mismatch.");
            return Escape(text, syntax, allowed);
        }
        if (depth > 0 && (_kind != "Hl7" || depth > 1)) throw Fail(FerruleEdiError.Schema, "Unsupported nested EDI composite.");
        var parts = node.Children.Select(child => WriteElement(child, Field(value, child.Name), syntax, depth: depth + 1)).ToList();
        while (parts.Count > 0 && parts[^1].Length == 0) parts.RemoveAt(parts.Count - 1);
        return string.Join(depth == 0 ? syntax.Component : syntax.Subcomponent!.Value, parts);
    }
    private string Escape(string text, Syntax syntax, char? allowed)
    {
        if (text.Length == 1 && text[0] == allowed) return text;
        var output = new StringBuilder();
        foreach (char c in text)
        {
            bool reserved = c == syntax.Element || c == syntax.Component || c == syntax.Terminator || c == syntax.Repetition || _kind == "Tradacoms" && c == '=';
            if (_kind == "Hl7")
            {
                char? code = c == syntax.Element ? 'F' : c == syntax.Component ? 'S' : c == syntax.Repetition ? 'R' : c == syntax.Release ? 'E' : c == syntax.Subcomponent ? 'T' : null;
                if (code is { } escaped) output.Append(syntax.Release).Append(escaped).Append(syntax.Release); else output.Append(c);
            }
            else
            {
                if (reserved && syntax.Release is null) throw Fail(FerruleEdiError.Value, "Unescapable EDI delimiter.");
                if (reserved || c == syntax.Release) output.Append(syntax.Release);
                output.Append(c);
            }
        }
        return output.ToString();
    }

    private static bool HasCompleteIsa(Node node, bool root = false)
    {
        if (!root && Id(node.Name) is { } id) return id == "ISA" && node.Children.Length == 16;
        return node.Children.Any(child => HasCompleteIsa(child));
    }

    private static void EncodeIsa(List<string> elements, Syntax syntax)
    {
        if (elements.Count != 16) return;
        if (syntax.Element > 127 || syntax.Component > 127 || syntax.Terminator > 127 || syntax.Repetition is > (char)127)
            throw Fail(FerruleEdiError.Value, "Complete ISA requires ASCII syntax separators.");
        int[] widths = [2, 10, 2, 10, 2, 15, 2, 15, 6, 4, 1, 5, 9, 1, 1, 1];
        for (int at = 0; at < elements.Count; at++)
        {
            string value = elements[at]; bool pad = at is 1 or 3 or 5 or 7;
            if (value.Any(c => c < ' ' || c > '~' || c == syntax.Element || c == syntax.Terminator || c == syntax.Release || at != 15 && c == syntax.Component || at != 10 && c == syntax.Repetition)
                || value.Length > widths[at] || !pad && value.Length != widths[at]) throw Fail(FerruleEdiError.Value, "Invalid fixed-width ISA element.");
            elements[at] = pad ? value.PadRight(widths[at]) : value;
        }
    }

    private byte[] WriteIdoc(FerruleInstance source)
    {
        using var output = new System.IO.MemoryStream(); int records = 0;
        Write(_root, source, true);
        return output.ToArray();
        void Write(Node node, FerruleInstance value, bool root)
        {
            if (value is FerruleRepeated repeated) { foreach (var item in repeated.Items) Write(node, item, root); return; }
            if (!root && _layouts.TryGetValue(node.Name, out var layout))
            {
                if (++records > 100_000) throw Fail(FerruleEdiError.Limit, "IDoc record limit exceeded.");
                var fields = Array(layout, "fields"); byte[] name = Utf8.GetBytes(node.Name);
                int size = Math.Max(name.Length, fields.Max(field => field.GetProperty("last_byte").GetInt32()));
                if (size > 1024 * 1024 || output.Length + size + 2 > MaximumBytes) throw Fail(FerruleEdiError.Limit, "IDoc output limit exceeded.");
                byte[] record = Enumerable.Repeat((byte)32, size).ToArray(); bool[] used = new bool[size];
                name.CopyTo(record, 0); System.Array.Fill(used, true, 0, name.Length);
                for (int at = 0; at < fields.Length; at++)
                {
                    var field = fields[at]; Node child = node.Children[at];
                    int start = field.GetProperty("first_byte").GetInt32() - 1, width = field.GetProperty("last_byte").GetInt32() - start;
                    if (used.AsSpan(start, width).Contains(true)) throw Fail(FerruleEdiError.Schema, "Overlapping IDoc fields.");
                    System.Array.Fill(used, true, start, width);
                    string text = ScalarText(child, (Field(value, child.Name) as FerruleScalar)?.Value ?? FerruleValue.Null);
                    if (text.Any(c => char.IsControl(c))) throw Fail(FerruleEdiError.Value, "Control text in IDoc field.");
                    byte[] bytes = Utf8.GetBytes(text);
                    if (bytes.Length > width) throw new FerruleEdiException(FerruleEdiError.FieldWidth, "IDoc field exceeds layout width.", bytes.Length, width, child.Name);
                    bytes.CopyTo(record, child.Type is "int" or "float" ? start + width - bytes.Length : start);
                }
                output.Write(record); output.Write("\r\n"u8);
            }
            else foreach (Node child in node.Children) if (Field(value, child.Name) is { } item) Write(child, item, false);
        }
    }
}
