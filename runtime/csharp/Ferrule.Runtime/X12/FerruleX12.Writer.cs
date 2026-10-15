using System.Globalization;

namespace Ferrule.Runtime;

public static partial class FerruleX12
{
    private static void WriteContainer(Node node, FerruleInstance instance, Profile profile, string[] path,
        List<Segment> segments, Budget budget, int depth, bool root = false)
    {
        budget.Visit(depth);
        if (instance is not FerruleGroup group) throw Failure(FerruleX12Error.Value, "X12 requires a group instance.", path: string.Join('/', path));
        if (group.Fields.Any(field => !node.Children.Any(child => child.Name == field.Name)))
            throw Failure(FerruleX12Error.Value, "X12 instance contains an undeclared field.", path: string.Join('/', path));
        string? id = root ? null : SegmentId(node.Name);
        if (id is not null)
        {
            if (segments.Count == MaximumSegments) throw Limit("X12 output segment limit exceeded.");
            Syntax syntax = profile.OutputSyntax;
            bool mapforce = profile.MapForce is not null;
            int control = id switch { "ISA" => 12, "GS" => 5, "ST" or "SE" or "GE" or "IEA" => 1, _ => -1 };
            // MapForce controls may be typed, completed on the wire, or refused by final validation.
            if (!mapforce && control >= 0 && (!group.TryGetField(node.Children[control].Name, out FerruleInstance? supplied)
                || supplied is not FerruleScalar { Value.Kind: FerruleValueKind.String } scalar || scalar.Value.StringValue.Length == 0))
                throw Failure(FerruleX12Error.Envelope, "X12 control numbers must be supplied explicitly.", path: string.Join('/', path));
            var elements = new string[node.Children.Length];
            long textBytes = 0;
            long wireBytes = id.Length + 1 + (syntax.Segment == '\n' ? 0 : 1);
            for (int index = 0; index < elements.Length; index++)
            {
                Node child = node.Children[index];
                FerruleInstance? value = group.TryGetField(child.Name, out FerruleInstance? suppliedValue) ? suppliedValue : null;
                // A modern ISA11 default is the explicitly selected physical
                // syntax, never inferred from a supplied nonempty value.
                if (id == "ISA" && index == 10 && syntax.Repetition is { } repetition && Unsupplied(value))
                    value = new FerruleScalar(FerruleValue.FromString(child.Fixed is { Length: > 0 } fixedValue
                        ? fixedValue : repetition.ToString()));
                // Like the native writer, MapForce interchanges take an empty
                // ISA12 from the selected version and an empty ISA16 from the syntax.
                if (mapforce && id == "ISA" && index is 11 or 15 && Unsupplied(value))
                    value = new FerruleScalar(FerruleValue.FromString(child.Fixed is { Length: > 0 } fixedDefault ? fixedDefault
                        : index == 11 ? profile.SelectedVersion.Interchange : syntax.Component.ToString()));
                long remaining = Math.Max(0, budget.RemainingOutput - id.Length - 1 - (syntax.Segment == '\n' ? 0 : 1)
                    - textBytes - index - 1);
                string text = child.Repeating
                    ? WriteRepeatedElement(child, value, profile, [.. path, child.Name], budget, depth + 1, remaining)
                    : WriteElement(child, value, syntax, [.. path, child.Name], budget, depth + 1, id == "ISA" && index == 15,
                        remaining, id == "ISA" && index == 10,
                        deferEmpty: mapforce && profile.Autocomplete is not null && MapForceCompletionField(id, index));
                // Typed controls become integer wire text; completion and
                // final validation never reinterpret a signed or fractional number.
                if (mapforce && IsEnvelope(id) && child.Type is "int" or "float" && text.Length > 0 && !text.All(char.IsAsciiDigit))
                    throw Failure(FerruleX12Error.Value, "MapForce X12 numeric envelope controls require non-negative integers.",
                        path: string.Join('/', [.. path, child.Name]));
                elements[index] = text;
                textBytes += StrictUtf8.GetByteCount(text);
                if (id == "ISA" && index is 1 or 3 or 5 or 7)
                    textBytes += Math.Max(0, IsaWidths[index] - text.Length);
                // Trailing empty body elements are omitted, but internal empty
                // positions and every ISA delimiter still occupy wire bytes.
                if (id == "ISA" || text.Length > 0)
                    wireBytes = id.Length + 1 + (syntax.Segment == '\n' ? 0 : 1) + textBytes + index + 1;
                budget.CheckOutput(wireBytes);
            }
            budget.AddOutput(wireBytes);
            segments.Add(new(id, elements, node.Children.Any(child => child.Repeating)
                ? node.Children.Select(child => child.Repeating).ToArray() : null));
            return;
        }
        foreach (Node child in node.Children)
        {
            group.TryGetField(child.Name, out FerruleInstance? value);
            string[] childPath = [.. path, child.Name];
            if (child.Repeating)
            {
                IReadOnlyList<FerruleInstance> items = value switch
                {
                    null => [],
                    FerruleRepeated repeated => repeated.Items,
                    FerruleMappedSequence sequence => sequence.Items,
                    _ => throw Failure(FerruleX12Error.Value, "X12 repeating fields require an ordered collection.", path: string.Join('/', childPath)),
                };
                if (items.Count > MaximumLoopInstances) throw Limit("X12 output loop instance limit exceeded.");
                ValidateCount(child, items.Count, childPath);
                // Each interchange or group occurrence owns exactly one header
                // and at most one trailer; completion never merges owners.
                if (profile.MapForce is { } shape)
                {
                    if ((child == shape.Isa || child == shape.Gs) && items.Count != 1)
                        throw Failure(FerruleX12Error.Envelope, "MapForce X12 output requires exactly one header per owner.",
                            segments.Count, string.Join('/', childPath));
                    if ((child == shape.Ge || child == shape.Iea || child == shape.Se) && items.Count > 1)
                        throw Failure(FerruleX12Error.Envelope, "MapForce X12 output permits at most one trailer per owner.",
                            segments.Count, string.Join('/', childPath));
                }
                for (int itemIndex = 0; itemIndex < items.Count; itemIndex++)
                {
                    budget.Loop();
                    string[] itemPath = profile.IndexedPaths
                        ? [.. path, child.Name + "[" + itemIndex.ToString(CultureInfo.InvariantCulture) + "]"] : childPath;
                    WriteContainer(child, items[itemIndex], profile, itemPath, segments, budget, depth + 1);
                }
            }
            else
            {
                // Selected completion creates a missing SE, as the native wire completion does.
                if (value is null && profile.Autocomplete is not null && profile.MapForce is { } shape && child == shape.Se)
                    value = new FerruleGroup([]);
                WriteContainer(child, value ?? throw Failure(FerruleX12Error.Value, "X12 output is missing a required group.", path: string.Join('/', childPath)),
                    profile, childPath, segments, budget, depth + 1);
            }
        }
    }

    private static bool Unsupplied(FerruleInstance? value)
        => value is null or FerruleScalar { Value.Kind: FerruleValueKind.Null or FerruleValueKind.JsonNull }
            || value is FerruleScalar { Value.Kind: FerruleValueKind.String } empty && empty.Value.StringValue.Length == 0;

    // A missing or Null repeating element has no occurrences. Each occurrence
    // keeps the ordinary scalar or composite rules before repetition joins it.
    private static string WriteRepeatedElement(Node node, FerruleInstance? instance, Profile profile, string[] path,
        Budget budget, int depth, long maximumBytes)
    {
        budget.Visit(depth);
        IReadOnlyList<FerruleInstance> items = instance switch
        {
            null or FerruleScalar { Value.Kind: FerruleValueKind.Null or FerruleValueKind.JsonNull } => [],
            FerruleRepeated repeated => repeated.Items,
            FerruleMappedSequence sequence => sequence.Items,
            _ => throw Failure(FerruleX12Error.Value, "X12 repeating fields require an ordered collection.", path: string.Join('/', path)),
        };
        if (items.Count > MaximumLoopInstances) throw Limit("X12 output loop instance limit exceeded.");
        Syntax syntax = profile.OutputSyntax;
        char repetition = syntax.Repetition
            ?? throw Failure(FerruleX12Error.UnsupportedProfile, "Modern X12 output requires a selected repetition separator.");
        var occurrences = new List<string>(items.Count);
        long bytes = 0;
        for (int index = 0; index < items.Count; index++)
        {
            budget.Loop();
            string[] itemPath = profile.IndexedPaths
                ? [.. path[..^1], path[^1] + "[" + index.ToString(CultureInfo.InvariantCulture) + "]"] : path;
            string text = WriteElement(node, items[index], syntax, itemPath, budget, depth + 1,
                maximumBytes: Math.Max(0, maximumBytes - bytes - (index > 0 ? 1 : 0)));
            bytes += StrictUtf8.GetByteCount(text) + (index > 0 ? 1 : 0);
            if (bytes > maximumBytes) throw Limit("X12 repeated element byte limit exceeded.");
            occurrences.Add(text);
        }
        return string.Join(repetition, occurrences);
    }

    // A deferred empty field is completion-owned: its constraints apply to the
    // supplied value only, as native lexical validation precedes completion.
    private static string WriteElement(Node node, FerruleInstance? instance, Syntax syntax, string[] path,
        Budget budget, int depth, bool isaComponent = false, long maximumBytes = MaximumDocumentBytes, bool isaRepetition = false,
        bool deferEmpty = false)
    {
        budget.Visit(depth);
        if (node.Type is null)
        {
            if (instance is not null && instance is not FerruleGroup) throw Failure(FerruleX12Error.Value, "X12 composite requires a group.", path: string.Join('/', path));
            var group = instance as FerruleGroup;
            if (group is not null && group.Fields.Any(field => !node.Children.Any(child => child.Name == field.Name)))
                throw Failure(FerruleX12Error.Value, "X12 composite contains an undeclared field.", path: string.Join('/', path));
            var components = node.Children.Select(child => WriteElement(child,
                group is not null && group.TryGetField(child.Name, out FerruleInstance? value) ? value : null,
                syntax, [.. path, child.Name], budget, depth + 1)).ToList();
            while (components.Count > 0 && components[^1].Length == 0) components.RemoveAt(components.Count - 1);
            long bytes = Math.Max(0, components.Count - 1);
            foreach (string component in components)
            {
                bytes += StrictUtf8.GetByteCount(component);
                if (bytes > maximumBytes) throw Limit("X12 composite byte limit exceeded.");
            }
            return string.Join(syntax.Component, components);
        }
        if (instance is not null && instance is not FerruleScalar) throw Failure(FerruleX12Error.Value, "X12 leaf requires a scalar.", path: string.Join('/', path));
        FerruleValue scalar = instance is FerruleScalar leaf ? leaf.Value : FerruleValue.Null;
        string text;
        if (scalar.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull
            || scalar.Kind == FerruleValueKind.String && scalar.StringValue.Length == 0 && node.Fixed is not null) text = node.Fixed ?? "";
        else if (node.Type == "string" && scalar.Kind == FerruleValueKind.String) text = scalar.StringValue;
        else if (node.Type == "int" && scalar.Kind == FerruleValueKind.Int64) text = scalar.Int64Value.ToString(CultureInfo.InvariantCulture);
        else if (node.Type == "float" && scalar.Kind is FerruleValueKind.Double or FerruleValueKind.Int64)
        {
            double number = scalar.Kind == FerruleValueKind.Double ? scalar.DoubleValue : scalar.Int64Value;
            if (!double.IsFinite(number)) throw Failure(FerruleX12Error.Value, "X12 numbers must be finite.", path: string.Join('/', path));
            text = FerruleValueMaps.RustFloatText(number);
        }
        else throw Failure(FerruleX12Error.Value, "X12 scalar type does not match the schema.", path: string.Join('/', path));
        if (node.Fixed is { } fixedText && text != fixedText && node.Type is not null and not "string")
        {
            FerruleValue fixedValue = ReadScalar(node, fixedText, path);
            bool same = node.Type == "int"
                ? long.TryParse(text, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out long integer)
                    && fixedValue.Kind == FerruleValueKind.Int64 && fixedValue.Int64Value == integer
                : double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out double number)
                    && fixedValue.Kind == FerruleValueKind.Double && fixedValue.DoubleValue == number;
            if (same) text = fixedText;
        }
        RequireUtf8(text, MaximumDocumentBytes, "X12 field");
        if (!deferEmpty || text.Length > 0) ValidateText(node, text, path);
        if (!(isaComponent && text == syntax.Component.ToString())
            && !(isaRepetition && syntax.Repetition is { } repetition && text == repetition.ToString())
            && text.Any(character => char.IsControl(character) || character == syntax.Element || character == syntax.Component
                || character == syntax.Segment || character == syntax.Repetition))
            throw Failure(FerruleX12Error.Value, "X12 scalar contains an unrepresentable delimiter.", path: string.Join('/', path));
        return text;
    }
}
