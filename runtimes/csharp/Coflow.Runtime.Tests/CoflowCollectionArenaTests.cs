using Coflow.Runtime;
using Coflow.Runtime.CompilerServices;
using System;
using System.Collections.Generic;
using Xunit;

namespace Coflow.Runtime.Tests;

public sealed class CoflowCollectionArenaTests : IDisposable
{
    private static readonly CoflowSchemaRuntime Runtime = BuildRuntime();
    private readonly CoflowSchemaRuntimeContext.Scope _runtimeScope =
        CoflowSchemaRuntimeContext.Enter(Runtime);

    private static CoflowSchemaRuntime BuildRuntime()
    {
        var runtime = new CoflowSchemaRuntimeBuilder();
        runtime.RegisterOption<long>();
        runtime.RegisterResult<Option<long>, string>();
        runtime.RegisterResult<long, string>();
        runtime.RegisterArray<long>();
        runtime.RegisterDictionary<string, IReadOnlyList<long>>();
        runtime.RegisterOption<double>();
        runtime.RegisterStruct<NestedArenaValue>(1, 0, 1,
            static (ref CoflowValueWriter writer, NestedArenaValue value) =>
            {
                writer.WriteValueId(default);
                writer.Write(value.Text);
            },
            static (ref CoflowValueReader reader) =>
            {
                _ = reader.ReadValueId();
                return new NestedArenaValue(reader.Read<string>());
            });
        return runtime.Build();
    }

    public void Dispose() => _runtimeScope.Dispose();

    [Fact]
    public void NestedCollectionsCompareWithoutMaterializationOrWarmAllocations()
    {
        using var context = new CoflowExecutionSession();
        context.Collections.Reset(7);
        var type = typeof(IReadOnlyDictionary<string, IReadOnlyList<long>>);
        var shape = CoflowValueShape.Of(type);
        var left = new CoflowValueRegister(shape, 0, 0, 0);
        var right = new CoflowValueRegister(shape, 1, 0, 0);
        var result = new CoflowValueRegister(CoflowValueShape.Of(typeof(bool)), 2, 0, 0);
        var first = new Dictionary<string, IReadOnlyList<long>> { ["a"] = new long[] { 1, 2 }, ["b"] = new long[] { 3 } };
        var second = new Dictionary<string, IReadOnlyList<long>> { ["b"] = new long[] { 3 }, ["a"] = new long[] { 1, 2 } };
        context.WriteEncodedRelative(CoflowCollectionEncoding.Encode(type, first, context.Collections), left);
        context.WriteEncodedRelative(CoflowCollectionEncoding.Encode(type, second, context.Collections), right);
        var call = CoflowEquality.Create(type);
        var frame = new CoflowNativeFrame(context, new[] { left, right }, result, typeof(bool));
        for (var index = 0; index < 100; index++) call.Invoke(frame);
        var allocated = GC.GetAllocatedBytesForCurrentThread();
        for (var index = 0; index < 1000; index++) call.Invoke(frame);
        allocated = GC.GetAllocatedBytesForCurrentThread() - allocated;
        Assert.Equal(0, allocated);
        Assert.Equal(1, context.Registers.ReadInteger(result.Scalar));
        second["a"] = new long[] { 1, 4 };
        context.WriteEncodedRelative(CoflowCollectionEncoding.Encode(type, second, context.Collections), right);
        call.Invoke(frame);
        Assert.Equal(0, context.Registers.ReadInteger(result.Scalar));
    }

    [Fact]
    public void CompoundEqualityIgnoresInactivePayloadAndPreservesNaNSemantics()
    {
        using var context = new CoflowExecutionSession();
        var type = typeof(Option<double>);
        var shape = CoflowValueShape.Of(type);
        var left = new CoflowValueRegister(shape, 0, 0, 0);
        var right = new CoflowValueRegister(shape, 1, 1, 0);
        var result = new CoflowValueRegister(CoflowValueShape.Of(typeof(bool)), 2, 0, 0);
        context.WriteEncodedRelative(CoflowEncodedValue.Encode(type, Option<double>.None), left);
        context.WriteEncodedRelative(CoflowEncodedValue.Encode(type, Option<double>.None), right);
        context.Registers.WriteFloatRelative(0, double.NaN);
        var call = CoflowEquality.Create(type);
        var frame = new CoflowNativeFrame(context, new[] { left, right }, result, typeof(bool));
        call.Invoke(frame);
        Assert.Equal(1, context.Registers.ReadInteger(result.Scalar));
        context.Registers.WriteIntegerRelative(0, 1);
        context.Registers.WriteIntegerRelative(1, 1);
        context.Registers.WriteFloatRelative(1, double.NaN);
        call.Invoke(frame);
        Assert.Equal(0, context.Registers.ReadInteger(result.Scalar));
    }

    [Fact]
    public void ArrayItemsUseTheirPhysicalLanes()
    {
        var arena = new CoflowCollectionArena();
        arena.Reset(7);
        var type = typeof(Result<Option<long>, string>);
        var handle = arena.AddArray(CoflowValueShape.Of(type), new[]
        {
            CoflowEncodedValue.Encode(type, Result<Option<long>, string>.Ok(Option<long>.Some(7))),
            CoflowEncodedValue.Encode(type, Result<Option<long>, string>.Err("failure")),
        });

        Assert.Equal(2, arena.ItemCount(handle));
        var first = arena.ReadArrayItem(handle, 0);
        var second = arena.ReadArrayItem(handle, 1);
        Assert.Equal(new long[] { 1, 1, 7 }, first.Integers);
        Assert.Equal(new object?[] { null }, first.References);
        Assert.Equal(new long[] { 0, 0, 0 }, second.Integers);
        Assert.Equal(new object?[] { "failure" }, second.References);
    }

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public void DictionaryIndexSurvivesFreezeAndSourceClear(bool strings)
    {
        using var context = new CoflowExecutionSession();
        var arena = context.Collections;
        arena.Reset(7);
        var type = strings ? typeof(string) : typeof(long);
        var shape = CoflowValueShape.Of(type);
        var keys = new CoflowEncodedValue[10001];
        for (var index = 0; index < 10000; index++)
            keys[index] = CoflowEncodedValue.Encode(type, strings ? (object)$"key{index}" : (long)index);
        keys[10000] = keys[0];
        var id = arena.AddDictionary(shape, shape, keys, keys);
        var frozen = arena.Freeze();
        var selected = arena.Freeze(new HashSet<CoflowCollectionId> { id });
        arena.Clear();
        var register = new CoflowValueRegister(shape, 0, 0, 0);
        foreach (var index in new[] { 0, 9999, 10001 })
        {
            context.WriteEncodedRelative(CoflowEncodedValue.Encode(type,
                strings ? (object)$"key{index}" : (long)index), register);
            var expected = index == 10001 ? -1 : index;
            Assert.Equal(expected, frozen.FindDictionaryKey(id, context, register));
            Assert.Equal(expected, selected.FindDictionaryKey(id, context, register));
        }
    }

    [Fact]
    public void DictionaryInterleavesKeysAndValuesWithoutMixingRows()
    {
        var arena = new CoflowCollectionArena();
        arena.Reset(7);
        var key = CoflowValueShape.Of(typeof(string));
        var value = CoflowValueShape.Of(typeof(double));
        var handle = arena.AddDictionary(key, value,
            new[]
            {
                CoflowEncodedValue.Encode(typeof(string), "first"),
                CoflowEncodedValue.Encode(typeof(string), "second"),
            },
            new[]
            {
                CoflowEncodedValue.Encode(typeof(double), 1.5),
                CoflowEncodedValue.Encode(typeof(double), 2.5),
            });

        Assert.Equal(CoflowCollectionKind.Dictionary, arena.Kind(handle));
        Assert.Equal("second", arena.ReadDictionaryKey(handle, 1).References[0]);
        Assert.Equal(1.5, arena.ReadDictionaryValue(handle, 0).Floats[0]);
        Assert.Equal(2.5, arena.ReadDictionaryValue(handle, 1).Floats[0]);
    }

    [Fact]
    public void RejectsMismatchedDictionaryCountsAndLayouts()
    {
        var arena = new CoflowCollectionArena();
        arena.Reset(7);
        Assert.Throws<InvalidOperationException>(() => arena.AddDictionary(
            CoflowValueShape.Of(typeof(long)), CoflowValueShape.Of(typeof(string)),
            new[] { CoflowEncodedValue.Encode(typeof(long), 1L) },
            Array.Empty<CoflowEncodedValue>()));
        Assert.Throws<InvalidOperationException>(() => arena.AddArray(
            CoflowValueShape.Of(typeof(long)),
            new[] { CoflowEncodedValue.Encode(typeof(double), 1.0) }));
    }

    [Fact]
    public void ClearInvalidatesHandlesAndReleasesEntries()
    {
        var arena = new CoflowCollectionArena();
        arena.Reset(7);
        var handle = arena.AddArray(CoflowValueShape.Of(typeof(string)),
            new[] { CoflowEncodedValue.Encode(typeof(string), "value") });

        arena.Clear();

        Assert.Equal(0, arena.Count);
        Assert.Throws<InvalidOperationException>(() => arena.ItemCount(handle));
    }

    [Fact]
    public void ResetRejectsHandlesFromThePreviousSnapshotGeneration()
    {
        var arena = new CoflowCollectionArena();
        arena.Reset(7);
        var old = arena.AddArray(CoflowValueShape.Of(typeof(long)),
            new[] { CoflowEncodedValue.Encode(typeof(long), 1L) });
        Assert.Equal(old, CoflowCollectionId.FromPacked(old.Packed));

        arena.Reset(8);
        var current = arena.AddArray(CoflowValueShape.Of(typeof(long)),
            new[] { CoflowEncodedValue.Encode(typeof(long), 2L) });

        Assert.Equal((uint)8, current.SnapshotId);
        Assert.Throws<InvalidOperationException>(() => arena.ItemCount(old));
        Assert.Equal(1, arena.ItemCount(current));
    }

    [Fact]
    public void SnapshotAllocatorKeepsInvocationArenaHandlesUnique()
    {
        uint nextIndex = 4;
        uint Allocate() => ++nextIndex;
        var firstArena = new CoflowCollectionArena();
        var secondArena = new CoflowCollectionArena();
        firstArena.Reset(7, 4, Allocate);
        secondArena.Reset(7, 4, Allocate);
        var shape = CoflowValueShape.Of(typeof(long));

        var first = firstArena.AddArray(shape,
            new[] { CoflowEncodedValue.Encode(typeof(long), 1L) });
        var second = secondArena.AddArray(shape,
            new[] { CoflowEncodedValue.Encode(typeof(long), 2L) });
        var frozen = firstArena.Freeze();

        Assert.NotEqual(first, second);
        Assert.True(firstArena.Contains(first));
        Assert.False(firstArena.Contains(second));
        Assert.True(secondArena.Contains(second));
        Assert.False(secondArena.Contains(first));
        Assert.Equal(1L, frozen.ReadArrayItem(first, 0).Integers[0]);
    }

    [Fact]
    public void ReachableFreezeDoesNotRetainUnrelatedCollections()
    {
        var arena = new CoflowCollectionArena();
        arena.Reset(7);
        var shape = CoflowValueShape.Of(typeof(string));
        var retained = arena.AddArray(shape,
            new[] { CoflowEncodedValue.Encode(typeof(string), "retained") });
        var unrelated = arena.AddArray(shape,
            new[] { CoflowEncodedValue.Encode(typeof(string), "unrelated") });

        var frozen = arena.Freeze(new HashSet<CoflowCollectionId> { retained });

        Assert.True(frozen.Contains(retained));
        Assert.False(frozen.Contains(unrelated));
        Assert.Equal(1, frozen.Count);
        Assert.Equal(1, frozen.StorageLaneCount);
        Assert.Equal("retained", frozen.ReadArrayItem(retained, 0).References[0]);
    }

    [Fact]
    public void ExecutionContextOwnsAndClearsInvocationCollections()
    {
        var context = new CoflowExecutionSession();
        context.Collections.Reset(7);
        context.Collections.AddArray(CoflowValueShape.Of(typeof(string)),
            new[] { CoflowEncodedValue.Encode(typeof(string), "value") });

        context.Dispose();

        Assert.Equal(0, context.Collections.Count);
    }

    [Fact]
    public void LiteralConstructionCopiesRegisterLanesDirectly()
    {
        var context = new CoflowExecutionSession();
        context.Collections.Reset(9);
        var shape = CoflowValueShape.Of(typeof(Result<long, string>));
        var first = new CoflowValueRegister(shape, 0, 0, 0);
        var second = new CoflowValueRegister(shape, shape.IntegerCount, 0, shape.ReferenceCount);
        context.WriteEncodedRelative(
            CoflowEncodedValue.Encode(typeof(Result<long, string>), Result<long, string>.Ok(11)), first);
        context.WriteEncodedRelative(
            CoflowEncodedValue.Encode(typeof(Result<long, string>), Result<long, string>.Err("error")), second);

        var id = context.Collections.AddArray(shape, context, new[] { first, second });

        Assert.Equal(new long[] { 1, 11 }, context.Collections.ReadArrayItem(id, 0).Integers);
        Assert.Equal("error", context.Collections.ReadArrayItem(id, 1).References[0]);
        context.Dispose();
    }

    [Fact]
    public void StructArenaWriterEncodesScalarFieldsDirectly()
    {
        var encoded = CoflowEncodedValue.EncodeArenaField(
            typeof(NestedArenaValue),
            new NestedArenaValue("source"),
            static (type, value) => type == typeof(string)
                ? CoflowEncodedValue.Encode(type, $"encoded:{value}")
                : CoflowEncodedValue.EncodeArenaField(type, value));

        Assert.Equal("source", encoded.References[0]);
    }

    private readonly record struct NestedArenaValue(string Text);
}
