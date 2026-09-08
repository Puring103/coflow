using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

/// <summary>拥有一次执行中的集合 Arena、捕获集合视图和集合 ID 解析顺序。</summary>
internal sealed class CoflowValueResolver
{
    private readonly CoflowCollectionArena _current = new();
    private IReadOnlyList<CoflowCollectionArena> _captured = Array.Empty<CoflowCollectionArena>();
    private Func<CoflowCollectionId, CoflowCollectionArena>? _publishedResolver;

    internal CoflowCollectionArena Current => _current;

    internal void Start(
        uint generation,
        uint firstIndex,
        Func<uint>? allocateIndex,
        CoflowExecutionBudget? budget,
        IReadOnlyList<CoflowCollectionArena>? captured,
        Func<CoflowCollectionId, CoflowCollectionArena>? publishedResolver)
    {
        _captured = captured ?? Array.Empty<CoflowCollectionArena>();
        _publishedResolver = publishedResolver;
        _current.Reset(generation, firstIndex, allocateIndex, budget);
    }

    internal void AddCaptured(IReadOnlyList<CoflowCollectionArena> collections)
    {
        if (collections.Count != 0)
            _captured = _captured.Concat(collections).Distinct().ToArray();
    }

    internal CoflowCollectionArena Resolve(CoflowCollectionId id)
    {
        if (_current.Contains(id)) return _current;
        foreach (CoflowCollectionArena arena in _captured)
            if (arena.Contains(id)) return arena;
        return (_publishedResolver ?? throw new InvalidOperationException(
            "A collection does not belong to this standalone execution."))(id);
    }

    internal CoflowCollectionKind Kind(CoflowCollectionId id) => Resolve(id).Kind(id);
    internal int ItemCount(CoflowCollectionId id) => Resolve(id).ItemCount(id);
    internal CoflowEncodedValue ReadArrayItem(CoflowCollectionId id, int index) =>
        Resolve(id).ReadArrayItem(id, index);
    internal CoflowEncodedValue ReadDictionaryKey(CoflowCollectionId id, int index) =>
        Resolve(id).ReadDictionaryKey(id, index);
    internal CoflowEncodedValue ReadDictionaryValue(CoflowCollectionId id, int index) =>
        Resolve(id).ReadDictionaryValue(id, index);

    internal CoflowCollectionArena[] FreezeClosureCollections()
    {
        if (_current.Count == 0) return _captured.ToArray();
        // closure 持有独立集合快照，执行 session 可以继续复用自己的可变 Arena。
        return _captured.Append(_current.Freeze()).ToArray();
    }

    internal void Clear()
    {
        _current.Clear();
        _captured = Array.Empty<CoflowCollectionArena>();
        _publishedResolver = null;
    }
}
}
