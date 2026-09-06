namespace Coflow.Runtime.CompilerServices;

using global::Coflow.Runtime;

internal static class CoflowInvocationContext
{
    [ThreadStatic]
    private static InvocationState? _state;

    [ThreadStatic]
    private static InvocationState? _cached;

    [ThreadStatic]
    private static int _depth;

    internal static Scope Enter(global::Coflow.Runtime.Coflow owner, CoflowSnapshot snapshot)
    {
        if (_depth != 0 && !ReferenceEquals(_state!.Snapshot, snapshot))
            throw new InvalidOperationException("A Coflow invocation cannot switch snapshots during reentrancy.");
        if (_depth == 0)
        {
            _state = _cached ?? new InvocationState();
            _cached = null;
            _state.Reset(owner, snapshot);
        }
        else if (!ReferenceEquals(_state!.Owner, owner))
            throw new InvalidOperationException("A Coflow invocation cannot switch owners during reentrancy.");
        _depth++;
        return new Scope();
    }

    internal static bool TryGetLayout(Type type, out CoflowValueShape layout)
    {
        if (_state is not null) return _state.Snapshot.Layouts.TryGet(type, out layout);
        layout = null!;
        return false;
    }

    internal static CoflowBoundFunction Function(
        CoflowValueId id,
        CoflowTypeId typeId,
        CoflowFieldId fieldId) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("A function field was read outside a Coflow invocation."))
        .Function(id, typeId, fieldId);

    internal static (CoflowFunctionId FunctionId, CoflowValueId EnvironmentId) FunctionHandle(
        CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId)
    {
        var function = Function(id, typeId, fieldId);
        var kind = function.Entry.CompiledProgram is null ? CoflowFunctionKind.Native : CoflowFunctionKind.Program;
        return (new CoflowFunctionId(SnapshotId, kind, function.Entry.TargetIndex), id);
    }

    internal static CoflowLinkedFunction LinkedFunction(int programIndex) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("A VM call was executed outside a Coflow invocation."))
        .LinkedFunction(programIndex);

    internal static CoflowFunctionTarget Function(CoflowFunctionId functionId, CoflowValueId environmentId) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("A VM call was executed outside a Coflow invocation."))
        .Function(functionId, environmentId);

    internal static object ApiValue(CoflowValueId id, Type expectedType) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("A value was materialized outside a Coflow invocation."))
        .ApiValue(id, expectedType);

    internal static bool IsType(CoflowValueId id, Type expectedType) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("A type test occurred outside a Coflow invocation."))
        .IsType(id, expectedType);

    internal static long ReadArenaInteger(CoflowValueId id, int offset) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("Arena access occurred outside a Coflow invocation."))
        .ReadArenaInteger(id, offset);

    internal static double ReadArenaFloat(CoflowValueId id, int offset) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("Arena access occurred outside a Coflow invocation."))
        .ReadArenaFloat(id, offset);

    internal static object? ReadArenaReference(CoflowValueId id, int offset) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("Arena access occurred outside a Coflow invocation."))
        .ReadArenaReference(id, offset);

    internal static void CopyArenaField(CoflowValueId id, CoflowFieldAccess access,
        CoflowVm.CoflowExecutionContext context, CoflowValueRegister target) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("Arena access occurred outside a Coflow invocation."))
        .CopyArenaField(id, access, context, target);

    internal static T Import<T>(T value, CoflowCollectionArena collections) =>
        (_state ?? throw new InvalidOperationException("A value was imported outside a Coflow invocation."))
        .ImportContext.Import(value, collections);

    internal static T PromoteResult<T>(T value, CoflowCollectionArena collections)
    {
        if (!CoflowEscapeValue<T>.MayContainRecord) return value;
        var collector = new CoflowValueIdCollector();
        CoflowEscapeValue<T>.Collect(value, collector);
        if (collector.Ids.Count != 0)
            (_state ?? throw new InvalidOperationException("A value was returned outside a Coflow invocation."))
                .Promote(collector.Ids, collections);
        return value;
    }

    internal static global::Coflow.Runtime.Coflow Owner =>
        _state?.Owner ?? throw new InvalidOperationException("A closure was created outside a Coflow invocation.");

    internal static CoflowExecutionBudget Budget =>
        _state?.Budget ?? throw new InvalidOperationException("A budget was requested outside a Coflow invocation.");

    internal static uint Generation =>
        (_state?.Snapshot ?? throw new InvalidOperationException("A VM context was started outside a Coflow invocation."))
        .Generation;

    internal static uint SnapshotId =>
        (_state?.Snapshot ?? throw new InvalidOperationException("A VM context was started outside a Coflow invocation."))
        .SnapshotId;

    internal static int PublishedCollectionCount =>
        (_state?.Snapshot ?? throw new InvalidOperationException("A VM context was started outside a Coflow invocation."))
        .CollectionCount;

    internal static uint AllocateCollectionIndex() =>
        (_state ?? throw new InvalidOperationException("A collection was created outside a Coflow invocation."))
        .AllocateCollectionIndex();

    internal static CoflowCollectionKind CollectionKind(CoflowCollectionId id) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("A collection was read outside a Coflow invocation."))
        .CollectionKind(id);

    internal static CoflowCollectionArena CollectionArena(CoflowCollectionId id) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("A collection was read outside a Coflow invocation."))
        .CollectionArena(id);

    internal static int CollectionItemCount(CoflowCollectionId id) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("A collection was read outside a Coflow invocation."))
        .CollectionItemCount(id);

    internal static CoflowEncodedValue ReadArrayItem(CoflowCollectionId id, int index) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("A collection was read outside a Coflow invocation."))
        .ReadArrayItem(id, index);

    internal static void CopyArrayItem(CoflowCollectionId id, int index,
        CoflowVm.CoflowExecutionContext context, CoflowValueRegister target) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("A collection was read outside a Coflow invocation."))
        .CopyArrayItem(id, index, context, target);

    internal static CoflowEncodedValue ReadDictionaryKey(CoflowCollectionId id, int index) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("A collection was read outside a Coflow invocation."))
        .ReadDictionaryKey(id, index);

    internal static CoflowEncodedValue ReadDictionaryValue(CoflowCollectionId id, int index) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("A collection was read outside a Coflow invocation."))
        .ReadDictionaryValue(id, index);

    internal static void CopyDictionaryKey(CoflowCollectionId id, int index,
        CoflowVm.CoflowExecutionContext context, CoflowValueRegister target) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("A collection was read outside a Coflow invocation."))
        .CopyDictionaryKey(id, index, context, target);

    internal static void CopyDictionaryValue(CoflowCollectionId id, int index,
        CoflowVm.CoflowExecutionContext context, CoflowValueRegister target) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("A collection was read outside a Coflow invocation."))
        .CopyDictionaryValue(id, index, context, target);

    internal static int FindDictionaryKey(CoflowCollectionId id,
        CoflowVm.CoflowExecutionContext context, CoflowValueRegister key) =>
        (_state?.Snapshot ?? throw new InvalidOperationException("A collection was read outside a Coflow invocation."))
        .FindDictionaryKey(id, context, key);

    internal static CoflowValueId AttachExternal(CoflowTypeId typeId, object apiValue) =>
        (_state ?? throw new InvalidOperationException("A value was imported outside a Coflow invocation."))
        .Attach(typeId, apiValue);

    internal static CoflowValueId AttachClosure(CoflowClosure closure) =>
        (_state ?? throw new InvalidOperationException("A closure was created outside a Coflow invocation."))
        .AttachClosure(closure);

    internal static void SetExternalValue(object value, CoflowEncodedValue arenaValue) =>
        (_state ?? throw new InvalidOperationException("A value was imported outside a Coflow invocation."))
        .SetLastValue(value, arenaValue);

    internal static bool TryGetExternal(CoflowValueId id, out ExternalValue value)
    {
        if (_state is not null && _state.TryGet(id, out value)) return true;
        value = null!;
        return false;
    }

    internal static bool TryGetClosure(CoflowValueId id, out CoflowClosure closure)
    {
        if (_state is not null && _state.TryGetClosure(id, out closure)) return true;
        closure = null!;
        return false;
    }

    internal readonly struct Scope : IDisposable
    {
        public void Dispose()
        {
            _depth--;
            if (_depth == 0)
            {
                var completed = _state!;
                completed.Clear();
                _state = null;
                _cached = completed;
            }
        }
    }

    internal sealed class ExternalValue(CoflowValueId id, CoflowTypeId typeId, object apiValue)
    {
        internal CoflowValueId Id { get; } = id;
        internal CoflowTypeId TypeId { get; } = typeId;
        internal object ApiValue { get; set; } = apiValue;
        internal CoflowEncodedValue? ArenaValue { get; set; }
    }

    private sealed class InvocationState
    {
        private readonly List<ExternalValue> _values = new();
        private readonly List<(CoflowValueId Id, CoflowClosure Closure)> _closures = new();
        private uint _nextValueIndex;
        private uint _nextCollectionIndex;

        internal InvocationState()
        {
            ImportContext = new CoflowImportContext();
            Budget = new CoflowExecutionBudget(global::Coflow.Runtime.CoflowOptions.Default);
        }
        internal CoflowSnapshot Snapshot { get; private set; } = null!;
        internal global::Coflow.Runtime.Coflow Owner { get; private set; } = null!;
        internal CoflowImportContext ImportContext { get; }
        internal CoflowExecutionBudget Budget { get; }

        internal void Reset(global::Coflow.Runtime.Coflow owner, CoflowSnapshot snapshot)
        {
            Owner = owner;
            Snapshot = snapshot;
            ImportContext.Reset(snapshot);
            Budget.Reset(owner.Options);
            _nextValueIndex = snapshot.InvocationValueIndexBase;
            _nextCollectionIndex = snapshot.InvocationCollectionIndexBase;
        }

        internal void Clear()
        {
            // 线程缓存归还前必须释放调用期 Arena 持有的全部应用对象。
            _values.Clear();
            _closures.Clear();
            Owner = null!;
            Snapshot = null!;
            Budget.Reset(global::Coflow.Runtime.CoflowOptions.Default);
        }

        internal CoflowValueId Attach(CoflowTypeId typeId, object apiValue)
        {
            Budget.InvocationValue();
            var id = new CoflowValueId(Snapshot.Generation, checked(++_nextValueIndex));
            _values.Add(new ExternalValue(id, typeId, apiValue));
            return id;
        }

        internal CoflowValueId AttachClosure(CoflowClosure closure)
        {
            Budget.InvocationValue();
            var id = new CoflowValueId(Snapshot.Generation, checked(++_nextValueIndex));
            _closures.Add((id, closure));
            return id;
        }

        internal uint AllocateCollectionIndex() => checked(++_nextCollectionIndex);

        internal void SetLastValue(object value, CoflowEncodedValue arenaValue)
        {
            _values[^1].ApiValue = value;
            _values[^1].ArenaValue = arenaValue;
        }

        internal bool TryGet(CoflowValueId id, out ExternalValue value)
        {
            if (id.Generation == Snapshot.Generation)
            {
                for (var index = _values.Count - 1; index >= 0; index--)
                {
                    if (_values[index].Id != id) continue;
                    value = _values[index];
                    return true;
                }
            }
            value = null!;
            return false;
        }

        internal bool TryGetClosure(CoflowValueId id, out CoflowClosure closure)
        {
            if (id.Generation == Snapshot.Generation)
            {
                for (var index = _closures.Count - 1; index >= 0; index--)
                {
                    if (_closures[index].Id != id) continue;
                    closure = _closures[index].Closure;
                    return true;
                }
            }
            closure = null!;
            return false;
        }

        internal void Promote(IReadOnlyCollection<CoflowValueId> ids, CoflowCollectionArena collections)
        {
            if (ids.Count == 0) return;
            // closure 环境和集合元素都是值图节点；发布前必须求出完整传递闭包。
            var reachable = ids.ToHashSet();
            var reachableCollections = new HashSet<CoflowCollectionId>();
            var changed = true;
            while (changed)
            {
                changed = false;
                foreach (var item in _closures.Where(value => reachable.Contains(value.Id)))
                {
                    var collector = new CoflowValueIdCollector();
                    item.Closure.CollectValueIds(collector);
                    foreach (var id in collector.Ids)
                        changed |= reachable.Add(id);
                }
                foreach (var item in _values.Where(value =>
                             reachable.Contains(value.Id) && value.ArenaValue is not null))
                {
                    var collector = new CoflowValueIdCollector();
                    Snapshot.CollectArenaRowValueIds(item, collector,
                        reachableCollections, ResolveCollectionArena);
                    foreach (var id in collector.Ids)
                        changed |= reachable.Add(id);
                }
            }
            var escapedCollections = reachableCollections.Any(collections.Contains)
                ? new[] { collections.Freeze(reachableCollections) }
                : Array.Empty<CoflowCollectionArena>();
            Snapshot.Promote(
                _values.Where(value => reachable.Contains(value.Id)),
                _closures.Where(value => reachable.Contains(value.Id)),
                escapedCollections);
            Snapshot.CommitInvocationIndexes(_nextValueIndex, _nextCollectionIndex);

            CoflowCollectionArena ResolveCollectionArena(CoflowCollectionId id) =>
                collections.Contains(id) ? collections : Snapshot.CollectionArena(id);
        }
    }
}
