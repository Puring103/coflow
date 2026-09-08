using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime
{

using global::Coflow.Runtime.CompilerServices;

/// <summary>保存属于单个发布版本的逸出对象；快照本体发布后保持只读。</summary>
internal sealed class CoflowEscapeStore
{
    private readonly Dictionary<uint, CoflowExternalValue> _values = new();
    private readonly Dictionary<uint, CoflowClosure> _closures = new();
    private readonly Dictionary<uint, CoflowCollectionArena> _collections = new();
    private readonly CoflowOptions _options;
    private uint _nextValueIndex;
    private uint _nextCollectionIndex;
    private long _lanes;

    internal CoflowEscapeStore(
        CoflowOptions options,
        uint publishedValueCount,
        uint publishedCollectionCount)
    {
        _options = options ?? throw new ArgumentNullException(nameof(options));
        _nextValueIndex = publishedValueCount;
        _nextCollectionIndex = publishedCollectionCount;
    }

    internal int ValueCount => _values.Count;
    internal uint InvocationValueIndexBase => _nextValueIndex;
    internal uint InvocationCollectionIndexBase => _nextCollectionIndex;

    internal bool TryGetValue(uint index, out CoflowExternalValue value) =>
        _values.TryGetValue(index, out value!);

    internal bool TryGetClosure(uint index, out CoflowClosure closure) =>
        _closures.TryGetValue(index, out closure!);

    internal CoflowCollectionArena? FindCollection(CoflowCollectionId id) =>
        id.IsValid && _collections.TryGetValue(id.Index, out var arena) && arena.Contains(id)
            ? arena : null;

    internal void CommitInvocationIndexes(uint valueIndex, uint collectionIndex)
    {
        // 调用失败或结果未逸出也推进游标，旧临时 ID 永远不能别名到新对象。
        if (valueIndex > _nextValueIndex) _nextValueIndex = valueIndex;
        if (collectionIndex > _nextCollectionIndex) _nextCollectionIndex = collectionIndex;
    }

    internal void Promote(
        IEnumerable<CoflowExternalValue> values,
        IEnumerable<(CoflowValueId Id, CoflowClosure Closure)> closures,
        IEnumerable<CoflowCollectionArena> collections)
    {
        var valueAdditions = values.Where(value => !_values.ContainsKey(value.Id.Index)).ToArray();
        var closureAdditions = closures.Where(closure => !_closures.ContainsKey(closure.Id.Index)).ToArray();
        var collectionAdditions = collections.Where(collection =>
            collection.Indexes.Any(index => !_collections.ContainsKey(index))).ToArray();
        var lanes =
            valueAdditions.Sum(value => (long)(value.ArenaValue?.LaneCount ?? 0)) +
            closureAdditions.Sum(closure => (long)closure.Closure.StorageLaneCount) +
            collectionAdditions.Sum(collection => (long)collection.StorageLaneCount);
        foreach (var collection in collectionAdditions)
            foreach (var index in collection.Indexes)
                if (_collections.TryGetValue(index, out var existing) &&
                    !ReferenceEquals(existing, collection))
                    throw new InvalidOperationException($"Escaped collection index `{index}` is duplicated.");
        EnsureCapacity(checked(valueAdditions.Length + closureAdditions.Length), lanes);
        foreach (var value in valueAdditions) _values.Add(value.Id.Index, value);
        foreach (var closure in closureAdditions) _closures.Add(closure.Id.Index, closure.Closure);
        foreach (var collection in collectionAdditions)
            foreach (var index in collection.Indexes)
            {
                _collections[index] = collection;
            }
        _lanes = checked(_lanes + lanes);
    }

    private void EnsureCapacity(int additions, long lanes)
    {
        if (additions > _options.MaxEscapedValues - _values.Count - _closures.Count)
            throw new CoflowExecutionLimitException(nameof(_options.MaxEscapedValues));
        if (lanes > _options.MaxEscapedLanes - _lanes)
            throw new CoflowExecutionLimitException(nameof(_options.MaxEscapedLanes));
    }
}
}
