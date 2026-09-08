using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

using global::Coflow.Runtime;

internal static class CoflowInvocationContext
{
    [ThreadStatic]
    private static InvocationState? _state;

    [ThreadStatic]
    private static InvocationState? _cached;

    [ThreadStatic]
    private static int _depth;

    [ThreadStatic]
    private static CoflowSchemaRuntimeContext.Scope _runtimeScope;

    internal static Scope Enter(global::Coflow.Runtime.Coflow owner, CoflowSnapshot snapshot)
    {
        if (_depth != 0 && !ReferenceEquals(_state!.Snapshot, snapshot))
            throw new InvalidOperationException("A Coflow invocation cannot switch snapshots during reentrancy.");
        if (_depth == 0)
        {
            _runtimeScope = CoflowSchemaRuntimeContext.Enter(snapshot.Runtime);
            _state = _cached ?? new InvocationState();
            _cached = null;
            _state.Reset(owner, snapshot);
        }
        else if (!ReferenceEquals(_state!.Owner, owner))
            throw new InvalidOperationException("A Coflow invocation cannot switch owners during reentrancy.");
        _depth++;
        return new Scope();
    }

    internal static ExecutionEnvironment CurrentExecution =>
        (_state ?? throw new InvalidOperationException("A VM context was started outside a Coflow invocation."))
        .Environment;

    internal static bool TryGetLayout(Type type, out CoflowValueShape layout)
    {
        if (_state is not null) return _state.Snapshot.Layouts.TryGet(type, out layout);
        layout = null!;
        return false;
    }

    internal static bool TryGetRuntime(out CoflowSchemaRuntime runtime)
    {
        if (_state is not null)
        {
            runtime = _state.Snapshot.Runtime;
            return true;
        }
        runtime = null!;
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
                _runtimeScope.Dispose();
                _runtimeScope = default;
            }
        }
    }

    /// <summary>VM 一次性捕获的显式运行环境，避免 dispatch 依赖线程静态查询。</summary>
    internal sealed class ExecutionEnvironment
    {
        private readonly InvocationState _state;
        private readonly Func<uint> _allocateCollectionIndex;
        private readonly Func<CoflowCollectionId, CoflowCollectionArena> _collectionArena;

        internal ExecutionEnvironment(InvocationState state)
        {
            _state = state;
            _allocateCollectionIndex = state.Imports.AllocateCollectionIndex;
            _collectionArena = id => state.Snapshot.CollectionArena(id);
        }

        internal CoflowSnapshot Snapshot => _state.Snapshot;
        internal CoflowImportState TransientValues => _state.Imports;
        internal global::Coflow.Runtime.Coflow Owner => _state.Owner;
        internal CoflowExecutionBudget Budget => _state.Budget;
        internal uint SnapshotId => Snapshot.SnapshotId;
        internal int PublishedCollectionCount => Snapshot.CollectionCount;
        internal Func<uint> CollectionIndexAllocator => _allocateCollectionIndex;
        internal Func<CoflowCollectionId, CoflowCollectionArena> PublishedCollectionResolver => _collectionArena;
        internal CoflowLinkedFunction LinkedFunction(int programIndex) => Snapshot.LinkedFunction(programIndex);
        internal CoflowFunctionTarget Function(CoflowFunctionId functionId, CoflowValueId environmentId) =>
            Snapshot.Function(functionId, environmentId, TransientValues);
        internal CoflowRawFunctionHandle FunctionHandle(
            CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId)
        {
            var function = Snapshot.Function(id, typeId, fieldId, TransientValues);
            var kind = function.Entry.CompiledProgram is null
                ? CoflowFunctionKind.Native
                : CoflowFunctionKind.Program;
            return new CoflowRawFunctionHandle(
                new CoflowFunctionId(SnapshotId, kind, function.Entry.TargetIndex), id);
        }
        internal bool IsType(CoflowValueId id, Type expectedType) => Snapshot.IsType(id, expectedType, TransientValues);
        internal long ReadArenaInteger(CoflowValueId id, int offset) => Snapshot.ReadArenaInteger(id, offset, TransientValues);
        internal double ReadArenaFloat(CoflowValueId id, int offset) => Snapshot.ReadArenaFloat(id, offset, TransientValues);
        internal object? ReadArenaReference(CoflowValueId id, int offset) => Snapshot.ReadArenaReference(id, offset, TransientValues);
        internal void CopyArenaField(CoflowValueId id, CoflowFieldAccess access,
            CoflowExecutionSession session, CoflowValueRegister target) =>
            Snapshot.CopyArenaField(id, access, session, target, TransientValues);
        internal CoflowValueId AttachClosure(CoflowClosure closure) => _state.Imports.AttachClosure(closure);

        internal T PromoteResult<T>(T value, CoflowCollectionArena collections)
        {
            return _state.Imports.PromoteResult(value, collections);
        }
    }

    internal sealed class InvocationState
    {
        internal InvocationState()
        {
            Budget = new CoflowExecutionBudget(global::Coflow.Runtime.CoflowOptions.Default);
            Imports = new CoflowImportState();
            Environment = new ExecutionEnvironment(this);
        }
        internal CoflowSnapshot Snapshot { get; private set; } = null!;
        internal global::Coflow.Runtime.Coflow Owner { get; private set; } = null!;
        internal CoflowImportState Imports { get; }
        internal CoflowExecutionBudget Budget { get; }
        internal ExecutionEnvironment Environment { get; }

        internal void Reset(global::Coflow.Runtime.Coflow owner, CoflowSnapshot snapshot)
        {
            Owner = owner;
            Snapshot = snapshot;
            Budget.Reset(owner.Options);
            Imports.Start(snapshot, Budget);
        }

        internal void Clear()
        {
            // 线程缓存归还前必须释放调用期 Arena 持有的全部应用对象。
            // 临时 ID 也不能跨调用复用，否则 Host 留存的旧值可能错误别名到下一次调用。
            Imports.Clear();
            Owner = null!;
            Snapshot = null!;
            Budget.Reset(global::Coflow.Runtime.CoflowOptions.Default);
        }
    }
}
}
