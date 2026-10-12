using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static FerruleOwnedSourceLimits AdmissionLimits => new(4, 4, 8, 4096, 8);
    private static FerruleBasicNamedSourceDeclaration[] AdmissionCatalogue =>
        [new("Ref", FerruleBasicNamedSourceMode.Optional)];
    private static FerruleGroup AdmissionPrimary() => Group(Field("Value", Scalar(Text("P.root"))));
    private static FerruleGroup AdmissionNamed() => Group(Field("Value", Scalar(Text("R.root"))));
    private sealed class AdmissionProbe : IReadOnlyList<FerruleNamedSourceInput>
    {
        internal AdmissionProbe(params FerruleNamedSourceInput[] records) { Records = records; ExposedCount = records.Length; }
        internal FerruleNamedSourceInput[] Records { get; }
        internal int ExposedCount { get; set; }
        internal int CountReads { get; private set; }
        internal List<int> IndexReads { get; } = [];
        internal int AccessorCalls { get; set; }
        internal int EnumeratorCalls { get; private set; }
        internal bool ChangeCountAtZero { get; set; }
        internal FerruleNamedSourceInput? ChangedZero { get; set; }
        internal int ThrowIndex { get; set; } = -1;
        internal Exception? Sentinel { get; set; }
        public int Count { get { CountReads++; return ExposedCount; } }
        public FerruleNamedSourceInput this[int index]
        {
            get
            {
                IndexReads.Add(index);
                if (index == ThrowIndex) throw Sentinel ?? new InvalidOperationException("missing sentinel");
                if (index == 0 && ChangeCountAtZero) ExposedCount = 0;
                if (index == 0 && ChangedZero.HasValue && IndexReads.Count(i => i == 0) > 1) return ChangedZero.Value;
                return Records[index];
            }
        }
        public IEnumerator<FerruleNamedSourceInput> GetEnumerator()
        { EnumeratorCalls++; throw new InvalidOperationException("the supplied list must not be enumerated"); }
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => GetEnumerator();
    }
    private readonly record struct AdmissionAccessor(AdmissionProbe Probe) : IFerruleNamedSourceInputAccessor<FerruleNamedSourceInput>
    {
        public FerruleNamedSourceInput Read(FerruleNamedSourceInput item) { Probe.AccessorCalls++; return item; }
    }
    private sealed class AdmissionResult<T>(T value, Exception? error)
    { internal T Value { get; } = value; internal Exception? Error { get; } = error; }
    private static object AdmissionInputSnapshot(FerruleInstance? primary, FerruleBasicNamedSourceDeclaration[] catalogue,
        AdmissionProbe? supplied, FerruleOwnedSourceLimits limits) => new
    {
        primary = BasicInstanceSnapshot(primary),
        catalogue = catalogue.Select((d, i) => new { ordinal = i, nameCodeUnits = d.Name?.Select(c => (int)c).ToArray(), mode = (int)d.Mode }).ToArray(),
        records = supplied?.Records.Select(r => new { nameCodeUnits = r.Name?.Select(c => (int)c).ToArray(), instance = BasicInstanceSnapshot(r.Instance) }).ToArray(),
        probeRecipe = supplied is null ? null : new { supplied.ExposedCount, supplied.ChangeCountAtZero, supplied.ThrowIndex,
            changedZero = supplied.ChangedZero.HasValue ? new { nameCodeUnits = supplied.ChangedZero.Value.Name?.Select(c => (int)c).ToArray(), instance = BasicInstanceSnapshot(supplied.ChangedZero.Value.Instance) } : null,
            sentinel = supplied.Sentinel is null ? null : AdmissionSnapshot(supplied.Sentinel) },
        roots = (ulong)limits.MaxRoots, frames = (ulong)limits.MaxActiveFrames, segments = (ulong)limits.MaxPathSegments,
        bytes = (ulong)limits.MaxMetadataBytes, contexts = (ulong)limits.MaxWalkContexts,
    };
    private static object? AdmissionSnapshot(object? value) => value switch
    {
        FerruleBasicSourceRoots roots => new
        {
            native = BasicReachedSnapshot(roots), storage = (ulong)roots.OwnedRoots.RegistryStorageCount,
            ambiguityReservation = (ulong)roots.OwnedRoots.AmbiguityReservationCount, text = (ulong)roots.OwnedRoots.RegistryText,
        },
        FerruleOwnedScopeContext context => new
        {
            storage = (ulong)context.Roots.RegistryStorageCount,
            frames = Enumerable.Range(0, context.FrameCount).Select(i =>
            { var f = context.FrameAt(i); return new { f.Owner, path = f.Path.ToArray(), item = BasicInstanceSnapshot(f.Item), index = (ulong)f.Index }; }).ToArray(),
        },
        OwnedQuery query => new { slots = (ulong)query.Slots.Used, bytes = (ulong)query.Bytes.Used,
            registry = (ulong)query.Roots.RegistryStorageCount, ambiguity = (ulong)query.Roots.AmbiguityReservationCount },
        nuint[] indices => indices.Select(i => (ulong)i).ToArray(),
        FerruleValue[] values => values.Select(BasicValueSnapshot).ToArray(),
        Exception error => new { native = BasicErrorSnapshot(error), error.StackTrace, error.Source,
            actualValue = (error as ArgumentOutOfRangeException)?.ActualValue },
        _ => BasicReachedSnapshot(value),
    };
    private static AdmissionResult<T> AdmissionReached<T>(string id, object? inputs, AdmissionProbe? probe, Func<T> action)
    {
        T value = default!; Exception? error = null;
        try { value = action(); } catch (Exception e) { error = e; }
        var record = new
        {
            caseId = id, inputs, reachedValue = error is null ? AdmissionSnapshot(value) : null,
            reachedException = error is null ? null : AdmissionSnapshot(error),
            supplied = probe is null ? null : new { probe.CountReads, indexReads = probe.IndexReads.ToArray(), probe.AccessorCalls, probe.EnumeratorCalls, probe.ExposedCount },
        };
        Console.WriteLine("N527_RAW " + System.Text.Json.JsonSerializer.Serialize(record));
        return new(value, error);
    }
    private static AdmissionResult<FerruleBasicSourceRoots> AdmissionNormalize(string id, FerruleInstance? primary,
        FerruleBasicNamedSourceDeclaration[] catalogue, AdmissionProbe? probe, FerruleOwnedSourceLimits? limits = null)
    {
        var policy = limits ?? AdmissionLimits; var inputs = AdmissionInputSnapshot(primary, catalogue, probe, policy);
        return AdmissionReached(id, inputs, probe, () => FerruleBasicSourceRoots.CreateFromNamedInputs(
            primary!, catalogue, probe!, new AdmissionAccessor(probe!), policy));
    }
    private static T AdmissionSuccess<T>(AdmissionResult<T> result)
    { AdmissionCheck(result.Error is null, "expected success after complete capture"); return result.Value; }
    private static void AdmissionCheck([System.Diagnostics.CodeAnalysis.DoesNotReturnIf(false)] bool condition, string message)
    { if (!condition) throw new InvalidOperationException("named input admission: " + message); }
    private static void AdmissionReads(AdmissionProbe probe, int count, params int[] indices)
    {
        AdmissionCheck(probe.CountReads == count && probe.IndexReads.SequenceEqual(indices), "exact Count/indexer consumption");
        AdmissionCheck(probe.AccessorCalls == indices.Length && probe.EnumeratorCalls == 0, "exact value accessor/no enumeration");
    }
    private static void AdmissionOrdinaryError<T>(AdmissionResult<T> result, FerruleRuntimeError kind, string name)
    {
        var e = result.Error as FerruleRuntimeException;
        AdmissionCheck(e is not null && e.Error == kind && e.Detail == name && e.InnerException is null, "complete ordinary named-input error");
        AdmissionCheck(e!.Node is null && e.Function is null && e.ExpectedArity is null && e.ActualArity is null && e.FoundKind is null && e.AggregateOperation is null && e.RequestedItems is null && e.MaximumItems is null && e.MaximumDepth is null && e.RuntimeValue is null && e.FailureRule is null && e.MappingFailureMessage is null && e.MappingExceptionMessage is null && e.Join is null && e.UserFunction is null && e.FunctionParameter is null && e.ExpectedScalarType is null && e.RuntimeParameter is null && e.SourceField is null && e.FoundInstance is null && e.PrimaryRoot is null && e.OwnedSourceFailure is null && e.BasicSourceBridgeFailure is null, "all other ordinary error fields null");
        var message = kind switch { FerruleRuntimeError.UnexpectedNamedSource => "named source is not declared by this mapping", FerruleRuntimeError.DuplicateNamedSource => "named source was supplied more than once", FerruleRuntimeError.MissingNamedSource => "named source is required by this mapping", _ => throw new InvalidOperationException("frozen category") };
        AdmissionCheck(e.Message == message, "exact constant message");
    }
    private static void AdmissionOwnedError<T>(AdmissionResult<T> result, FerruleOwnedSourceErrorKind kind,
        FerruleOwnedSourceLimitKind? limit = null, nuint? used = null, nuint? requested = null, nuint? maximum = null, string? unavailableName = null)
    {
        var outer = result.Error as FerruleRuntimeException; var cause = outer?.OwnedSourceFailure;
        AdmissionCheck(outer is not null && outer.Error == FerruleRuntimeError.OwnedSource && cause is not null && ReferenceEquals(cause, outer.InnerException), "bare owned cause identity");
        AdmissionCheck(cause!.Error == kind && cause.LimitKind == limit && cause.Used == used && cause.Requested == requested && cause.Maximum == maximum, "complete owned resource payload");
        AdmissionCheck(cause.RequestedName == unavailableName && (unavailableName is null ? cause.Owner is null : cause.Owner is not null && !cause.Owner.IsPrimary && cause.Owner.Name == unavailableName), "qualified native owner/name aliases");
        AdmissionCheck(cause.DeclarationIndices is null && cause.Path is null && cause.Frame is null && cause.Collection is null && cause.Segment is null && cause.FoundInstance is null && cause.RequestedIndex is null && cause.InnerException is null, "remaining owned cause fields null");
    }
    private static FerruleBasicScopeContext AdmissionRoot(string id, FerruleBasicSourceRoots roots) =>
        AdmissionSuccess(AdmissionReached(id + ".root", AdmissionSnapshot(roots), null, () => roots.RootContext()));
    private static FerruleValue AdmissionField(string id, FerruleBasicScopeContext context, FerruleOwnedSourceOwner owner, params string[] path) =>
        AdmissionSuccess(AdmissionReached(id + ".field", new { context = BasicContextSnapshot(context), owner = owner.Name, path }, null, () => context.ResolveOwnedField(owner, path)));

    private static void NamedAdmissionPrimaryNamesAndOrdinals()
    {
        var empty = new AdmissionProbe(); var scalar = Scalar(Text("P.root"));
        var r1 = AdmissionNormalize("N527-01", scalar, [], empty, new FerruleOwnedSourceLimits(1, 0, 0, 0, 0));
        var roots1 = AdmissionSuccess(r1); var c1 = AdmissionRoot("N527-01", roots1); var value1 = AdmissionField("N527-01", c1, FerruleOwnedSourceOwner.Primary);
        Equal(Text("P.root"), value1); AdmissionCheck(roots1.OwnedRoots.RegistryStorageCount == 1, "empty catalogue adds no backing cells");
        var names = new[] { " Ref 雪 ", "ref", "primary" }; var cat = names.Select(n => new FerruleBasicNamedSourceDeclaration(n, FerruleBasicNamedSourceMode.Optional)).ToArray();
        var p2 = new AdmissionProbe(new FerruleNamedSourceInput("primary", Group(Field("Value", Scalar(Text("N.primary"))))), new FerruleNamedSourceInput("ref", Group(Field("Value", Scalar(Text("N.ref"))))), new FerruleNamedSourceInput(" Ref 雪 ", Group(Field("Value", Scalar(Text("N.unicode"))))));
        var roots2 = AdmissionSuccess(AdmissionNormalize("N527-02", AdmissionPrimary(), cat, p2)); var c2 = AdmissionRoot("N527-02", roots2);
        var values2 = names.Select(n => AdmissionField("N527-02." + n, c2, FerruleOwnedSourceOwner.Named(n), "Value")).ToArray();
        AdmissionCheck(values2.SequenceEqual(new[] { Text("N.unicode"), Text("N.ref"), Text("N.primary") }), "exact Unicode/case/space identities");
        AdmissionCheck(Enumerable.Range(1, 3).Select(roots2.OwnedRoots.Name).SequenceEqual(names) && Enumerable.Range(0, roots2.UniqueCount).Select(roots2.UniqueIndex).SequenceEqual(new[] { 0, 1, 2 }), "catalogue ordinal order independent of supplied order");
        var p3 = new AdmissionProbe(new FerruleNamedSourceInput("Other", Scalar(Text("O"))), new FerruleNamedSourceInput("Ref", Scalar(Text("R"))));
        var roots3 = AdmissionSuccess(AdmissionNormalize("N527-03", AdmissionPrimary(), [new("Ref", FerruleBasicNamedSourceMode.Optional), new("Other", FerruleBasicNamedSourceMode.Optional)], p3)); var c3 = AdmissionRoot("N527-03", roots3);
        var v3a = AdmissionField("N527-03.Ref", c3, FerruleOwnedSourceOwner.Named("Ref")); var v3b = AdmissionField("N527-03.Other", c3, FerruleOwnedSourceOwner.Named("Other"));
        Equal(Text("R"), v3a); Equal(Text("O"), v3b); AdmissionCheck(roots3.UniqueIndex(0) == 0 && roots3.UniqueIndex(1) == 1, "original ordinals");
        var p4 = new AdmissionProbe(); var roots4 = AdmissionSuccess(AdmissionNormalize("N527-04", AdmissionPrimary(), AdmissionCatalogue, p4)); var c4 = AdmissionRoot("N527-04", roots4); var v4 = AdmissionField("N527-04", c4, FerruleOwnedSourceOwner.Primary, "Value");
        Equal(Text("P.root"), v4); AdmissionCheck(roots4.OwnedRoots.Instance(1) is null, "optional absence is inert");
        var p7 = new AdmissionProbe(new FerruleNamedSourceInput("Ref", AdmissionNamed())); var roots7 = AdmissionSuccess(AdmissionNormalize("N527-07", AdmissionPrimary(), [new("Ref", FerruleBasicNamedSourceMode.RequiredLegacy), new("Unused", FerruleBasicNamedSourceMode.Optional)], p7)); var c7 = AdmissionRoot("N527-07", roots7);
        var legacyPath7 = new[] { "Ref", "Value" };
        var legacy7 = AdmissionSuccess(AdmissionReached("N527-07.legacy", new { context = BasicContextSnapshot(c7), path = legacyPath7 }, null, () => c7.LegacyView.ResolveSourceField(legacyPath7)));
        Equal(Text("R.root"), legacy7); AdmissionCheck(roots7.OwnedRoots.Instance(2) is null && roots7.OwnedRoots.Name(2) == "Unused", "absent original slot retained");
        var p13 = new AdmissionProbe(new FerruleNamedSourceInput("Ref", AdmissionNamed())); var roots13 = AdmissionSuccess(AdmissionNormalize("N527-13", AdmissionPrimary(), [new("Dyn", FerruleBasicNamedSourceMode.ReservedUnavailable), new("Ref", FerruleBasicNamedSourceMode.Optional)], p13, new FerruleOwnedSourceLimits(3, 4, 8, 4096, 8))); var c13 = AdmissionRoot("N527-13", roots13); var v13 = AdmissionField("N527-13", c13, FerruleOwnedSourceOwner.Named("Ref"), "Value");
        Equal(Text("R.root"), v13); AdmissionCheck(roots13.OwnedRoots.Instance(1) is null && roots13.OwnedRoots.Name(1) == "Dyn" && roots13.UniqueIndex(0) == 1, "reserved ordinal never compacted");
        var p15 = new AdmissionProbe(new FerruleNamedSourceInput("primary", Scalar(Text("N.primary")))); var roots15 = AdmissionSuccess(AdmissionNormalize("N527-15", AdmissionPrimary(), [new("primary", FerruleBasicNamedSourceMode.Optional)], p15)); var c15 = AdmissionRoot("N527-15", roots15); var v15a = AdmissionField("N527-15.primary", c15, FerruleOwnedSourceOwner.Primary, "Value"); var v15b = AdmissionField("N527-15.named", c15, FerruleOwnedSourceOwner.Named("primary"));
        Equal(Text("P.root"), v15a); Equal(Text("N.primary"), v15b); AdmissionCheck(roots15.OwnedRoots.Name(0) is null && roots15.OwnedRoots.Name(1) == "primary", "Primary is not a named spelling");
    }
    private static void NamedAdmissionOptionalAndRequiredFailures()
    {
        var roots = AdmissionSuccess(AdmissionNormalize("N527-05", AdmissionPrimary(), AdmissionCatalogue, new AdmissionProbe())); var context = AdmissionRoot("N527-05", roots);
        var fieldOwner = FerruleOwnedSourceOwner.Named("Ref"); var fieldPath = Array.Empty<string>(); IReadOnlyList<string>? fieldFrame = null;
        var missing = AdmissionReached("N527-05.field", new { context = BasicContextSnapshot(context), owner = new { fieldOwner.IsPrimary, fieldOwner.Name }, path = fieldPath, frame = fieldFrame }, null, () => context.ResolveOwnedField(fieldOwner, fieldPath, fieldFrame));
        AdmissionOwnedError(missing, FerruleOwnedSourceErrorKind.SourceUnavailable, unavailableName: "Ref");
        var required = AdmissionNormalize("N527-06", AdmissionPrimary(), [new("Ref", FerruleBasicNamedSourceMode.RequiredLegacy)], new AdmissionProbe());
        AdmissionOrdinaryError(required, FerruleRuntimeError.MissingNamedSource, "Ref");
    }
    private static void NamedAdmissionSuppliedOrderFailures()
    {
        var shared = AdmissionNamed();
        foreach (var row in new[]
        {
            (Id: "N527-08", Values: new[] { new FerruleNamedSourceInput("Ref", Scalar(Text("R"))), new FerruleNamedSourceInput("X", Scalar(Text("X"))) }, Kind: FerruleRuntimeError.UnexpectedNamedSource, Name: "X", Reached: 2),
            (Id: "N527-09", Values: new[] { new FerruleNamedSourceInput("Ref", shared), new FerruleNamedSourceInput("Ref", shared) }, Kind: FerruleRuntimeError.DuplicateNamedSource, Name: "Ref", Reached: 2),
            (Id: "N527-10", Values: new[] { new FerruleNamedSourceInput("Ref", Scalar(Text("R1"))), new FerruleNamedSourceInput("Ref", Scalar(Text("R2"))) }, Kind: FerruleRuntimeError.DuplicateNamedSource, Name: "Ref", Reached: 2),
            (Id: "N527-11", Values: new[] { new FerruleNamedSourceInput("X", Scalar(Text("X"))), new FerruleNamedSourceInput("Ref", Scalar(Text("R1"))), new FerruleNamedSourceInput("Ref", Scalar(Text("R2"))) }, Kind: FerruleRuntimeError.UnexpectedNamedSource, Name: "X", Reached: 1),
            (Id: "N527-12", Values: new[] { new FerruleNamedSourceInput("Ref", Scalar(Text("R1"))), new FerruleNamedSourceInput("Ref", Scalar(Text("R2"))), new FerruleNamedSourceInput("X", Scalar(Text("X"))) }, Kind: FerruleRuntimeError.DuplicateNamedSource, Name: "Ref", Reached: 2),
        })
        {
            var probe = new AdmissionProbe(row.Values); var result = AdmissionNormalize(row.Id, AdmissionPrimary(), AdmissionCatalogue, probe);
            AdmissionOrdinaryError(result, row.Kind, row.Name); AdmissionReads(probe, 1, Enumerable.Range(0, row.Reached).ToArray());
        }
        var reserved = AdmissionNormalize("N527-14", AdmissionPrimary(), [new("Dyn", FerruleBasicNamedSourceMode.ReservedUnavailable), new("Ref", FerruleBasicNamedSourceMode.Optional)], new AdmissionProbe(new FerruleNamedSourceInput("Dyn", Scalar(Text("D")))), new FerruleOwnedSourceLimits(3, 4, 8, 4096, 8));
        AdmissionOrdinaryError(reserved, FerruleRuntimeError.UnexpectedNamedSource, "Dyn");
    }
    private static void NamedAdmissionCatalogueFailures()
    {
        foreach (var row in new[] { (Id: "N527-16", Cat: new[] { new FerruleBasicNamedSourceDeclaration("Ref", FerruleBasicNamedSourceMode.Optional), new FerruleBasicNamedSourceDeclaration("Ref", FerruleBasicNamedSourceMode.Optional) }, Index: (nuint)1), (Id: "N527-18", Cat: new[] { new FerruleBasicNamedSourceDeclaration("Ref", (FerruleBasicNamedSourceMode)99) }, Index: (nuint)0) })
        {
            var probe = new AdmissionProbe(new FerruleNamedSourceInput("Ref", AdmissionNamed())); var result = AdmissionNormalize(row.Id, AdmissionPrimary(), row.Cat, probe); var outer = result.Error as FerruleRuntimeException; var cause = outer?.BasicSourceBridgeFailure;
            AdmissionCheck(outer is not null && outer.Error == FerruleRuntimeError.BasicSourceBridge && cause is not null && ReferenceEquals(cause, outer.InnerException), "bare bridge cause identity");
            AdmissionCheck(cause!.Error == FerruleBasicSourceBridgeErrorKind.InvalidDeclarationWitness && cause.DeclarationIndex == row.Index && cause.DeclaredCount == (nuint)row.Cat.Length && cause.Component is null && cause.Used is null && cause.Requested is null && cause.Maximum is null && cause.InputOrdinal is null && cause.CandidateOrdinal is null && cause.Found is null && cause.InnerException is null, "complete declaration witness error"); AdmissionReads(probe, 0);
        }
        var p17 = new AdmissionProbe(new FerruleNamedSourceInput("Ref", AdmissionNamed())); var empty = AdmissionNormalize("N527-17", AdmissionPrimary(), [new("", FerruleBasicNamedSourceMode.Optional)], p17);
        AdmissionOwnedError(empty, FerruleOwnedSourceErrorKind.InvalidNamedOwner); AdmissionReads(p17, 0);
    }
    private static void NamedAdmissionRootAndByteLimits()
    {
        var ordinary = new AdmissionProbe(new FerruleNamedSourceInput("Ref", AdmissionNamed())); var rootLimit = AdmissionNormalize("N527-19", AdmissionPrimary(), AdmissionCatalogue, ordinary, AdmissionLimits with { MaxRoots = 1 });
        AdmissionOwnedError(rootLimit, FerruleOwnedSourceErrorKind.MetadataLimit, FerruleOwnedSourceLimitKind.Roots, 0, 2, 1); AdmissionReads(ordinary, 0);
        var p20 = new AdmissionProbe(new FerruleNamedSourceInput("Ref", AdmissionNamed())); var names = AdmissionNormalize("N527-20", AdmissionPrimary(), AdmissionCatalogue, p20, AdmissionLimits with { MaxMetadataBytes = 2 });
        AdmissionOwnedError(names, FerruleOwnedSourceErrorKind.MetadataLimit, FerruleOwnedSourceLimitKind.MetadataBytes, 0, 3, 2); AdmissionReads(p20, 0);
        var empty = AdmissionSuccess(AdmissionNormalize("N527-21", AdmissionPrimary(), AdmissionCatalogue, new AdmissionProbe(), AdmissionLimits with { MaxMetadataBytes = 3 })); AdmissionCheck(empty.OwnedRoots.RegistryText == 3 && empty.OwnedRoots.Instance(1) is null, "exact no-query byte boundary");
        var p22 = new AdmissionProbe(new FerruleNamedSourceInput("X", Scalar(Text("X")))); var tooLong = AdmissionNormalize("N527-22", AdmissionPrimary(), AdmissionCatalogue, p22, AdmissionLimits with { MaxMetadataBytes = 3 });
        AdmissionOwnedError(tooLong, FerruleOwnedSourceErrorKind.MetadataLimit, FerruleOwnedSourceLimitKind.MetadataBytes, 3, 1, 3);
        var exact = AdmissionNormalize("N527-23", AdmissionPrimary(), AdmissionCatalogue, new AdmissionProbe(new FerruleNamedSourceInput("X", Scalar(Text("X")))), AdmissionLimits with { MaxMetadataBytes = 4 }); AdmissionOrdinaryError(exact, FerruleRuntimeError.UnexpectedNamedSource, "X");
        var overflow = AdmissionNormalize("N527-24", Scalar(Text("P.root")), [], new AdmissionProbe(), new FerruleOwnedSourceLimits(nuint.MaxValue, 0, 0, 0, 0)); AdmissionOwnedError(overflow, FerruleOwnedSourceErrorKind.MetadataArithmeticOverflow, FerruleOwnedSourceLimitKind.MetadataSlots);
    }
    private static FerruleBasicSourceRoots AdmissionRowsRoots(string id, bool primaryRows = false, bool same = false)
    {
        if (primaryRows) return AdmissionSuccess(AdmissionNormalize(id, Repeated(Scalar(Text("P1")), Scalar(Text("P2"))), [], new AdmissionProbe(), new FerruleOwnedSourceLimits(1, 4, 8, 4096, 8)));
        var rows = Repeated(Scalar(Text("R1")), Scalar(Text("R2")), Scalar(Text("R3")));
        return AdmissionSuccess(AdmissionNormalize(id, same ? rows : Scalar(Text("P.root")), AdmissionCatalogue, new AdmissionProbe(new FerruleNamedSourceInput("Ref", rows)), new FerruleOwnedSourceLimits(2, 4, 8, 16384, 3)));
    }
    private static (FerruleValue[] Values, nuint[] Positions, nuint[] Raw, int[] Owners) AdmissionWalkRows(string id, FerruleBasicScopeContext context, FerruleOwnedSourceOwner owner)
    {
        var original = AdmissionSuccess(AdmissionReached(id + ".walk", new { context = BasicContextSnapshot(context), owner = owner.Name, path = Array.Empty<string>() }, null, () => context.WalkOwnedSourceCohort(owner, Array.Empty<string>())));
        var values = new FerruleValue[original.Count]; var positions = new nuint[original.Count]; var raw = new nuint[original.Count]; var owners = new int[original.Count];
        for (var i = 0; i < original.Count; i++)
        {
            var candidate = original[i]; raw[i] = candidate.RawSourceOrdinal ?? 0; owners[i] = candidate.Context.OwnedView.FrameAt(candidate.Context.OwnedView.FrameCount - 1).Owner; values[i] = AdmissionField(id + "." + i, candidate.Context, owner);
            var positionCollection = Array.Empty<string>();
            positions[i] = AdmissionSuccess(AdmissionReached(id + ".position." + i, new { candidate = BasicCandidateSnapshot(candidate), owner = new { owner.IsPrimary, owner.Name }, collection = positionCollection }, null, () => candidate.Context.OwnedPosition(owner, positionCollection)));
        }
        return (values, positions, raw, owners);
    }
    private static void NamedAdmissionRepeatedRootsAndSharedInstances()
    {
        var root25 = AdmissionRoot("N527-25", AdmissionRowsRoots("N527-25", primaryRows: true)); var r25 = AdmissionWalkRows("N527-25", root25, FerruleOwnedSourceOwner.Primary);
        AdmissionCheck(r25.Values.SequenceEqual(new[] { Text("P1"), Text("P2") }) && r25.Positions.SequenceEqual(new nuint[] { 1, 2 }) && r25.Raw.SequenceEqual(new nuint[] { 1, 2 }) && r25.Owners.SequenceEqual(new[] { 0, 0 }), "primary repeated-root values/positions");
        var roots26 = AdmissionRowsRoots("N527-26"); var root26 = AdmissionRoot("N527-26", roots26); var r26 = AdmissionWalkRows("N527-26", root26, FerruleOwnedSourceOwner.Named("Ref"));
        AdmissionCheck(r26.Values.SequenceEqual(new[] { Text("R1"), Text("R2"), Text("R3") }) && r26.Positions.SequenceEqual(new nuint[] { 1, 2, 3 }) && roots26.UniqueIndex(0) == 0 && r26.Raw.SequenceEqual(new nuint[] { 1, 2, 3 }) && r26.Owners.SequenceEqual(new[] { 1, 1, 1 }), "named repeated-root values/positions/original index");
        var roots27 = AdmissionRowsRoots("N527-27", same: true); var root27 = AdmissionRoot("N527-27", roots27); var p27 = AdmissionWalkRows("N527-27.primary", root27, FerruleOwnedSourceOwner.Primary); var n27 = AdmissionWalkRows("N527-27.named", root27, FerruleOwnedSourceOwner.Named("Ref"));
        AdmissionCheck(ReferenceEquals(roots27.OwnedRoots.Instance(0), roots27.OwnedRoots.Instance(1)) && p27.Positions.SequenceEqual(new nuint[] { 1, 2, 3 }) && n27.Positions.SequenceEqual(new nuint[] { 1, 2, 3 }) && p27.Owners.SequenceEqual(new[] { 0, 0, 0 }) && n27.Owners.SequenceEqual(new[] { 1, 1, 1 }), "same payload keeps distinct owner keys");
    }
    private static void NamedAdmissionCompletePhaseStorage()
    {
        foreach (var row in new[] { (Id: "N527-28", Indices: new nuint[] { 2, 0 }), (Id: "N527-29", Indices: Array.Empty<nuint>()) })
        {
            var roots = AdmissionRowsRoots(row.Id); var context = AdmissionRoot(row.Id, roots); var owner = FerruleOwnedSourceOwner.Named("Ref");
            var originalPath = Array.Empty<string>();
            var original = AdmissionSuccess(AdmissionReached(row.Id + ".original", new { context = BasicContextSnapshot(context), owner = new { owner.IsPrimary, owner.Name }, path = originalPath }, null, () => context.WalkOwnedSourceCohort(owner, originalPath)));
            var selection = AdmissionSuccess(AdmissionReached(row.Id + ".selection", new { original = BasicReachedSnapshot(original), indices = row.Indices.Select(i => (ulong)i).ToArray() }, null, () => FerrulePhaseSelection.FromOriginal(original, row.Indices)));
            var phase = AdmissionSuccess(AdmissionReached(row.Id + ".phase", new { original = BasicReachedSnapshot(original), selection = BasicReachedSnapshot(selection), context = BasicContextSnapshot(context) }, null, () => context.ReindexRetainedPhase(original, selection)));
            var values = phase.Select((candidate, i) => AdmissionField(row.Id + ".value." + i, candidate.Context, owner)).ToArray();
            var positionCollection = Array.Empty<string>();
            var positions = phase.Select((candidate, i) => AdmissionSuccess(AdmissionReached(row.Id + ".position." + i, new { candidate = BasicCandidateSnapshot(candidate), owner = new { owner.IsPrimary, owner.Name }, collection = positionCollection }, null, () => candidate.Context.OwnedPosition(owner, positionCollection)))).ToArray();
            var originalValues = original.Select((candidate, i) => AdmissionField(row.Id + ".held." + i, candidate.Context, owner)).ToArray();
            var originalRaw = original.Select(candidate => candidate.RawSourceOrdinal ?? 0).ToArray();
            Console.WriteLine("N527_RAW " + System.Text.Json.JsonSerializer.Serialize(new { caseId = row.Id + ".held-raw", original = BasicReachedSnapshot(original), rawPositions = originalRaw.Select(i => (ulong)i).ToArray() }));
            AdmissionCheck(originalValues.SequenceEqual(new[] { Text("R1"), Text("R2"), Text("R3") }) && originalRaw.SequenceEqual(new nuint[] { 1, 2, 3 }) && original.Count == 3 && selection.Current.Count == 3, "complete held original/current conserved");
            if (row.Id == "N527-28") AdmissionCheck(values.SequenceEqual(new[] { Text("R3"), Text("R1") }) && positions.SequenceEqual(new nuint[] { 1, 2 }), "ordered phase output and effective positions");
            else AdmissionCheck(values.Length == 0 && positions.Length == 0 && phase.Count == 0, "empty selection still accounts held full cohort");
        }
    }
    private static void NamedAdmissionRegistrySeedSnapshots()
    {
        var roots = AdmissionSuccess(AdmissionNormalize("N527-30", AdmissionPrimary(), AdmissionCatalogue, new AdmissionProbe(new FerruleNamedSourceInput("Ref", AdmissionNamed()))));
        var direct = AdmissionSuccess(AdmissionReached("N527-30.direct-root", AdmissionSnapshot(roots), null, () => roots.OwnedRoots.RootContext()));
        var queryOwner = FerruleOwnedSourceOwner.Named("Ref"); var queryPath = new[] { "Value" }; IReadOnlyList<string>? queryFrame = null;
        var query = AdmissionSuccess(AdmissionReached("N527-30.direct-query", new { context = AdmissionSnapshot(direct), owner = new { queryOwner.IsPrimary, queryOwner.Name }, path = queryPath, frame = queryFrame }, null, () => new OwnedQuery(direct, queryOwner, queryPath, queryFrame)));
        var context = AdmissionRoot("N527-30", roots);
        var budget = new BasicSourceBudget(AdmissionLimits, false); roots.Seed(budget, false);
        Console.WriteLine("N527_RAW " + System.Text.Json.JsonSerializer.Serialize(new { caseId = "N527-30.seed", roots = AdmissionSnapshot(roots), retainedSlots = (ulong)budget.Slots.Used, bytes = (ulong)budget.Bytes.Used }));
        var clone = AdmissionSuccess(AdmissionReached("N527-30.clone", BasicContextSnapshot(context), null, () => context.CloneView()));
        var walkOwner = FerruleOwnedSourceOwner.Named("Ref"); var walkPath = Array.Empty<string>();
        var walk = AdmissionSuccess(AdmissionReached("N527-30.walk", new { context = BasicContextSnapshot(context), owner = new { walkOwner.IsPrimary, walkOwner.Name }, path = walkPath }, null, () => context.WalkOwnedSourceCohort(walkOwner, walkPath)));
        var selectionIndices = new nuint[] { 0 };
        var selected = AdmissionSuccess(AdmissionReached("N527-30.selection", new { original = BasicReachedSnapshot(walk), indices = selectionIndices.Select(i => (ulong)i).ToArray() }, null, () => FerrulePhaseSelection.FromOriginal(walk, selectionIndices)));
        var phase = AdmissionSuccess(AdmissionReached("N527-30.phase", new { original = BasicReachedSnapshot(walk), selection = BasicReachedSnapshot(selected), context = BasicContextSnapshot(context) }, null, () => context.ReindexRetainedPhase(walk, selected)));
        AdmissionCheck(roots.OwnedRoots.RegistryCount == 2 && roots.OwnedRoots.RegistryStorageCount == 3 && roots.OwnedRoots.AmbiguityReservationCount == 1 && roots.OwnedRoots.RegistryText == 3, "semantic count/table/header/name footprint");
        AdmissionCheck(budget.Bytes.Used == 3 && budget.Slots.Used == 10 && clone.OriginCount == context.OriginCount && phase.Count == 1 && query.Roots.RegistryStorageCount == 3, "same retained seeds survive query/clone/walk/full phase");
    }
    private static void NamedAdmissionNullArgumentOrder()
    {
        var p31 = new AdmissionProbe(new FerruleNamedSourceInput("Ref", AdmissionNamed())); var primary = AdmissionNormalize("N527-31", null, AdmissionCatalogue, p31);
        AdmissionCheck(primary.Error is ArgumentNullException { ParamName: "primary" }, "primary null first"); AdmissionReads(p31, 0);
        var supplied = AdmissionNormalize("N527-32", AdmissionPrimary(), AdmissionCatalogue, null); AdmissionCheck(supplied.Error is ArgumentNullException { ParamName: "supplied" }, "supplied null first");
        var p33 = new AdmissionProbe(new FerruleNamedSourceInput(null, Scalar(Text("R")))); var name = AdmissionNormalize("N527-33", AdmissionPrimary(), AdmissionCatalogue, p33); AdmissionCheck(name.Error is ArgumentNullException { ParamName: "supplied" }, "null projected name"); AdmissionReads(p33, 1, 0);
        var p34 = new AdmissionProbe(new FerruleNamedSourceInput("X", null)); var instance = AdmissionNormalize("N527-34", AdmissionPrimary(), AdmissionCatalogue, p34); AdmissionCheck(instance.Error is ArgumentNullException { ParamName: "supplied" }, "null instance precedes unknown name"); AdmissionReads(p34, 1, 0);
    }
    private static void NamedAdmissionMalformedUtf16()
    {
        var probe = new AdmissionProbe(new FerruleNamedSourceInput(new string((char)0xD800, 1), Scalar(Text("X")))); var result = AdmissionNormalize("N527-35", AdmissionPrimary(), AdmissionCatalogue, probe);
        AdmissionCheck(result.Error is ArgumentException { ParamName: "supplied" } && result.Error.Message.StartsWith("Text must contain valid Unicode scalar values.", StringComparison.Ordinal), "standard constructor literal/ParamName, no lossy replacement"); AdmissionReads(probe, 1, 0);
    }
    private sealed class TestSentinelException : Exception { internal TestSentinelException() : base("sentinel") { } }
    private static void NamedAdmissionFrozenCountAndCallerErrors()
    {
        var cat = new[] { new FerruleBasicNamedSourceDeclaration("Ref", FerruleBasicNamedSourceMode.Optional), new FerruleBasicNamedSourceDeclaration("Other", FerruleBasicNamedSourceMode.Optional) };
        var p36 = new AdmissionProbe(new FerruleNamedSourceInput("Ref", Scalar(Text("R"))), new FerruleNamedSourceInput("Other", Scalar(Text("O")))) { ChangeCountAtZero = true };
        var roots36 = AdmissionSuccess(AdmissionNormalize("N527-36", AdmissionPrimary(), cat, p36)); var c36 = AdmissionRoot("N527-36", roots36); var r36 = AdmissionField("N527-36.Ref", c36, FerruleOwnedSourceOwner.Named("Ref")); var o36 = AdmissionField("N527-36.Other", c36, FerruleOwnedSourceOwner.Named("Other"));
        Equal(Text("R"), r36); Equal(Text("O"), o36); AdmissionReads(p36, 1, 0, 1);
        var p37 = new AdmissionProbe(new FerruleNamedSourceInput("Ref", Scalar(Text("R.original")))) { ChangedZero = new FerruleNamedSourceInput("Ref", Scalar(Text("R.changed"))) };
        var roots37 = AdmissionSuccess(AdmissionNormalize("N527-37", AdmissionPrimary(), AdmissionCatalogue, p37)); var c37 = AdmissionRoot("N527-37", roots37); var r37 = AdmissionField("N527-37", c37, FerruleOwnedSourceOwner.Named("Ref"));
        Equal(Text("R.original"), r37); AdmissionReads(p37, 1, 0);
        var sentinel = new TestSentinelException(); var p38 = new AdmissionProbe(new FerruleNamedSourceInput("Ref", Scalar(Text("R"))), default, new FerruleNamedSourceInput("X", Scalar(Text("unreached")))) { ThrowIndex = 1, Sentinel = sentinel };
        var failed = AdmissionNormalize("N527-38", AdmissionPrimary(), cat, p38);
        AdmissionCheck(ReferenceEquals(failed.Error, sentinel), "exact caller exception, no wrapper"); AdmissionCheck(p38.CountReads == 1 && p38.IndexReads.SequenceEqual(new[] { 0, 1 }) && p38.AccessorCalls == 1 && p38.EnumeratorCalls == 0, "no retry or unconsumed suffix");
        var typedSentinel = new FerruleOwnedSourceException(FerruleOwnedSourceErrorKind.InvalidNamedOwner);
        var typedProbe = new AdmissionProbe(new FerruleNamedSourceInput("Ref", Scalar(Text("R"))), default, new FerruleNamedSourceInput("X", Scalar(Text("unreached")))) { ThrowIndex = 1, Sentinel = typedSentinel };
        var typedFailure = AdmissionNormalize("N527-38.typed-caller", AdmissionPrimary(), cat, typedProbe);
        AdmissionCheck(ReferenceEquals(typedFailure.Error, typedSentinel), "caller typed cause is not lifted or replaced");
        AdmissionCheck(typedProbe.CountReads == 1 && typedProbe.IndexReads.SequenceEqual(new[] { 0, 1 }) && typedProbe.AccessorCalls == 1 && typedProbe.EnumeratorCalls == 0, "typed caller suffix remains unread");
    }
    private static void NamedAdmissionNegativeCount()
    {
        var probe = new AdmissionProbe(new FerruleNamedSourceInput("Ref", AdmissionNamed())) { ExposedCount = -1, ThrowIndex = 0, Sentinel = new TestSentinelException() };
        var result = AdmissionNormalize("N527-39", AdmissionPrimary(), AdmissionCatalogue, probe);
        AdmissionCheck(result.Error is ArgumentOutOfRangeException { ParamName: "supplied", ActualValue: -1 } && result.Error.Message.StartsWith("Count cannot be negative.", StringComparison.Ordinal), "native negative Count argument payload"); AdmissionReads(probe, 1);
    }
}
