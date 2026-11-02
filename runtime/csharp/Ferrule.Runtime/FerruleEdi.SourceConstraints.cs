using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public enum FerruleEdiConstraintViolation { TooShort, TooLong, NotAllowed }

/// <summary>A source lexeme violation, without disclosing the input value.</summary>
public sealed record FerruleEdiValidationIssue(string Field, string Location,
    FerruleEdiConstraintViolation Violation, uint? Minimum = null, uint? Maximum = null,
    int? Actual = null, int? AllowedCount = null);

/// <summary>At most 10,000 retained issues; Truncated means further issues exist.</summary>
public sealed class FerruleEdiValidationReport
{
    public IReadOnlyList<FerruleEdiValidationIssue> Issues { get; }
    public bool Truncated { get; }
    internal FerruleEdiValidationReport(List<FerruleEdiValidationIssue> issues, bool truncated)
    {
        Issues = issues.AsReadOnly();
        Truncated = truncated;
    }
    public override string ToString()
    {
        var message = new StringBuilder($"EDI source validation found {Issues.Count} issue(s)");
        foreach (var issue in Issues.Take(8))
        {
            message.Append($"; EDI value `{issue.Location}` ");
            message.Append(issue.Violation switch
            {
                FerruleEdiConstraintViolation.TooShort => $"contains {issue.Actual} character(s), minimum is {issue.Minimum}",
                FerruleEdiConstraintViolation.TooLong => $"contains {issue.Actual} character(s), maximum is {issue.Maximum}",
                _ => $"is not one of the {issue.AllowedCount} configured code-list value(s)",
            });
        }
        if (Issues.Count > 8 || Truncated) message.Append("; additional issues omitted");
        return message.ToString();
    }
}

public sealed partial class FerruleEdi
{
    private sealed record SourceConstraint(Node Leaf, string Field, uint Minimum, uint Maximum, HashSet<string> Allowed);

    private SourceConstraint[] PrepareSourceConstraints()
    {
        if (!_options.TryGetProperty("edi_value_constraints", out var metadata)) return [];
        if (metadata.ValueKind != JsonValueKind.Array) throw Fail(FerruleEdiError.Descriptor, "EDI value constraints must be an array.");
        if (metadata.GetArrayLength() > 10_000) throw Fail(FerruleEdiError.Descriptor, "EDI constraint count exceeds 10,000.");
        var constraints = new List<SourceConstraint>();
        var seen = new HashSet<string>(StringComparer.Ordinal);
        foreach (var rule in metadata.EnumerateArray())
        {
            if (rule.ValueKind != JsonValueKind.Object || !rule.TryGetProperty("path", out var parts) || parts.ValueKind != JsonValueKind.Array)
                throw Fail(FerruleEdiError.Descriptor, "EDI constraint requires a schema path.");
            if (parts.GetArrayLength() is 0 or > 64 || parts.EnumerateArray().Any(part => part.ValueKind != JsonValueKind.String || string.IsNullOrEmpty(part.GetString())))
                throw Fail(FerruleEdiError.Descriptor, "Invalid EDI constraint path.");
            string[] path = parts.EnumerateArray().Select(part => part.GetString()!).ToArray();
            // JSON preserves the boundaries even when schema names contain NUL.
            if (!seen.Add(JsonSerializer.Serialize(path))) throw Fail(FerruleEdiError.Descriptor, "Duplicate EDI constraint path.");
            Node leaf = _root;
            foreach (string part in path)
                leaf = leaf.Children.FirstOrDefault(child => child.Name == part) ?? throw Fail(FerruleEdiError.Descriptor, "EDI constraint path is absent from schema.");
            if (leaf.Type is null) throw Fail(FerruleEdiError.Descriptor, "EDI constraint path must select a scalar.");
            if (!rule.TryGetProperty("min_chars", out var minimum) || minimum.ValueKind != JsonValueKind.Number || !minimum.TryGetUInt32(out uint min)
                || !rule.TryGetProperty("max_chars", out var maximum) || maximum.ValueKind != JsonValueKind.Number || !maximum.TryGetUInt32(out uint max)
                || max == 0 || min > max)
                throw Fail(FerruleEdiError.Descriptor, "EDI constraints require 0 <= min_chars <= max_chars and a positive maximum.");
            var allowed = new HashSet<string>(StringComparer.Ordinal);
            if (rule.TryGetProperty("allowed_values", out var codes))
            {
                if (codes.ValueKind != JsonValueKind.Array || codes.EnumerateArray().Any(code => code.ValueKind != JsonValueKind.String))
                    throw Fail(FerruleEdiError.Descriptor, "EDI code lists must contain strings.");
                foreach (var code in codes.EnumerateArray()) allowed.Add(code.GetString()!);
                if (allowed.Count > 4096) throw Fail(FerruleEdiError.Descriptor, "EDI code lists exceed 4096 distinct values.");
            }
            constraints.Add(new(leaf, string.Join('/', path), min, max, allowed));
        }
        return constraints.ToArray();
    }

    private sealed class SourceValidation
    {
        private const int MaximumIssues = 10_000;
        // Nodes are schema identities, not record-value equality: two leaves
        // named Code in different segments may have different constraints.
        private readonly Dictionary<Node, SourceConstraint> _rules = new(ReferenceEqualityComparer.Instance);
        private readonly List<FerruleEdiValidationIssue> _issues = [];
        private bool _truncated;
        public SourceValidation(SourceConstraint[] constraints)
        {
            foreach (var constraint in constraints) _rules.Add(constraint.Leaf, constraint);
        }
        public string Child(string location, string name) => _rules.Count == 0 ? "" : location.Length == 0 ? name : location + "/" + name;
        public string Occurrence(string location, int occurrence) => _rules.Count == 0 ? "" : $"{location}[{occurrence}]";
        public void Check(Node node, string raw, string location)
        {
            // Only consumed projected fields reach here. Empty wire positions
            // become Null under the existing grammar, not a zero-length lexeme.
            if (raw.Length == 0 || _truncated || !_rules.TryGetValue(node, out var rule)) return;
            int count = raw.EnumerateRunes().Count();
            if (count < rule.Minimum) Add(FerruleEdiConstraintViolation.TooShort);
            else if (count > rule.Maximum) Add(FerruleEdiConstraintViolation.TooLong);
            if (rule.Allowed.Count > 0 && !rule.Allowed.Contains(raw)) Add(FerruleEdiConstraintViolation.NotAllowed);

            void Add(FerruleEdiConstraintViolation violation)
            {
                if (_issues.Count == MaximumIssues) { _truncated = true; return; }
                _issues.Add(new(rule.Field, location, violation,
                    violation == FerruleEdiConstraintViolation.TooShort ? rule.Minimum : null,
                    violation == FerruleEdiConstraintViolation.TooLong ? rule.Maximum : null,
                    violation == FerruleEdiConstraintViolation.NotAllowed ? null : count,
                    violation == FerruleEdiConstraintViolation.NotAllowed ? rule.Allowed.Count : null));
            }
        }
        public void ThrowIfAny()
        {
            if (_issues.Count == 0) return;
            var report = new FerruleEdiValidationReport(_issues, _truncated);
            throw new FerruleEdiException(FerruleEdiError.Constraint, report.ToString(), field: _issues[0].Field) { ValidationReport = report };
        }
    }
}
