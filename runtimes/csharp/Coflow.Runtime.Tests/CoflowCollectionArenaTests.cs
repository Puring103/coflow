using Coflow.Runtime;
using Coflow.Runtime.CompilerServices;
using System;
using System.Collections.Generic;
using Xunit;

namespace Coflow.Runtime.Tests;

public sealed class CoflowCollectionArenaTests
{
    static CoflowCollectionArenaTests()
    {
        CoflowValueLayout.RegisterOption<long>();
        CoflowValueLayout.RegisterResult<Option<long>, string>();
        CoflowValueLayout.RegisterResult<long, string>();
        CoflowStructCodec.Register<NestedArenaValue>(1, 0, 1,
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

        Assert.Equal((uint)8, current.Generation);
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
        var context = new CoflowVm.CoflowExecutionContext();
        context.Collections.Reset(7);
        context.Collections.AddArray(CoflowValueShape.Of(typeof(string)),
            new[] { CoflowEncodedValue.Encode(typeof(string), "value") });

        context.Dispose();

        Assert.Equal(0, context.Collections.Count);
    }

    [Fact]
    public void LiteralConstructionCopiesRegisterLanesDirectly()
    {
        var context = new CoflowVm.CoflowExecutionContext();
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
    public void StructArenaWriterUsesTheRecursiveValueEncoder()
    {
        var encoded = CoflowEncodedValue.EncodeArenaField(
            typeof(NestedArenaValue),
            new NestedArenaValue("source"),
            static (type, value) => type == typeof(string)
                ? CoflowEncodedValue.Encode(type, $"encoded:{value}")
                : CoflowEncodedValue.EncodeArenaField(type, value));

        Assert.Equal("encoded:source", encoded.References[0]);
    }

    private readonly record struct NestedArenaValue(string Text);
}
