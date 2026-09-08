using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

/// <summary>编译期虚拟值；编号只在所属函数内有效，类型在创建后不可变。</summary>
internal readonly struct CoflowVirtualValue
{
    public int OwnerId { get; init; }
    public int Index { get; init; }
    public Type Type { get; init; }

    public CoflowVirtualValue(int OwnerId, int Index, Type Type)
    {
        this.OwnerId = OwnerId;
        this.Index = Index;
        this.Type = Type;
    }
}

internal readonly struct CoflowSourceOrigin
{
    public string SourcePath { get; init; }
    public CfdSpan? Span { get; init; }

    public CoflowSourceOrigin(string SourcePath, CfdSpan? Span)
    {
        this.SourcePath = SourcePath;
        this.Span = Span;
    }
}

/// <summary>typed CFG 操作只表达语言语义；最终 opcode 由寄存器 lowering 唯一选择。</summary>
internal abstract record CoflowVirtualOperation
{
    internal sealed record Constant(object? Value) : CoflowVirtualOperation;
    internal sealed record Move : CoflowVirtualOperation;
    internal sealed record Unary(string Operator) : CoflowVirtualOperation;
    internal sealed record Binary(string Operator) : CoflowVirtualOperation;
    internal sealed record Convert(Type SourceType, Type TargetType) : CoflowVirtualOperation;
    internal sealed record TypeTest(Type TargetType, bool UsesArenaIdentity) : CoflowVirtualOperation;
    internal sealed record MakeOptionNone : CoflowVirtualOperation;
    internal sealed record MakeOptionSome : CoflowVirtualOperation;
    internal sealed record MakeResult(bool IsOk) : CoflowVirtualOperation;
    internal sealed record ReadValueTag : CoflowVirtualOperation;
    internal sealed record ReadPayload(bool First) : CoflowVirtualOperation;
    internal sealed record MakeCollection(bool Dictionary) : CoflowVirtualOperation;
    internal sealed record CollectionIndex(bool Dictionary) : CoflowVirtualOperation;
    internal sealed record CollectionRead(CoflowVirtualCollectionReadKind Kind) : CoflowVirtualOperation;
    internal sealed record DictionaryProjection(bool Values) : CoflowVirtualOperation;
    internal sealed record CollectionBuiltin(CoflowBuiltin Builtin) : CoflowVirtualOperation;
    internal sealed record ArrayBuilder(bool Append) : CoflowVirtualOperation;
    internal sealed record FieldRead(CoflowFieldAccess Field) : CoflowVirtualOperation;
    internal sealed record Native(CoflowNativeCall Call) : CoflowVirtualOperation;
    internal sealed record BindFunction(CoflowFunctionReferenceTemplate Template) : CoflowVirtualOperation;
    internal sealed record MakeClosure(CoflowClosureProgramTemplate Template) : CoflowVirtualOperation;
    internal sealed record DirectCall(CoflowCallSite Call) : CoflowVirtualOperation;
    internal sealed record IndirectCall : CoflowVirtualOperation;
}

internal enum CoflowVirtualCollectionReadKind
{
    Count,
    ArrayItem,
    DictionaryKey,
    DictionaryValue,
}

internal sealed record CoflowVirtualInstruction(
    CoflowVirtualOperation Operation,
    CoflowVirtualValue? Target,
    CoflowVirtualValue[] Inputs,
    CoflowSourceOrigin Origin);

internal abstract record CoflowBlockTerminator(CoflowSourceOrigin Origin)
{
    internal sealed record Jump(int TargetBlock, CoflowSourceOrigin SourceOrigin) :
        CoflowBlockTerminator(SourceOrigin);

    internal sealed record Branch(
        CoflowVirtualValue Condition,
        int TrueBlock,
        int FalseBlock,
        CoflowSourceOrigin SourceOrigin) : CoflowBlockTerminator(SourceOrigin);

    internal sealed record Return(CoflowVirtualValue Value, CoflowSourceOrigin SourceOrigin) :
        CoflowBlockTerminator(SourceOrigin);

    internal sealed record DirectTailCall(
        CoflowVirtualValue[] Inputs,
        CoflowVirtualValue Result,
        CoflowCallSite Call,
        CoflowSourceOrigin SourceOrigin) : CoflowBlockTerminator(SourceOrigin);

    internal sealed record IndirectTailCall(
        CoflowVirtualValue[] Inputs,
        CoflowVirtualValue Result,
        CoflowSourceOrigin SourceOrigin) : CoflowBlockTerminator(SourceOrigin);

    internal sealed record Propagate(
        CoflowVirtualValue Source,
        CoflowVirtualValue Payload,
        CoflowVirtualValue ReturnValue,
        int ContinueBlock,
        CoflowSourceOrigin SourceOrigin) : CoflowBlockTerminator(SourceOrigin);
}

internal sealed class CoflowBasicBlock
{
    private readonly List<CoflowVirtualInstruction> _instructions = new();

    internal CoflowBasicBlock(int ownerId, int index)
    {
        OwnerId = ownerId;
        Index = index;
    }

    internal int OwnerId { get; }
    internal int Index { get; }
    internal IReadOnlyList<CoflowVirtualInstruction> Instructions => _instructions;
    internal CoflowBlockTerminator? Terminator { get; private set; }

    internal void Add(CoflowVirtualInstruction instruction)
    {
        if (Terminator is not null)
            throw new InvalidOperationException($"Basic block {Index} is already terminated.");
        _instructions.Add(instruction);
    }

    internal void Terminate(CoflowBlockTerminator terminator)
    {
        if (Terminator is not null)
            throw new InvalidOperationException($"Basic block {Index} is already terminated.");
        Terminator = terminator ?? throw new ArgumentNullException(nameof(terminator));
    }
}

internal sealed class CoflowVirtualProgram
{
    internal CoflowVirtualProgram(
        CoflowFunctionIdentity identity,
        string sourcePath,
        CfdSpan? sourceSpan,
        CoflowVirtualValue[] parameters,
        CoflowVirtualValue[] locals,
        CoflowVirtualValue[] values,
        CoflowBasicBlock[] blocks,
        Type returnType,
        IReadOnlyList<CoflowBindingDependency> bindingDependencies)
    {
        Identity = identity;
        SourcePath = sourcePath;
        SourceSpan = sourceSpan;
        Parameters = parameters;
        Locals = locals;
        Values = values;
        Blocks = blocks;
        ReturnType = returnType;
        BindingDependencies = bindingDependencies.ToArray();
    }

    internal CoflowFunctionIdentity Identity { get; }
    internal string SourcePath { get; }
    internal CfdSpan? SourceSpan { get; }
    internal CoflowVirtualValue[] Parameters { get; }
    internal CoflowVirtualValue[] Locals { get; }
    internal CoflowVirtualValue[] Values { get; }
    internal CoflowBasicBlock[] Blocks { get; }
    internal Type ReturnType { get; }
    internal CoflowBindingDependency[] BindingDependencies { get; }
}

/// <summary>构建显式虚拟值与基本块，并在写入时检查函数内所有权和控制流边界。</summary>
internal sealed class CoflowVirtualProgramBuilder
{
    private static int _nextOwnerId;
    private readonly int _ownerId = System.Threading.Interlocked.Increment(ref _nextOwnerId);
    private readonly CoflowFunctionIdentity _identity;
    private readonly string _sourcePath;
    private readonly CfdSpan? _sourceSpan;
    private readonly Type _returnType;
    private readonly List<CoflowVirtualValue> _values = new();
    private readonly List<CoflowVirtualValue> _parameters = new();
    private readonly List<CoflowVirtualValue> _localStorage = new();
    private readonly Dictionary<int, CoflowVirtualValue> _sourceLocals = new();
    private readonly List<CoflowBasicBlock> _blocks = new();
    private readonly Stack<(CoflowBasicBlock Continue, CoflowBasicBlock Break)> _loops = new();
    private readonly IReadOnlyList<CoflowBindingDependency> _bindingDependencies;

    internal CoflowVirtualProgramBuilder(
        CoflowFunctionIdentity identity,
        string sourcePath,
        CfdSpan? sourceSpan,
        IReadOnlyList<Type> parameterTypes,
        Type returnType,
        IReadOnlyList<CoflowBindingDependency>? bindingDependencies = null)
    {
        _identity = identity;
        _sourcePath = sourcePath;
        _sourceSpan = sourceSpan;
        _returnType = returnType;
        _bindingDependencies = bindingDependencies ?? Array.Empty<CoflowBindingDependency>();
        foreach (var type in parameterTypes) _parameters.Add(CreateValue(type));
        Entry = CreateBlock();
        Current = Entry;
    }

    internal CoflowBasicBlock Entry { get; }
    internal CoflowBasicBlock Current { get; private set; }
    internal bool IsCurrentTerminated => Current.Terminator is not null;
    internal IReadOnlyList<CoflowVirtualValue> Parameters => _parameters;

    internal CoflowVirtualValue CreateValue(Type type)
    {
        if (type is null) throw new ArgumentNullException(nameof(type));
        var value = new CoflowVirtualValue(_ownerId, _values.Count, type);
        _values.Add(value);
        return value;
    }

    internal CoflowVirtualValue CreateLocal(Type type)
    {
        var value = CreateValue(type);
        _localStorage.Add(value);
        return value;
    }

    internal CoflowVirtualValue Local(int sourceIndex, Type type)
    {
        if (sourceIndex < 0) throw new ArgumentOutOfRangeException(nameof(sourceIndex));
        if (_sourceLocals.TryGetValue(sourceIndex, out var value))
        {
            if (value.Type != type)
                throw new InvalidOperationException($"Local {sourceIndex} changes type from `{value.Type}` to `{type}`.");
            return value;
        }
        value = CreateValue(type);
        _sourceLocals.Add(sourceIndex, value);
        _localStorage.Add(value);
        return value;
    }

    internal CoflowBasicBlock CreateBlock()
    {
        var block = new CoflowBasicBlock(_ownerId, _blocks.Count);
        _blocks.Add(block);
        return block;
    }

    internal void Enter(CoflowBasicBlock block)
    {
        RequireOwned(block);
        Current = block;
    }

    internal void EnterLoop(CoflowBasicBlock continueBlock, CoflowBasicBlock breakBlock)
    {
        RequireOwned(continueBlock);
        RequireOwned(breakBlock);
        _loops.Push((continueBlock, breakBlock));
    }

    internal void ExitLoop()
    {
        if (_loops.Count == 0)
            throw new InvalidOperationException("No CFG loop scope is active.");
        _loops.Pop();
    }

    internal (CoflowBasicBlock Continue, CoflowBasicBlock Break) CurrentLoop() =>
        _loops.Count != 0
            ? _loops.Peek()
            : throw new InvalidOperationException("CFG loop control was emitted outside a loop.");

    internal CoflowVirtualValue Emit(
        CoflowVirtualOperation operation,
        Type resultType,
        IReadOnlyList<CoflowVirtualValue> inputs,
        CoflowSourceOrigin origin)
    {
        var target = CreateValue(resultType);
        EmitTo(operation, target, inputs, origin);
        return target;
    }

    internal void EmitTo(
        CoflowVirtualOperation operation,
        CoflowVirtualValue target,
        IReadOnlyList<CoflowVirtualValue> inputs,
        CoflowSourceOrigin origin)
    {
        if (operation is null) throw new ArgumentNullException(nameof(operation));
        RequireOwned(target);
        foreach (var input in inputs) RequireOwned(input);
        Current.Add(new CoflowVirtualInstruction(operation, target, inputs.ToArray(), origin));
    }

    internal CoflowVirtualValue Constant(Type type, object? value, CoflowSourceOrigin origin) =>
        Emit(new CoflowVirtualOperation.Constant(value), type, Array.Empty<CoflowVirtualValue>(), origin);

    internal void MoveTo(CoflowVirtualValue target, CoflowVirtualValue source, CoflowSourceOrigin origin) =>
        EmitTo(new CoflowVirtualOperation.Move(), target, new[] { source }, origin);

    internal CoflowVirtualValue Unary(
        string operation, Type type, CoflowVirtualValue operand, CoflowSourceOrigin origin) =>
        Emit(new CoflowVirtualOperation.Unary(operation), type, new[] { operand }, origin);

    internal CoflowVirtualValue Binary(
        string operation, Type type, CoflowVirtualValue left, CoflowVirtualValue right,
        CoflowSourceOrigin origin) =>
        Emit(new CoflowVirtualOperation.Binary(operation), type, new[] { left, right }, origin);

    internal CoflowVirtualValue Native(
        Type type, IReadOnlyList<CoflowVirtualValue> inputs, CoflowNativeCall call, CoflowSourceOrigin origin) =>
        Emit(new CoflowVirtualOperation.Native(call), type, inputs, origin);

    internal void Jump(CoflowBasicBlock target, CoflowSourceOrigin origin)
    {
        RequireOwned(target);
        Current.Terminate(new CoflowBlockTerminator.Jump(target.Index, origin));
    }

    internal void Branch(
        CoflowVirtualValue condition,
        CoflowBasicBlock whenTrue,
        CoflowBasicBlock whenFalse,
        CoflowSourceOrigin origin)
    {
        RequireOwned(condition);
        RequireOwned(whenTrue);
        RequireOwned(whenFalse);
        if (condition.Type != typeof(bool))
            throw new InvalidOperationException("A CFG branch condition must be Boolean.");
        Current.Terminate(new CoflowBlockTerminator.Branch(condition, whenTrue.Index, whenFalse.Index, origin));
    }

    internal void Return(CoflowVirtualValue value, CoflowSourceOrigin origin)
    {
        RequireOwned(value);
        if (value.Type != _returnType)
            throw new InvalidOperationException($"CFG return type `{value.Type}` does not match `{_returnType}`.");
        Current.Terminate(new CoflowBlockTerminator.Return(value, origin));
    }

    internal void DirectTailCall(
        IReadOnlyList<CoflowVirtualValue> inputs,
        CoflowCallSite call,
        CoflowSourceOrigin origin)
    {
        foreach (var input in inputs) RequireOwned(input);
        var result = CreateValue(_returnType);
        Current.Terminate(new CoflowBlockTerminator.DirectTailCall(
            inputs.ToArray(), result, call, origin));
    }

    internal void IndirectTailCall(
        IReadOnlyList<CoflowVirtualValue> inputs,
        CoflowSourceOrigin origin)
    {
        foreach (var input in inputs) RequireOwned(input);
        var result = CreateValue(_returnType);
        Current.Terminate(new CoflowBlockTerminator.IndirectTailCall(
            inputs.ToArray(), result, origin));
    }

    internal CoflowVirtualValue Propagate(
        CoflowVirtualValue source,
        Type payloadType,
        CoflowBasicBlock continueBlock,
        CoflowSourceOrigin origin)
    {
        RequireOwned(source);
        RequireOwned(continueBlock);
        var payload = CreateValue(payloadType);
        var returnValue = CreateValue(_returnType);
        Current.Terminate(new CoflowBlockTerminator.Propagate(
            source, payload, returnValue, continueBlock.Index, origin));
        return payload;
    }

    internal CoflowVirtualProgram Build()
    {
        if (_loops.Count != 0)
            throw new InvalidOperationException("CFG loop scopes are unbalanced.");
        var blocks = ReachableBlocks();
        return new CoflowVirtualProgram(
            _identity, _sourcePath, _sourceSpan, _parameters.ToArray(),
            _localStorage.ToArray(), _values.ToArray(),
            blocks, _returnType, _bindingDependencies);
    }

    private CoflowBasicBlock[] ReachableBlocks()
    {
        var reachable = new HashSet<int>();
        var pending = new Queue<int>();
        pending.Enqueue(Entry.Index);
        while (pending.Count != 0)
        {
            var index = pending.Dequeue();
            if (!reachable.Add(index)) continue;
            var terminator = _blocks[index].Terminator ??
                throw new InvalidOperationException($"Reachable CFG basic block {index} must have a terminator.");
            switch (terminator)
            {
                case CoflowBlockTerminator.Jump jump:
                    pending.Enqueue(jump.TargetBlock);
                    break;
                case CoflowBlockTerminator.Branch branch:
                    pending.Enqueue(branch.TrueBlock);
                    pending.Enqueue(branch.FalseBlock);
                    break;
                case CoflowBlockTerminator.Propagate propagate:
                    pending.Enqueue(propagate.ContinueBlock);
                    break;
            }
        }

        var sourceBlocks = _blocks.Where(block => reachable.Contains(block.Index)).ToArray();
        var remap = sourceBlocks.Select((block, index) => (block.Index, index))
            .ToDictionary(pair => pair.Index, pair => pair.index);
        var result = sourceBlocks.Select((_, index) => new CoflowBasicBlock(_ownerId, index)).ToArray();
        for (var index = 0; index < sourceBlocks.Length; index++)
        {
            foreach (var instruction in sourceBlocks[index].Instructions) result[index].Add(instruction);
            result[index].Terminate(Remap(sourceBlocks[index].Terminator!, remap));
        }
        return result;
    }

    // 块编号只属于当前 CFG；删除不可达块后必须一次性重写全部控制流目标。
    private static CoflowBlockTerminator Remap(
        CoflowBlockTerminator terminator,
        IReadOnlyDictionary<int, int> blocks) => terminator switch
    {
        CoflowBlockTerminator.Jump jump => jump with { TargetBlock = blocks[jump.TargetBlock] },
        CoflowBlockTerminator.Branch branch => branch with
        {
            TrueBlock = blocks[branch.TrueBlock],
            FalseBlock = blocks[branch.FalseBlock],
        },
        CoflowBlockTerminator.Propagate propagate => propagate with
        {
            ContinueBlock = blocks[propagate.ContinueBlock],
        },
        _ => terminator,
    };

    private void RequireOwned(CoflowVirtualValue value)
    {
        if (value.OwnerId != _ownerId || (uint)value.Index >= (uint)_values.Count || !_values[value.Index].Equals(value))
            throw new InvalidOperationException("A virtual value belongs to another function.");
    }

    private void RequireOwned(CoflowBasicBlock block)
    {
        if (block.OwnerId != _ownerId || (uint)block.Index >= (uint)_blocks.Count || !ReferenceEquals(_blocks[block.Index], block))
            throw new InvalidOperationException("A basic block belongs to another function.");
    }
}
}
