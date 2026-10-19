using System.Collections.ObjectModel;

namespace Ferrule.Runtime;

public enum FerruleFilterMapPhase { Source, Capture, Predicate, Mapper }
public enum FerruleFilterMapBoundaryKind { NodeEvaluation, CallEntry, SourceReservation, StageItem, Result }
public enum FerruleFilterMapBudgetKind { SourceItems, Work }
public enum FerruleFilterMapFailureKind { ValueType, NonFinite, Budget, Cancelled }

/// <summary>The exact feature boundary; positions refer to the unfiltered source.</summary>
public readonly record struct FerruleFilterMapBoundary(
    uint Item,
    FerruleFilterMapPhase Phase,
    int? CaptureIndex = null,
    int? SourcePosition = null,
    ulong? Function = null,
    uint? Node = null,
    FerruleFilterMapBoundaryKind Kind = FerruleFilterMapBoundaryKind.Result);

/// <summary>Run-wide feature ceilings, independent of legacy generator policy.</summary>
public sealed class FerruleFilterMapLimits
{
    public const ulong MaximumSourceItems = 1_000_000;
    public const ulong MaximumWork = 10_000_000;
    public static FerruleFilterMapLimits Default { get; } = new(MaximumSourceItems, MaximumWork);

    public FerruleFilterMapLimits(UInt128 sourceItems, UInt128 work)
    {
        if (sourceItems > MaximumSourceItems) throw new ArgumentOutOfRangeException(nameof(sourceItems));
        if (work > MaximumWork) throw new ArgumentOutOfRangeException(nameof(work));
        SourceItems = sourceItems;
        Work = work;
    }

    public UInt128 SourceItems { get; }
    public UInt128 Work { get; }
}

/// <summary>Synchronous pre-work cancellation; a builtin in progress is not interrupted.</summary>
public interface IFerruleFilterMapCancellation
{
    bool IsCancelled(FerruleFilterMapBoundary boundary);
}

/// <summary>A feature cause with original scalar bits and exact reservation quantities.</summary>
public sealed class FerruleFilterMapFailure : Exception
{
    internal FerruleFilterMapFailure(
        FerruleFilterMapFailureKind kind,
        FerruleScalarType? expectedType = null,
        FerruleValue? found = null,
        ulong? floatBits = null,
        FerruleFilterMapBudgetKind? budgetKind = null,
        UInt128? used = null,
        UInt128? requested = null,
        UInt128? maximum = null)
        : base($"filter/map {kind}: expected={expectedType}; found={found?.Kind}; bits={floatBits:X16}; budget={budgetKind}; used={used}; requested={requested}; maximum={maximum}")
    {
        Kind = kind;
        ExpectedType = expectedType;
        Found = found;
        FloatBits = floatBits;
        BudgetKind = budgetKind;
        Used = used;
        Requested = requested;
        Maximum = maximum;
    }

    public FerruleFilterMapFailureKind Kind { get; }
    public FerruleScalarType? ExpectedType { get; }
    public FerruleValue? Found { get; }
    public ulong? FloatBits { get; }
    public FerruleFilterMapBudgetKind? BudgetKind { get; }
    public UInt128? Used { get; }
    public UInt128? Requested { get; }
    public UInt128? Maximum { get; }
}

/// <summary>One most-specific envelope, retaining its complete original cause.</summary>
public sealed class FerruleFilterMapException : Exception
{
    internal FerruleFilterMapException(FerruleFilterMapBoundary boundary, Exception cause)
        : base($"filter/map {boundary}: {cause.Message}", cause) => Boundary = boundary;

    public FerruleFilterMapBoundary Boundary { get; }
    public Exception Cause => InnerException!;

    internal static Exception Wrap(FerruleFilterMapBoundary boundary, Exception cause) =>
        cause is FerruleFilterMapException ? cause : new FerruleFilterMapException(boundary, cause);
}

/// <summary>A source expression and its original graph identity.</summary>
public sealed record FerruleFilterMapInput(
    uint Node,
    Func<ScopeContext, FerruleValue> Evaluate);

public sealed record FerruleFilterMapCapture(
    uint Node,
    FerruleScalarType Type,
    Func<ScopeContext, FerruleValue> Evaluate);

public sealed record FerruleFilterMapStage(
    ulong Function,
    uint OutputNode,
    Func<ScopeContext, FerruleValue[], FerruleValue> Invoke);

internal sealed class FerruleFilterMapRunState
{
    private readonly FerruleFilterMapLimits _limits;
    private readonly IFerruleFilterMapCancellation? _cancellation;
    private UInt128 _sourceItems;
    private UInt128 _work;

    internal FerruleFilterMapRunState(FerruleExecutionContext? context)
    {
        _limits = context?.FilterMapLimits ?? FerruleFilterMapLimits.Default;
        _cancellation = context?.FilterMapCancellation;
    }

    internal void Check(FerruleFilterMapBoundary boundary)
    {
        try
        {
            if (_cancellation?.IsCancelled(boundary) == true)
                throw new FerruleFilterMapFailure(FerruleFilterMapFailureKind.Cancelled);
        }
        catch (Exception error) { throw FerruleFilterMapException.Wrap(boundary, error); }
    }

    internal void Charge(FerruleFilterMapBoundary boundary)
    {
        Check(boundary);
        if (_work >= _limits.Work)
            throw FerruleFilterMapException.Wrap(boundary, new FerruleFilterMapFailure(
                FerruleFilterMapFailureKind.Budget, budgetKind: FerruleFilterMapBudgetKind.Work,
                used: _work, requested: 1, maximum: _limits.Work));
        _work++;
    }

    internal void Reserve(FerruleFilterMapBoundary boundary, UInt128 requested)
    {
        Check(boundary);
        if (requested > FerruleSequences.MaximumGeneratedSequenceItems)
            throw FerruleFilterMapException.Wrap(boundary, new FerruleRuntimeException(
                FerruleRuntimeError.GeneratedSequenceTooLarge,
                $"generate-sequence requested {requested} items; maximum is {FerruleSequences.MaximumGeneratedSequenceItems}",
                requestedItems: requested, maximumItems: FerruleSequences.MaximumGeneratedSequenceItems));
        if (requested > _limits.SourceItems - _sourceItems)
            throw FerruleFilterMapException.Wrap(boundary, new FerruleFilterMapFailure(
                FerruleFilterMapFailureKind.Budget, budgetKind: FerruleFilterMapBudgetKind.SourceItems,
                used: _sourceItems, requested: requested, maximum: _limits.SourceItems));
        if (requested > _limits.Work - _work)
            throw FerruleFilterMapException.Wrap(boundary, new FerruleFilterMapFailure(
                FerruleFilterMapFailureKind.Budget, budgetKind: FerruleFilterMapBudgetKind.Work,
                used: _work, requested: requested, maximum: _limits.Work));
        _sourceItems += requested;
        _work += requested;
    }
}

public sealed partial class ScopeContext
{
    private readonly FerruleFilterMapRunState _filterMapRunState;
    private readonly FerruleFilterMapBoundary? _filterMapBoundary;
    private readonly int _filterMapCallDepth;

    internal ScopeContext WithFilterMapBoundary(FerruleFilterMapBoundary boundary) =>
        new(_primarySource, _frames, _collections, _executionContext, _dynamicSourceLoader,
            _filterMapRunState, boundary, _filterMapCallDepth);

    /// <summary>Generated nodes charge only inside a reached feature subevaluation.</summary>
    public FerruleValue EvaluateFilterMapNode(uint node, ulong? function, Func<FerruleValue> evaluate)
    {
        ArgumentNullException.ThrowIfNull(evaluate);
        if (_filterMapBoundary is not { } active) return evaluate();
        var boundary = active with { Node = node, Function = function, Kind = FerruleFilterMapBoundaryKind.NodeEvaluation };
        _filterMapRunState.Charge(boundary);
        try { return evaluate(); }
        catch (Exception error) { throw FerruleFilterMapException.Wrap(boundary, error); }
    }

    /// <summary>Call entry follows complete argument evaluation and precedes adaptation.</summary>
    public ScopeContext EnterFilterMapCall(ulong function)
    {
        if (_filterMapBoundary is not { } active) return this;
        var boundary = active with { Function = function, Node = null, Kind = FerruleFilterMapBoundaryKind.CallEntry };
        _filterMapRunState.Charge(boundary);
        if (_filterMapCallDepth >= 64)
        {
            // Preserve the ordinary cause until the actual caller node/stage wraps it.
            throw new FerruleRuntimeException(
                FerruleRuntimeError.UserFunctionDepth, "user function exceeds the 64-call depth limit", maximumDepth: 64);
        }
        return new ScopeContext(_primarySource, _frames, _collections, _executionContext, _dynamicSourceLoader,
            _filterMapRunState, active, _filterMapCallDepth + 1);
    }

    internal void CheckFilterMap(FerruleFilterMapBoundary boundary) => _filterMapRunState.Check(boundary);
    internal void ReserveFilterMap(FerruleFilterMapBoundary boundary, UInt128 count) => _filterMapRunState.Reserve(boundary, count);
}

/// <summary>Eager scalar composition; a failed call never returns a partial sequence.</summary>
public static class FerruleFilterMap
{
    public static IReadOnlyList<FerruleValue> Evaluate(
        ScopeContext context,
        uint item,
        FerruleFilterMapInput? from,
        FerruleFilterMapInput to,
        IReadOnlyList<FerruleFilterMapCapture> captures,
        FerruleFilterMapStage predicate,
        FerruleFilterMapStage mapper,
        FerruleScalarType outputType)
    {
        ArgumentNullException.ThrowIfNull(context);
        ArgumentNullException.ThrowIfNull(to);
        ArgumentNullException.ThrowIfNull(captures);
        ArgumentNullException.ThrowIfNull(predicate);
        ArgumentNullException.ThrowIfNull(mapper);
        if (captures.Count > 16) throw new ArgumentOutOfRangeException(nameof(captures));
        var sourceBoundary = new FerruleFilterMapBoundary(item, FerruleFilterMapPhase.Source);
        var sourceContext = context.WithFilterMapBoundary(sourceBoundary);
        var source = SourceValues(sourceContext, sourceBoundary, from, to);
        var arguments = new FerruleValue[captures.Count + 2];
        for (var index = 0; index < captures.Count; index++)
        {
            var capture = captures[index];
            var boundary = new FerruleFilterMapBoundary(item, FerruleFilterMapPhase.Capture,
                CaptureIndex: index, Node: capture.Node);
            try
            {
                var value = capture.Evaluate(context.WithFilterMapBoundary(boundary));
                RequireValue(boundary, value, capture.Type);
                arguments[index + 2] = value;
            }
            catch (Exception error) { throw FerruleFilterMapException.Wrap(boundary, error); }
        }
        var output = new List<FerruleValue>();
        for (var index = 0; index < source.Count; index++)
        {
            var boundary = new FerruleFilterMapBoundary(item, FerruleFilterMapPhase.Predicate,
                SourcePosition: index + 1, Kind: FerruleFilterMapBoundaryKind.StageItem);
            context.CheckFilterMap(boundary);
            arguments[0] = source[index];
            arguments[1] = FerruleValue.FromInt64(index + 1);
            var keep = Stage(context, boundary, predicate, arguments, FerruleScalarType.Bool);
            if (!keep.BooleanValue) continue;
            boundary = boundary with { Phase = FerruleFilterMapPhase.Mapper };
            output.Add(Stage(context, boundary, mapper, arguments, outputType));
        }
        return new ReadOnlyCollection<FerruleValue>(output);
    }

    private static IReadOnlyList<FerruleValue> SourceValues(
        ScopeContext context,
        FerruleFilterMapBoundary boundary,
        FerruleFilterMapInput? from,
        FerruleFilterMapInput to)
    {
        var inputBoundary = boundary;
        try
        {
            inputBoundary = boundary with { Node = from?.Node };
            var lower = from?.Evaluate(context.WithFilterMapBoundary(inputBoundary));
            if (lower.HasValue && Absent(lower.Value))
            {
                context.ReserveFilterMap(boundary with { Kind = FerruleFilterMapBoundaryKind.SourceReservation }, 0);
                return Array.Empty<FerruleValue>();
            }
            inputBoundary = boundary with { Node = to.Node };
            var upper = to.Evaluate(context.WithFilterMapBoundary(inputBoundary));
            if (Absent(upper))
            {
                context.ReserveFilterMap(boundary with { Kind = FerruleFilterMapBoundaryKind.SourceReservation }, 0);
                return Array.Empty<FerruleValue>();
            }
            // Both expressions completed. Their coercions retain lower/upper order.
            inputBoundary = boundary with { Node = from?.Node };
            var first = lower.HasValue ? FerruleSequences.SequenceInteger(lower.Value) : 1L;
            inputBoundary = boundary with { Node = to.Node };
            var last = FerruleSequences.SequenceInteger(upper);
            inputBoundary = boundary;
            var count = first > last ? (UInt128)0 : (UInt128)((Int128)last - first + 1);
            context.ReserveFilterMap(boundary with { Kind = FerruleFilterMapBoundaryKind.SourceReservation }, count);
            var values = new List<FerruleValue>((int)count);
            for (UInt128 offset = 0; offset < count; offset++)
                values.Add(FerruleValue.FromInt64((long)((Int128)first + (Int128)offset)));
            return new ReadOnlyCollection<FerruleValue>(values);
        }
        catch (Exception error) { throw FerruleFilterMapException.Wrap(inputBoundary, error); }
    }

    private static FerruleValue Stage(
        ScopeContext context,
        FerruleFilterMapBoundary boundary,
        FerruleFilterMapStage stage,
        FerruleValue[] arguments,
        FerruleScalarType expected)
    {
        boundary = boundary with { Function = stage.Function, Node = stage.OutputNode, Kind = FerruleFilterMapBoundaryKind.Result };
        try
        {
            // An independent argument array prevents stage delegates from mutating
            // the captures or the next stage's supplied argument vector.
            var value = stage.Invoke(context.WithFilterMapBoundary(boundary).EnterFilterMapCall(stage.Function), (FerruleValue[])arguments.Clone());
            RequireValue(boundary, value, expected);
            return value;
        }
        catch (Exception error) { throw FerruleFilterMapException.Wrap(boundary, error); }
    }

    private static bool Absent(FerruleValue value) =>
        value.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull;

    private static void RequireValue(FerruleFilterMapBoundary boundary, FerruleValue value, FerruleScalarType expected)
    {
        var same = (expected, value.Kind) switch
        {
            (FerruleScalarType.Int64, FerruleValueKind.Int64) or
            (FerruleScalarType.Double, FerruleValueKind.Double) or
            (FerruleScalarType.Bool, FerruleValueKind.Bool) or
            (FerruleScalarType.String, FerruleValueKind.String) => true,
            _ => false,
        };
        if (!same) throw FerruleFilterMapException.Wrap(boundary, new FerruleFilterMapFailure(
            FerruleFilterMapFailureKind.ValueType, expectedType: expected, found: value));
        if (value.Kind == FerruleValueKind.Double && !double.IsFinite(value.DoubleValue))
            throw FerruleFilterMapException.Wrap(boundary, new FerruleFilterMapFailure(
                FerruleFilterMapFailureKind.NonFinite, floatBits: unchecked((ulong)BitConverter.DoubleToInt64Bits(value.DoubleValue))));
    }
}
