using System.Text;

namespace Ferrule.Runtime;

// These identities match mapping::SourceContractReason, not display messages.
public enum FerruleSourceContractReason
{
    UnsupportedModel, UnsupportedSchema, UnprovedOwner, UnexpectedBinding,
    UnexpectedParameter, UnexpectedField, DuplicateField, MissingRepeatedSlot,
    WrongShape, WrongScalarDomain, ResourceLimit,
}

public sealed class FerruleSourceContractViolation
{
    internal FerruleSourceContractViolation(string[] path, FerruleSourceContractReason reason)
    { Path = Array.AsReadOnly(path); Reason = reason; }
    public IReadOnlyList<string> Path { get; }
    public FerruleSourceContractReason Reason { get; }
}

/// <summary>Immutable literals emitted from a validated current Program.</summary>
public sealed class FerrulePrimarySourceShape
{
    private FerrulePrimarySourceShape(string name, bool repeating, FerruleScalarType? scalar,
        bool nillable, int[] children)
    { Name = name; Repeating = repeating; ScalarType = scalar; Nillable = nillable; Children = children; }
    internal string Name { get; }
    internal bool Repeating { get; }
    internal FerruleScalarType? ScalarType { get; }
    internal bool Nillable { get; }
    internal int[] Children { get; }
    public static FerrulePrimarySourceShape Group(string name, bool repeating, params int[] children)
    {
        ArgumentNullException.ThrowIfNull(name);
        ArgumentNullException.ThrowIfNull(children);
        if (children.Length > FerrulePrimarySourceContract.MaximumShapeNodes)
            throw FerrulePrimarySourceContract.Failure(FerruleSourceContractReason.ResourceLimit);
        return new(name, repeating, null, false, (int[])children.Clone());
    }
    public static FerrulePrimarySourceShape Scalar(string name, FerruleScalarType type, bool nillable)
    {
        ArgumentNullException.ThrowIfNull(name);
        return new(name, false, type, nillable, Array.Empty<int>());
    }
}

/// <summary>Closed primary-tree admission, never an alternate source resolver.</summary>
public sealed class FerrulePrimarySourceContract
{
    public const int MaximumDepth = 256;
    public const int MaximumShapeNodes = 65_536;
    public const int MaximumMetadataBytes = 32 * 1024 * 1024;
    public const int MaximumPathBytes = 4096;
    public const int MaximumVisits = 1_000_000;
    private readonly FerrulePrimarySourceShape[] _nodes;
    private readonly int[] _parents;

    // Generated execute overloads never accept a caller-supplied descriptor.
    public FerrulePrimarySourceContract(IReadOnlyList<FerrulePrimarySourceShape> nodes)
    {
        ArgumentNullException.ThrowIfNull(nodes);
        var count = nodes.Count;
        if (count <= 0 || count > MaximumShapeNodes)
            throw Failure(FerruleSourceContractReason.ResourceLimit);
        _nodes = new FerrulePrimarySourceShape[count];
        _parents = new int[count];
        Array.Fill(_parents, -1);
        var depths = new int[count];
        var pathBytes = new int[count];
        long metadata = 0;
        for (var index = 0; index < count; index++)
        {
            var node = nodes[index];
            if (node is null || node.Name.Length == 0)
                throw Failure(FerruleSourceContractReason.UnsupportedSchema);
            if (node.Name.Length > MaximumPathBytes)
                throw Failure(FerruleSourceContractReason.ResourceLimit);
            if (!FerruleUnicode.IsWellFormed(node.Name))
                throw Failure(FerruleSourceContractReason.UnsupportedSchema);
            var nameBytes = Encoding.UTF8.GetByteCount(node.Name);
            if (nameBytes > MaximumPathBytes)
                throw Failure(FerruleSourceContractReason.ResourceLimit);
            if (!FerrulePrimaryRoot.NcNameIsValid(node.Name))
                throw Failure(FerruleSourceContractReason.UnsupportedSchema);
            if (node.ScalarType is { } scalarType && !Enum.IsDefined(scalarType))
                throw Failure(FerruleSourceContractReason.UnsupportedSchema);
            metadata += nameBytes + 80L;
            if (metadata > MaximumMetadataBytes)
                throw Failure(FerruleSourceContractReason.ResourceLimit);
            _nodes[index] = node;
            pathBytes[index] = nameBytes;
        }
        if (_nodes[0].Repeating || _nodes[0].ScalarType is not null)
            throw Failure(FerruleSourceContractReason.UnsupportedSchema);
        for (var index = 0; index < _nodes.Length; index++)
        {
            var node = _nodes[index];
            if (index != 0 && _parents[index] < 0)
                throw Failure(FerruleSourceContractReason.UnsupportedSchema);
            if (depths[index] > MaximumDepth || pathBytes[index] > MaximumPathBytes)
                throw Failure(FerruleSourceContractReason.ResourceLimit);
            foreach (var child in node.Children)
            {
                if (child <= index || child >= _nodes.Length || _parents[child] != -1)
                    throw Failure(FerruleSourceContractReason.UnsupportedSchema);
                _parents[child] = index;
                depths[child] = depths[index] + 1;
                pathBytes[child] += pathBytes[index];
            }
            for (var slot = 1; slot < node.Children.Length; slot++)
                if (CompareUtf8(_nodes[node.Children[slot - 1]].Name,
                    _nodes[node.Children[slot]].Name) >= 0)
                    throw Failure(FerruleSourceContractReason.UnsupportedSchema);
        }
    }

    public static void ValidateBindings(int extras, int parameters)
    {
        if (extras != 0) throw Failure(FerruleSourceContractReason.UnexpectedBinding);
        if (parameters != 0) throw Failure(FerruleSourceContractReason.UnexpectedParameter);
    }

    public void Validate(FerruleInstance source, int extras, int parameters)
    {
        ValidateBindings(extras, parameters);
        ArgumentNullException.ThrowIfNull(source);
        var pending = new Stack<Work>();
        pending.Push(new(WorkKind.Check, 0, source));
        var visits = 0;
        while (pending.TryPop(out var work))
        {
            var node = _nodes[work.Index];
            switch (work.Kind)
            {
                case WorkKind.Check:
                    if (++visits > MaximumVisits) throw At(work.Index, FerruleSourceContractReason.ResourceLimit);
                    if (node.Repeating && !work.Member)
                    {
                        if (work.Value is not FerruleRepeated repeated)
                            throw At(work.Index, FerruleSourceContractReason.WrongShape);
                        if (repeated.Items.Count > MaximumVisits - visits)
                            throw At(work.Index, FerruleSourceContractReason.ResourceLimit);
                        pending.Push(new(WorkKind.Items, work.Index, repeated));
                    }
                    else if (node.ScalarType is { } scalarType)
                    {
                        if (work.Value is not FerruleScalar scalar)
                            throw At(work.Index, FerruleSourceContractReason.WrongShape);
                        var valid = scalar.Value.Kind switch
                        {
                            FerruleValueKind.Null => true,
                            FerruleValueKind.XmlNil => node.Nillable,
                            FerruleValueKind.String => scalarType == FerruleScalarType.String,
                            FerruleValueKind.Int64 => scalarType == FerruleScalarType.Int64,
                            FerruleValueKind.Bool => scalarType == FerruleScalarType.Bool,
                            FerruleValueKind.Double => scalarType == FerruleScalarType.Double && double.IsFinite(scalar.Value.DoubleValue),
                            _ => false,
                        };
                        if (!valid) throw At(work.Index, FerruleSourceContractReason.WrongScalarDomain);
                    }
                    else
                    {
                        if (work.Value is not FerruleGroup group)
                            throw At(work.Index, FerruleSourceContractReason.WrongShape);
                        if (group.Fields.Count > MaximumVisits - visits)
                            throw At(work.Index, FerruleSourceContractReason.ResourceLimit);
                        // One unit per declared slot funds both seen-ledger
                        // initialization and the final required-repetition scan.
                        if (node.Children.Length > MaximumVisits - visits)
                            throw At(work.Index, FerruleSourceContractReason.ResourceLimit);
                        visits += node.Children.Length;
                        pending.Push(new(WorkKind.Fields, work.Index, group, seen: new bool[node.Children.Length]));
                    }
                    break;
                case WorkKind.Items:
                    var items = ((FerruleRepeated)work.Value).Items;
                    if (work.Offset < items.Count)
                    {
                        pending.Push(work.Next());
                        pending.Push(new(WorkKind.Check, work.Index, items[work.Offset], member: true));
                    }
                    break;
                case WorkKind.Fields:
                    var fields = ((FerruleGroup)work.Value).Fields;
                    var seen = work.Seen!;
                    if (work.Offset < fields.Count)
                    {
                        var field = fields[work.Offset];
                        var slot = ChildSlot(node.Children, field.Name);
                        if (slot < 0) throw At(work.Index, FerruleSourceContractReason.UnexpectedField);
                        var child = node.Children[slot];
                        if (seen[slot]) throw At(child, FerruleSourceContractReason.DuplicateField);
                        seen[slot] = true;
                        pending.Push(work.Next());
                        pending.Push(new(WorkKind.Check, child, field.Value));
                    }
                    else
                    {
                        for (var slot = 0; slot < node.Children.Length; slot++)
                            if (_nodes[node.Children[slot]].Repeating && !seen[slot])
                                throw At(node.Children[slot], FerruleSourceContractReason.MissingRepeatedSlot);
                    }
                    break;
            }
        }
    }

    private int ChildSlot(int[] children, string name)
    {
        var low = 0;
        var high = children.Length - 1;
        while (low <= high)
        {
            var middle = low + (high - low) / 2;
            var comparison = CompareUtf8(_nodes[children[middle]].Name, name);
            if (comparison == 0) return middle;
            if (comparison < 0) low = middle + 1;
            else high = middle - 1;
        }
        return -1;
    }

    private FerruleRuntimeException At(int index, FerruleSourceContractReason reason)
    {
        var path = new List<string>();
        while (_parents[index] >= 0)
        { path.Add(_nodes[index].Name); index = _parents[index]; }
        path.Reverse();
        return Failure(reason, path.ToArray());
    }

    // The emitter supplies Rust UTF-8 lexical order. UTF-16 ordinal order is
    // different for supplementary NCName characters. Unicode scalar order is
    // exactly UTF-8 lexical order and requires no temporary encoded strings.
    private static int CompareUtf8(string left, string right)
    {
        var a = left.EnumerateRunes();
        var b = right.EnumerateRunes();
        while (a.MoveNext())
        {
            if (!b.MoveNext()) return 1;
            var comparison = a.Current.Value.CompareTo(b.Current.Value);
            if (comparison != 0) return comparison;
        }
        return b.MoveNext() ? -1 : 0;
    }

    internal static FerruleRuntimeException Failure(FerruleSourceContractReason reason, string[]? path = null) =>
        new(FerruleRuntimeError.SourceContractViolation, $"primary source contract: {reason}",
            sourceContractViolation: new(path ?? Array.Empty<string>(), reason));

    private enum WorkKind { Check, Items, Fields }
    private readonly struct Work
    {
        internal Work(WorkKind kind, int index, FerruleInstance value, int offset = 0, bool member = false, bool[]? seen = null)
        { Kind = kind; Index = index; Value = value; Offset = offset; Member = member; Seen = seen; }
        internal WorkKind Kind { get; }
        internal int Index { get; }
        internal FerruleInstance Value { get; }
        internal int Offset { get; }
        internal bool Member { get; }
        internal bool[]? Seen { get; }
        internal Work Next() => new(Kind, Index, Value, Offset + 1, Member, Seen);
    }
}
