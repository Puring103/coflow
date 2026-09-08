using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

using global::Coflow.Runtime;

internal sealed class CoflowExternalValue
{
    internal CoflowExternalValue(CoflowValueId id, CoflowTypeId typeId, object apiValue)
    {
        Id = id;
        TypeId = typeId;
        ApiValue = apiValue;
    }

    internal CoflowValueId Id { get; }
    internal CoflowTypeId TypeId { get; }
    internal object ApiValue { get; set; }
    internal CoflowEncodedValue? ArenaValue { get; set; }
}

internal abstract class CoflowTransientValues
{
    internal static CoflowTransientValues None { get; } = new EmptyTransientValues();

    internal abstract bool TryGet(CoflowValueId id, out CoflowExternalValue value);
    internal abstract bool TryGetClosure(CoflowValueId id, out CoflowClosure closure);

    private sealed class EmptyTransientValues : CoflowTransientValues
    {
        internal override bool TryGet(CoflowValueId id, out CoflowExternalValue value)
        {
            value = null!;
            return false;
        }

        internal override bool TryGetClosure(CoflowValueId id, out CoflowClosure closure)
        {
            closure = null!;
            return false;
        }
    }
}

/// <summary>唯一拥有调用期导入值、closure、临时 ID 和结果提升图。</summary>
internal sealed class CoflowImportState : CoflowTransientValues
{
    private readonly Dictionary<uint, CoflowExternalValue> _values = new();
    private readonly Dictionary<uint, CoflowClosure> _closures = new();
    private readonly CoflowImportContext _import = new();
    private readonly CoflowCollectionArena _hostResultCollections = new();
    private readonly Func<uint> _allocateCollectionIndex;
    private CoflowSnapshot? _snapshot;
    private CoflowExecutionBudget? _budget;
    private uint _nextValueIndex;
    private uint _nextCollectionIndex;
    private uint _lastValueIndex;

    internal CoflowImportState() => _allocateCollectionIndex = AllocateCollectionIndex;

    private CoflowSnapshot Snapshot => _snapshot ??
        throw new InvalidOperationException("The Coflow import state is not active.");
    private CoflowExecutionBudget Budget => _budget ??
        throw new InvalidOperationException("The Coflow import state is not active.");

    internal void Start(CoflowSnapshot snapshot, CoflowExecutionBudget budget)
    {
        _snapshot = snapshot;
        _budget = budget;
        _nextValueIndex = snapshot.InvocationValueIndexBase;
        _nextCollectionIndex = snapshot.InvocationCollectionIndexBase;
        _lastValueIndex = 0;
        _import.Reset(snapshot, this);
        _hostResultCollections.Reset(snapshot.SnapshotId, _nextCollectionIndex,
            _allocateCollectionIndex, budget);
    }

    internal void Clear()
    {
        if (_snapshot is not null)
            _snapshot.CommitInvocationIndexes(_nextValueIndex, _nextCollectionIndex);
        _values.Clear();
        _closures.Clear();
        _import.Clear();
        _hostResultCollections.Clear();
        _snapshot = null;
        _budget = null;
        _nextValueIndex = 0;
        _nextCollectionIndex = 0;
        _lastValueIndex = 0;
    }

    internal T Import<T>(T value, CoflowCollectionArena collections) => _import.Import(value, collections);

    internal T ImportHostResult<T>(T value)
    {
        var imported = _import.Import(value, _hostResultCollections);
        return PromoteResult(imported, _hostResultCollections);
    }

    internal CoflowValueId Attach(CoflowTypeId typeId, object apiValue)
    {
        Budget.InvocationValue();
        var id = new CoflowValueId(Snapshot.SnapshotId, checked(++_nextValueIndex));
        _values.Add(id.Index, new CoflowExternalValue(id, typeId, apiValue));
        _lastValueIndex = id.Index;
        return id;
    }

    internal CoflowValueId AttachClosure(CoflowClosure closure)
    {
        Budget.InvocationValue();
        var id = new CoflowValueId(Snapshot.SnapshotId, checked(++_nextValueIndex));
        _closures.Add(id.Index, closure);
        return id;
    }

    internal uint AllocateCollectionIndex() => checked(++_nextCollectionIndex);

    internal void SetLastValue(object value, CoflowEncodedValue arenaValue)
    {
        if (_lastValueIndex == 0 || !_values.TryGetValue(_lastValueIndex, out var external))
            throw new InvalidOperationException("No external value is awaiting its Arena row.");
        external.ApiValue = value;
        external.ArenaValue = arenaValue;
    }

    internal override bool TryGet(CoflowValueId id, out CoflowExternalValue value)
    {
        if (id.SnapshotId == Snapshot.SnapshotId && _values.TryGetValue(id.Index, out value!))
            return true;
        value = null!;
        return false;
    }

    internal override bool TryGetClosure(CoflowValueId id, out CoflowClosure closure)
    {
        if (id.SnapshotId == Snapshot.SnapshotId && _closures.TryGetValue(id.Index, out closure!))
            return true;
        closure = null!;
        return false;
    }

    internal T PromoteResult<T>(T value, CoflowCollectionArena collections)
    {
        if (!CoflowEscapeValue<T>.MayContainRecord) return value;
        var collector = new CoflowValueIdCollector();
        CoflowEscapeValue<T>.Collect(value, collector);
        if (collector.Ids.Count != 0) Promote(collector.Ids, collections);
        return value;
    }

    private void Promote(IReadOnlyCollection<CoflowValueId> ids, CoflowCollectionArena collections)
    {
        if (ids.Count == 0) return;
        // closure 环境和集合元素都是值图节点；工作队列保证每个调用期节点只展开一次。
        var reachable = ids.ToHashSet();
        var reachableCollections = new HashSet<CoflowCollectionId>();
        var pending = new Queue<CoflowValueId>(reachable);
        while (pending.Count != 0)
        {
            var current = pending.Dequeue();
            var collector = new CoflowValueIdCollector();
            if (_closures.TryGetValue(current.Index, out var closure))
            {
                closure.CollectValueIds(collector);
            }
            else if (_values.TryGetValue(current.Index, out var external) && external.ArenaValue is not null)
            {
                Snapshot.CollectArenaRowValueIds(external, collector,
                    reachableCollections, ResolveCollectionArena);
            }
            foreach (var id in collector.Ids)
                if (reachable.Add(id)) pending.Enqueue(id);
        }
        var escapedCollections = reachableCollections.Any(collections.Contains)
            ? new[] { collections.Freeze(reachableCollections) }
            : Array.Empty<CoflowCollectionArena>();
        Snapshot.Promote(
            _values.Values.Where(value => reachable.Contains(value.Id)),
            _closures.Where(value => reachable.Contains(
                    new CoflowValueId(Snapshot.SnapshotId, value.Key)))
                .Select(value => (new CoflowValueId(Snapshot.SnapshotId, value.Key), value.Value)),
            escapedCollections);
        Snapshot.CommitInvocationIndexes(_nextValueIndex, _nextCollectionIndex);

        CoflowCollectionArena ResolveCollectionArena(CoflowCollectionId id) =>
            collections.Contains(id) ? collections : Snapshot.CollectionArena(id);
    }
}
}
