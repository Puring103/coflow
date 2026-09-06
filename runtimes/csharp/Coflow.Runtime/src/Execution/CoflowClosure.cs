using System;
using System.Collections.Generic;

namespace Coflow.Runtime.CompilerServices;

internal abstract class CoflowClosure
{
    private sealed class Empty : CoflowClosure
    {
        public Empty(Coflow owner, CoflowProgram program, CoflowCaptureLayout[] captures,
            CoflowCollectionArena[] collections)
            : base(owner, program, captures, collections)
        {
        }

        internal override long Integer(int index)
        {
            throw NoCapture();
        }

        internal override double Float(int index)
        {
            throw NoCapture();
        }

        internal override object? Reference(int index)
        {
            throw NoCapture();
        }

        internal override void SetInteger(int index, long value)
        {
            throw NoCapture();
        }

        internal override void SetFloat(int index, double value)
        {
            throw NoCapture();
        }

        internal override void SetReference(int index, object? value)
        {
            throw NoCapture();
        }

        private static InvalidOperationException NoCapture()
        {
            return new InvalidOperationException("the closure has no captured values");
        }
    }

    private sealed class WithCaptures : CoflowClosure
    {
        private long _integer0;

        private double _float0;

        private object? _reference0;

        private readonly long[]? _integerCaptures;

        private readonly double[]? _floatCaptures;

        private readonly object?[]? _referenceCaptures;

        internal WithCaptures(Coflow owner, CoflowProgram program, CoflowCaptureLayout[] captures,
            CoflowCollectionArena[] collections, int integerCount, int floatCount, int referenceCount)
            : base(owner, program, captures, collections)
        {
            _integerCaptures = ((integerCount > 1) ? new long[integerCount] : null);
            _floatCaptures = ((floatCount > 1) ? new double[floatCount] : null);
            _referenceCaptures = referenceCount > 1 ? new object?[referenceCount] : null;
        }

        internal override long Integer(int index)
        {
            return _integerCaptures is { } captures ? captures[index] : _integer0;
        }

        internal override double Float(int index)
        {
            return _floatCaptures is { } captures ? captures[index] : _float0;
        }

        internal override object? Reference(int index)
        {
            return _referenceCaptures is { } captures ? captures[index] : _reference0;
        }

        internal override void SetInteger(int index, long value)
        {
            if (_integerCaptures is { } captures)
            {
                captures[index] = value;
            }
            else
            {
                _integer0 = value;
            }
        }

        internal override void SetFloat(int index, double value)
        {
            if (_floatCaptures is { } captures)
            {
                captures[index] = value;
            }
            else
            {
                _float0 = value;
            }
        }

        internal override void SetReference(int index, object? value)
        {
            if (_referenceCaptures is { } captures)
            {
                captures[index] = value;
            }
            else
            {
                _reference0 = value;
            }
        }
    }

    internal Coflow Owner { get; }

    internal CoflowProgram Program { get; }

    internal IReadOnlyList<CoflowCaptureLayout> Captures { get; }

    internal IReadOnlyList<CoflowCollectionArena> Collections { get; private set; }

    internal int StorageLaneCount => checked(
        Captures.Sum(capture => capture.Shape.IntegerCount + capture.Shape.FloatCount + capture.Shape.ReferenceCount) +
        Collections.Sum(collection => collection.StorageLaneCount));

    private CoflowClosure(Coflow owner, CoflowProgram program, CoflowCaptureLayout[] captures,
        CoflowCollectionArena[] collections)
    {
        Owner = owner;
        Program = program;
        Captures = captures;
        Collections = collections;
    }

    internal abstract long Integer(int index);

    internal abstract double Float(int index);

    internal abstract object? Reference(int index);

    internal abstract void SetInteger(int index, long value);

    internal abstract void SetFloat(int index, double value);

    internal abstract void SetReference(int index, object? value);

    internal void CollectValueIds(CoflowValueIdCollector collector)
    {
        var visitedCollections = new HashSet<CoflowCollectionId>();
        foreach (var capture in Captures)
            CollectValueIds(capture.Shape, capture.IntegerBase, capture.FloatBase,
                capture.ReferenceBase, collector, visitedCollections);
    }

    internal void RetainReachableCollections()
    {
        if (Collections.Count == 0) return;
        var reachable = new HashSet<CoflowCollectionId>();
        var collector = new CoflowValueIdCollector();
        foreach (var capture in Captures)
            CollectValueIds(capture.Shape, capture.IntegerBase, capture.FloatBase,
                capture.ReferenceBase, collector, reachable);
        Collections = Collections.Where(arena => reachable.Any(arena.Contains)).ToArray();
    }

    private void CollectValueIds(
        CoflowValueShape shape,
        int integerBase,
        int floatBase,
        int referenceBase,
        CoflowValueIdCollector collector,
        HashSet<CoflowCollectionId> visitedCollections)
    {
        CollectValueIds(shape, Integer, Float, Reference, integerBase, floatBase, referenceBase,
            collector, visitedCollections, ResolveCollectionArena);
    }

    internal static void CollectEncodedValueIds(
        CoflowEncodedValue value,
        CoflowValueIdCollector collector,
        HashSet<CoflowCollectionId> visitedCollections,
        Func<CoflowCollectionId, CoflowCollectionArena> resolve) =>
        CollectValueIds(value.Shape,
            index => value.Integers[index], index => value.Floats[index],
            index => value.References[index], 0, 0, 0, collector, visitedCollections, resolve);

    internal static void CollectArenaRowValueIds(
        CoflowEncodedValue value,
        IEnumerable<Type> fieldTypes,
        CoflowValueIdCollector collector,
        HashSet<CoflowCollectionId> visitedCollections,
        Func<CoflowCollectionId, CoflowCollectionArena> resolve)
    {
        var integerBase = 0;
        var floatBase = 0;
        var referenceBase = 0;
        foreach (var fieldType in fieldTypes)
        {
            var shape = CoflowValueShape.Of(fieldType);
            CollectValueIds(shape,
                index => value.Integers[index], index => value.Floats[index],
                index => value.References[index], integerBase, floatBase, referenceBase,
                collector, visitedCollections, resolve);
            integerBase += shape.IntegerCount;
            floatBase += shape.FloatCount;
            referenceBase += shape.ReferenceCount;
        }
    }

    private static void CollectValueIds(
        CoflowValueShape shape,
        Func<int, long> integer,
        Func<int, double> floating,
        Func<int, object?> reference,
        int integerBase,
        int floatBase,
        int referenceBase,
        CoflowValueIdCollector collector,
        HashSet<CoflowCollectionId> visitedCollections,
        Func<CoflowCollectionId, CoflowCollectionArena> resolve)
    {
        if (shape.Kind == CoflowValueShapeKind.Record)
        {
            collector.Add(CoflowValueId.FromPacked(unchecked((ulong)integer(integerBase))));
            return;
        }
        if (shape.Kind == CoflowValueShapeKind.Collection)
        {
            var id = CoflowCollectionId.FromPacked(unchecked((ulong)integer(integerBase)));
            resolve(id).CollectValueIds(id, collector, visitedCollections, resolve);
            return;
        }
        if (shape.Kind == CoflowValueShapeKind.Function)
        {
            collector.Add(CoflowValueId.FromPacked(unchecked((ulong)integer(integerBase + 1))));
            return;
        }
        if (shape.Kind == CoflowValueShapeKind.Struct)
        {
            var descriptor = CoflowStructCodecs.TryGet(shape.Type, out var value)
                ? value : throw new InvalidOperationException($"No schema struct codec exists for `{shape.Type}`.");
            var integerOffset = integerBase;
            var floatOffset = floatBase;
            var referenceOffset = referenceBase;
            foreach (var fieldType in descriptor.FieldTypes)
            {
                var field = CoflowValueShape.Of(fieldType);
                CollectValueIds(field, integer, floating, reference,
                    integerOffset, floatOffset, referenceOffset,
                    collector, visitedCollections, resolve);
                integerOffset += field.IntegerCount;
                floatOffset += field.FloatCount;
                referenceOffset += field.ReferenceCount;
            }
            collector.Add(CoflowValueId.FromPacked(unchecked((ulong)integer(integerOffset))));
            return;
        }
        if (shape.Kind is not (CoflowValueShapeKind.Option or CoflowValueShapeKind.Result)) return;
        var firstActive = integer(integerBase) != 0;
        if (shape.Kind == CoflowValueShapeKind.Option && !firstActive) return;
        var payload = firstActive ? shape.First! : shape.Second!;
        CollectValueIds(payload, integer, floating, reference,
            integerBase + 1 + (firstActive ? 0 : shape.First!.IntegerCount),
            floatBase + (firstActive ? 0 : shape.First!.FloatCount),
            referenceBase + (firstActive ? 0 : shape.First!.ReferenceCount),
            collector, visitedCollections, resolve);
    }

    private CoflowCollectionArena ResolveCollectionArena(CoflowCollectionId id)
    {
        foreach (var arena in Collections)
            if (arena.Contains(id)) return arena;
        return CoflowInvocationContext.CollectionArena(id);
    }

    internal static CoflowClosure Create(CoflowProgram program, CoflowCaptureLayout[] captures,
        CoflowCollectionArena[] collections, int integerCount, int floatCount, int referenceCount)
    {
        return integerCount == 0 && floatCount == 0 && referenceCount == 0
            ? new Empty(CoflowInvocationContext.Owner, program, captures, collections)
            : new WithCaptures(CoflowInvocationContext.Owner, program, captures, collections,
                integerCount, floatCount, referenceCount);
    }
}
