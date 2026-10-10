using System.Globalization;

namespace Ferrule.Runtime;

public static partial class FerruleX12
{
    private sealed record CompletionSegment(Node Node, FerruleGroup Group, string[] Path);
    private sealed record CompletionTimestamp(string Date8, string Date6, string Time4, string Time6);

    private static FerruleInstance CompleteOutputView(Profile profile, FerruleInstance document, FerruleExecutionContext? executionContext)
    {
        CompletionTimestamp timestamp = ReadCompletionTimestamp(executionContext?.CurrentDateTime);
        var segments = new List<CompletionSegment>();
        FerruleInstance populated = PopulateTrailerGroups(profile.Root, document, [], segments, new Budget(), 0, root: true);
        var replacements = new Dictionary<Node, FerruleGroup>();
        var envelopes = segments.Where(segment => IsEnvelope(SegmentId(segment.Node.Name) ?? ""))
            .ToDictionary(segment => SegmentId(segment.Node.Name)!);
        if (!envelopes.TryGetValue("ISA", out CompletionSegment? isa)
            || !envelopes.TryGetValue("GS", out CompletionSegment? gs)
            || !envelopes.TryGetValue("ST", out CompletionSegment? st))
            return populated;

        FerruleGroup isaGroup = isa.Group;
        string[] isaDefaults = ["00", "          ", "00", "          ", "ZZ", "", "ZZ", "",
            timestamp.Date6, timestamp.Time4, "", "", "000000000", profile.Autocomplete!.RequestAcknowledgement ? "1" : "0", "P", ""];
        for (int index = 0; index < isaDefaults.Length; index++) isaGroup = FillCompletionField(isa.Node, isaGroup, index, isaDefaults[index]);
        isaGroup = NormalizeCompletionDate(isa.Node, isaGroup, 8, shortDate: true);
        isaGroup = NormalizeCompletionTime(isa.Node, isaGroup, 9, shortTime: true);
        replacements.Add(isa.Node, isaGroup);

        FerruleGroup gsGroup = FillCompletionField(gs.Node, gs.Group, 3, timestamp.Date8);
        gsGroup = FillCompletionField(gs.Node, gsGroup, 4, timestamp.Time6);
        gsGroup = NormalizeCompletionDate(gs.Node, gsGroup, 3, shortDate: false);
        gsGroup = NormalizeCompletionTime(gs.Node, gsGroup, 4, shortTime: false);
        gsGroup = FillCompletionField(gs.Node, gsGroup, 5, "1");
        replacements.Add(gs.Node, gsGroup);

        FerruleGroup stGroup = FillCompletionField(st.Node, st.Group, 0, profile.Autocomplete.TransactionSet ?? "");
        stGroup = FillCompletionField(st.Node, stGroup, 1, "0001");
        replacements.Add(st.Node, stGroup);

        if (envelopes.TryGetValue("SE", out CompletionSegment? se))
        {
            int count = segments.IndexOf(se) - segments.IndexOf(st) + 1;
            FerruleGroup group = FillCompletionField(se.Node, se.Group, 0, count.ToString(CultureInfo.InvariantCulture));
            replacements.Add(se.Node, FillCompletionField(se.Node, group, 1, CompletionText(st.Node, stGroup, 1)));
        }
        if (envelopes.TryGetValue("GE", out CompletionSegment? ge))
        {
            FerruleGroup group = FillCompletionField(ge.Node, ge.Group, 0, "1");
            replacements.Add(ge.Node, FillCompletionField(ge.Node, group, 1, CompletionText(gs.Node, gsGroup, 5)));
        }
        if (envelopes.TryGetValue("IEA", out CompletionSegment? iea))
        {
            FerruleGroup group = FillCompletionField(iea.Node, iea.Group, 0, "1");
            replacements.Add(iea.Node, FillCompletionField(iea.Node, group, 1, CompletionText(isa.Node, isaGroup, 12)));
        }
        return ReplaceCompletionGroups(profile.Root, populated, replacements, new Budget(), 0, root: true);
    }

    private static CompletionTimestamp ReadCompletionTimestamp(string? value)
    {
        if (value is null)
            throw Failure(FerruleX12Error.Envelope, "X12 completion requires a supplied current dateTime.");
        RequireUtf8(value, MaximumDocumentBytes, "X12 completion context");
        bool valid = value.Length >= 19 && value[4] == '-' && value[7] == '-' && value[10] == 'T'
            && value[13] == ':' && value[16] == ':' && Digits(value[..4]) && Digits(value[5..7]) && Digits(value[8..10])
            && Digits(value[11..13]) && Digits(value[14..16]) && Digits(value[17..19]);
        int cursor = 19;
        if (valid && cursor < value.Length && value[cursor] == '.')
        {
            int first = ++cursor;
            while (cursor < value.Length && char.IsAsciiDigit(value[cursor])) cursor++;
            valid = cursor > first;
        }
        valid = valid && ValidTimezone(value[cursor..])
            && DateOnly.TryParseExact(value[..10], "yyyy-MM-dd", CultureInfo.InvariantCulture, DateTimeStyles.None, out _)
            && int.Parse(value[11..13], CultureInfo.InvariantCulture) <= 23
            && int.Parse(value[14..16], CultureInfo.InvariantCulture) <= 59
            && int.Parse(value[17..19], CultureInfo.InvariantCulture) <= 59;
        if (!valid)
            throw Failure(FerruleX12Error.Envelope, "X12 completion requires a valid supplied current dateTime.");
        return new(value[..4] + value[5..7] + value[8..10], value[2..4] + value[5..7] + value[8..10],
            value[11..13] + value[14..16], value[11..13] + value[14..16] + value[17..19]);
    }

    private static FerruleInstance PopulateTrailerGroups(Node node, FerruleInstance instance, string[] path,
        List<CompletionSegment> segments, Budget budget, int depth, bool root = false)
    {
        budget.Visit(depth);
        if (node.Type is not null || instance is not FerruleGroup group) return instance;
        string? id = root ? null : SegmentId(node.Name);
        if (id is not null)
        {
            if (segments.Count == MaximumSegments) throw Limit("X12 output segment limit exceeded.");
            if (IsEnvelope(id))
                for (int index = 0; index < group.Fields.Count; index++) budget.Visit(depth + 1);
            segments.Add(new(node, group, path));
            return instance;
        }
        var replacements = new Dictionary<string, FerruleInstance>(StringComparer.Ordinal);
        foreach (FerruleField field in group.Fields)
            if (!node.Children.Any(child => child.Name == field.Name)) budget.Visit(depth + 1);
        foreach (Node child in node.Children)
        {
            group.TryGetField(child.Name, out FerruleInstance? value);
            if (value is null && CanMaterializeTrailer(child)) value = new FerruleGroup([]);
            if (value is null) continue;
            string[] childPath = [.. path, child.Name];
            if (child.Repeating && value is FerruleRepeated or FerruleMappedSequence)
            {
                IReadOnlyList<FerruleInstance> items = value is FerruleRepeated repeated ? repeated.Items : ((FerruleMappedSequence)value).Items;
                if (items.Count > MaximumLoopInstances) throw Limit("X12 output loop instance limit exceeded.");
                var completed = new List<FerruleInstance>();
                foreach (FerruleInstance item in items)
                {
                    budget.Loop();
                    completed.Add(PopulateTrailerGroups(child, item, childPath, segments, budget, depth + 1));
                }
                replacements.Add(child.Name, value is FerruleRepeated ? new FerruleRepeated(completed) : new FerruleMappedSequence(completed));
            }
            else replacements.Add(child.Name, PopulateTrailerGroups(child, value, childPath, segments, budget, depth + 1));
        }
        var fields = group.Fields.Select(field => new FerruleField(field.Name, replacements.GetValueOrDefault(field.Name, field.Value))).ToList();
        foreach (Node child in node.Children)
            if (!group.TryGetField(child.Name, out _) && replacements.TryGetValue(child.Name, out FerruleInstance? value))
                fields.Add(new FerruleField(child.Name, value));
        return group.CloneFields(fields);
    }

    private static bool CanMaterializeTrailer(Node node)
        => !node.Repeating && node.Type is null && (SegmentId(node.Name) is { } id
            ? id is "SE" or "GE" or "IEA" : node.Children.All(CanMaterializeTrailer));

    private static FerruleInstance ReplaceCompletionGroups(Node node, FerruleInstance instance,
        Dictionary<Node, FerruleGroup> replacements, Budget budget, int depth, bool root = false)
    {
        budget.Visit(depth);
        if (!root && replacements.TryGetValue(node, out FerruleGroup? replacement)) return replacement;
        if (node.Type is not null || instance is not FerruleGroup group || !root && SegmentId(node.Name) is not null) return instance;
        return group.CloneFields(group.Fields.Select(field =>
        {
            Node? child = node.Children.FirstOrDefault(candidate => candidate.Name == field.Name);
            if (child is null) { budget.Visit(depth + 1); return field; }
            FerruleInstance value = field.Value;
            if (child.Repeating && value is FerruleRepeated or FerruleMappedSequence)
            {
                IReadOnlyList<FerruleInstance> items = value is FerruleRepeated repeated ? repeated.Items : ((FerruleMappedSequence)value).Items;
                if (items.Count > MaximumLoopInstances) throw Limit("X12 output loop instance limit exceeded.");
                var completed = new List<FerruleInstance>();
                foreach (FerruleInstance item in items)
                {
                    budget.Loop();
                    completed.Add(ReplaceCompletionGroups(child, item, replacements, budget, depth + 1));
                }
                value = value is FerruleRepeated ? new FerruleRepeated(completed) : new FerruleMappedSequence(completed);
            }
            else value = ReplaceCompletionGroups(child, value, replacements, budget, depth + 1);
            return new FerruleField(field.Name, value);
        }));
    }

    private static FerruleGroup FillCompletionField(Node node, FerruleGroup group, int index, string fallback)
    {
        string name = node.Children[index].Name;
        if (group.TryGetField(name, out FerruleInstance? instance)
            && instance is not FerruleScalar { Value.Kind: FerruleValueKind.Null or FerruleValueKind.JsonNull }
            && instance is not FerruleScalar { Value.Kind: FerruleValueKind.String, Value.StringValue.Length: 0 }) return group;
        return SetCompletionField(group, name, fallback);
    }

    private static string CompletionText(Node node, FerruleGroup group, int index)
    {
        Node child = node.Children[index];
        if (!group.TryGetField(child.Name, out FerruleInstance? instance)
            || instance is FerruleScalar { Value.Kind: FerruleValueKind.Null or FerruleValueKind.JsonNull }) return child.Fixed ?? "";
        return instance is FerruleScalar { Value.Kind: FerruleValueKind.String } scalar ? scalar.Value.StringValue : "";
    }

    private static FerruleGroup SetCompletionField(FerruleGroup group, string name, string value)
    {
        var scalar = new FerruleScalar(FerruleValue.FromString(value));
        var fields = group.Fields.Select(field => field.Name == name ? new FerruleField(name, scalar) : field).ToList();
        if (!group.TryGetField(name, out _)) fields.Add(new FerruleField(name, scalar));
        return group.CloneFields(fields);
    }

    private static FerruleGroup NormalizeCompletionDate(Node node, FerruleGroup group, int index, bool shortDate)
    {
        string value = CompletionText(node, group, index);
        RequireUtf8(value, MaximumDocumentBytes, "X12 field");
        if (value.Length == 10 && value[4] == '-' && value[7] == '-') value = value[..4] + value[5..7] + value[8..10];
        if (shortDate && value.Length == 8) value = value[2..];
        if (value.Length != (shortDate ? 6 : 8) || !Digits(value)) throw CompletionHeaderFailure();
        return SetCompletionField(group, node.Children[index].Name, value);
    }

    private static FerruleGroup NormalizeCompletionTime(Node node, FerruleGroup group, int index, bool shortTime)
    {
        string value = CompletionText(node, group, index);
        RequireUtf8(value, MaximumDocumentBytes, "X12 field");
        var compact = new System.Text.StringBuilder();
        foreach (char character in value)
        {
            if (character is '+' or '-' or 'Z') break;
            if (char.IsAsciiDigit(character))
            {
                if (compact.Length == 6) throw CompletionHeaderFailure();
                compact.Append(character);
            }
            else if (character is not (':' or '?')) throw CompletionHeaderFailure();
        }
        if (shortTime && compact.Length == 6) compact.Length = 4;
        if (compact.Length != (shortTime ? 4 : 6)) throw CompletionHeaderFailure();
        return SetCompletionField(group, node.Children[index].Name, compact.ToString());
    }

    private static FerruleX12Exception CompletionHeaderFailure()
        => Failure(FerruleX12Error.Envelope, "X12 completion requires valid header dates and times.");
}
