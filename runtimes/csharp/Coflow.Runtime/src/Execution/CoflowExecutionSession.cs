using System.Threading.Tasks;
using System.Threading;
using System.IO;
using System;
using System.Collections.Generic;
using System.Linq;

namespace Coflow.Runtime.CompilerServices
{

internal sealed class CoflowExecutionSession : IDisposable
{
    private readonly CoflowFrameStack _frames = new();
    private readonly CoflowRegisterStorage _registers = new();

    private readonly CoflowValueResolver _values = new();
    private readonly CoflowExecutionBudgetLease _budgetLease = new();
    private CoflowInvocationContext.ExecutionEnvironment? _environment;
    // 独立执行也在创建时捕获 Schema；codec 热路径不探测线程上下文。
    internal CoflowSchemaRuntime? Runtime { get; private set; } =
        CoflowSchemaRuntimeContext.TryGet(out var runtime) ? runtime : null;

    internal CoflowRegisterStorage Registers => _registers;

    internal CoflowExecutionSession? NextPooled { get; set; }

    internal int IntegerBase => _registers.IntegerBase;

    internal int FloatBase => _registers.FloatBase;

    internal int ReferenceBase => _registers.ReferenceBase;

    internal CoflowCollectionArena Collections => _values.Current;

    internal CoflowExecutionBudget Budget => _budgetLease.Budget;

    internal T ImportBoundary<T>(T value) =>
        Environment.TransientValues.Import(value, _values.Current);

    internal object ApiValue(CoflowValueId id, Type expectedType) =>
        Environment.Snapshot.ApiValue(id, expectedType, Environment.TransientValues);

    internal CoflowInvocationContext.ExecutionEnvironment Environment => _environment ??
        throw new InvalidOperationException("This standalone Coflow execution has no snapshot environment.");

    internal CoflowProgram Program { get; private set; } = null!;

    internal int Pc { get; set; }

    internal IEnumerable<CoflowFunctionIdentity> CallStack => _frames.CallStack(Program);

    internal void Reset()
    {
        _frames.Reset();
        _registers.Reset();
        Pc = 0;
        Program = null!;
        _environment = null;
        Runtime = null;
    }

    internal void Start<TArguments>(CoflowProgram program, TArguments arguments,
        uint? standaloneGeneration = null, CoflowClosure? closure = null) where TArguments : struct, ICoflowArgumentPack
    {
        Program = program;
        _environment = standaloneGeneration.HasValue ? null : CoflowInvocationContext.CurrentExecution;
        Runtime = _environment?.Snapshot.Runtime ??
            (CoflowSchemaRuntimeContext.TryGet(out var runtime) ? runtime : null);
        var budget = _environment?.Budget ?? CoflowExecutionBudget.CreateUnbounded();
        _budgetLease.Start(budget);
        uint generation = standaloneGeneration ?? _environment!.SnapshotId;
        uint firstIndex = _environment is null ? 0u : checked((uint)_environment.PublishedCollectionCount);
        IReadOnlyList<CoflowCollectionArena>? captured = closure?.Collections;
        if (captured is not null)
        {
            foreach (var arena in captured)
                firstIndex = Math.Max(firstIndex, arena.LastIndex);
        }
        // 正常执行由快照分配全局唯一索引；独立程序仍使用 Arena 内的连续索引。
        _values.Start(generation, firstIndex,
            _environment?.CollectionIndexAllocator,
            _environment is null ? null : budget,
            captured,
            _environment?.PublishedCollectionResolver);
        _registers.Reserve(program.RegisterProgram, _budgetLease);
        arguments.Write(this);
    }

    internal CoflowValueRegister Parameter(int index)
    {
        return OffsetRelative(Program.RegisterProgram.Parameters[index]);
    }

    internal CoflowValueRegister OffsetRelative(CoflowValueRegister register)
    {
        return _registers.Offset(register);
    }

    internal void WriteBooleanRelative(int index, bool value)
    {
        _registers.WriteIntegerRelative(index, value ? 1 : 0);
    }

    internal void Write<T>(CoflowValueRegister register, T value)
    {
        CoflowBoundaryCodec<T>.WriteImported(this, register, value);
    }

    internal void Copy(CoflowValueRegister source, CoflowValueRegister target)
    {
        _registers.Copy(source, target);
    }

    internal void CopyRelative(CoflowValueRegister source, CoflowValueRegister target)
    {
        _registers.CopyRelative(source, target);
    }

    internal void WriteEncodedRelative(CoflowEncodedValue source, CoflowValueRegister target)
    {
        _registers.WriteEncodedRelative(source, target);
    }

    internal bool Call(CoflowRegisterCallSite site, CoflowProgram? target, bool tail)
    {
        PrepareCallArguments(site);
        if (target == null)
        {
            return false;
        }
        if (tail && target == Program)
        {
            for (int i = 0; i < site.Arguments.Length; i++)
            {
                CopyRelative(site.Arguments[i], Program.RegisterProgram.Parameters[i]);
            }
            Pc = 0;
            return true;
        }
        EnterDirect(target, site, tail, OffsetRelative(site.Result));
        return true;
    }

    private void PrepareCallArguments(CoflowRegisterCallSite site)
    {
        for (int i = 0; i < site.Arguments.Length; i++)
        {
            if (site.CopyArguments[i])
            {
                CopyRelative(site.SourceArguments[i], site.Arguments[i]);
            }
        }
    }

    private void EnterDirect(CoflowProgram target, CoflowRegisterCallSite site, bool tail, CoflowValueRegister returnTarget)
    {
        CoflowRegisterWindow previous = _registers.Window;
        if (!tail)
        {
            PushFrame(returnTarget);
        }
        _registers.EnterDirect(site, previous);
        Program = target;
        Pc = 0;
        _registers.Reserve(target.RegisterProgram, _budgetLease);
        if (tail)
        {
            _registers.CompactTailWindow(target.RegisterProgram, previous);
        }
    }

    private void EnterFromRegisters(CoflowProgram target, IReadOnlyList<CoflowValueRegister> arguments, bool tail, CoflowValueRegister returnTarget)
    {
        CoflowRegisterWindow previous = _registers.Window;
        if (!tail)
        {
            PushFrame(returnTarget);
        }
        _registers.EnterAfter(previous);
        Program = target;
        Pc = 0;
        _registers.Reserve(target.RegisterProgram, _budgetLease);
        for (int i = 0; i < arguments.Count; i++)
        {
            CoflowValueRegister source = CoflowRegisterStorage.Absolute(arguments[i], previous);
            Copy(source, Parameter(i));
        }
        if (tail)
        {
            _registers.CompactTailWindow(target.RegisterProgram, previous);
        }
    }

    internal bool CallIndirect<TResult>(CoflowRegisterIndirectCallSite site, bool tail, out TResult returned)
    {
        returned = default!;
        var functionId = CoflowFunctionId.FromPacked(unchecked((ulong)
            Registers.ReadIntegerRelative(site.Callable.IntegerBase)));
        var environmentId = CoflowValueId.FromPacked(unchecked((ulong)
            Registers.ReadIntegerRelative(site.Callable.IntegerBase + 1)));
        var callable = Environment.Function(functionId, environmentId);
        if (callable.Closure is { } closure)
        {
            EnterClosureFromRegisters(closure, site.Arguments, tail, OffsetRelative(site.Result));
            return false;
        }
        var functionEntry = callable.Entry!;
        CoflowProgram? compiledProgram = functionEntry.CompiledProgram;
        if (compiledProgram != null)
        {
            EnterBoundFromRegisters(compiledProgram, callable.Receiver!, site.Arguments, tail, OffsetRelative(site.Result));
            return false;
        }
        Budget.HostCall(CoflowVm.BoundaryLanes(site.Arguments, site.Result));
        functionEntry.InvokeBoundFromVm(new CoflowNativeFrame(this, site.Arguments, site.Result, site.ResultType));
        return tail && ReturnRegister<TResult>(site.Result, out returned);
    }

    internal T DecodeEncoded<T>(CoflowEncodedValue source)
    {
        return _registers.DecodeEncoded<T>(this, source);
    }

    internal CoflowCollectionKind CollectionKind(CoflowCollectionId id)
    {
        return _values.Kind(id);
    }

    internal int CollectionItemCount(CoflowCollectionId id)
    {
        return _values.ItemCount(id);
    }

    internal CoflowEncodedValue ReadArrayItem(CoflowCollectionId id, int index)
    {
        return _values.ReadArrayItem(id, index);
    }

    internal CoflowEncodedValue ReadDictionaryKey(CoflowCollectionId id, int index)
    {
        return _values.ReadDictionaryKey(id, index);
    }

    internal CoflowEncodedValue ReadDictionaryValue(CoflowCollectionId id, int index)
    {
        return _values.ReadDictionaryValue(id, index);
    }

    internal void MakeCollection(CoflowRegisterCollectionSite site, bool dictionary)
    {
        CoflowCollectionId collectionId;
        if (dictionary)
        {
            IReadOnlyList<CoflowValueRegister> values = site.Second ?? throw new InvalidOperationException("A dictionary site has no value registers.");
            Type[] genericArguments = site.Target.Shape.Type.GetGenericArguments();
            CoflowValueShape keyShape = ((site.First.Length == 0) ? CoflowValueShape.Of(genericArguments[0]) : site.First[0].Shape);
            CoflowValueShape valueShape = values.Count == 0 ? CoflowValueShape.Of(genericArguments[1]) : values[0].Shape;
            collectionId = _values.Current.AddDictionary(keyShape, valueShape, this, site.First, values);
        }
        else
        {
            CoflowValueShape elementShape = ((site.First.Length == 0) ? CoflowValueShape.Of(site.Target.Shape.Type.GetGenericArguments()[0]) : site.First[0].Shape);
            collectionId = _values.Current.AddArray(elementShape, this, site.First);
        }
        Registers.WriteIntegerRelative(site.Target.IntegerBase, (long)collectionId.Packed);
    }

    internal void ArrayIndex(CoflowRegisterArrayIndexSite site)
    {
        CoflowCollectionId id = CoflowCollectionId.FromPacked((ulong)Registers.ReadIntegerRelative(site.Collection.IntegerBase));
        long index = Registers.ReadIntegerRelative(site.Index.IntegerBase);
        if (index < 0 || index >= CollectionItemCount(id))
        {
            Registers.WriteIntegerRelative(site.Target.IntegerBase, 0L);
            return;
        }
        Registers.WriteIntegerRelative(site.Target.IntegerBase, 1L);
        CoflowValueRegister target = OffsetRelative(site.Target.First);
        checked
        {
            _values.Resolve(id).CopyArrayItem(id, (int)index, this, target);
        }
    }

    internal void DictionaryIndex(CoflowRegisterArrayIndexSite site)
    {
        CoflowCollectionId id = CoflowCollectionId.FromPacked((ulong)Registers.ReadIntegerRelative(site.Collection.IntegerBase));
        int index = _values.Resolve(id).FindDictionaryKey(id, this, site.Index);
        if (index < 0)
        {
            Registers.WriteIntegerRelative(site.Target.IntegerBase, 0L);
            return;
        }
        Registers.WriteIntegerRelative(site.Target.IntegerBase, 1L);
        CoflowValueRegister target = OffsetRelative(site.Target.First);
        _values.Resolve(id).CopyDictionaryValue(id, index, this, target);
    }

    internal void ReadCollection(CoflowRegisterCollectionReadSite site, CoflowRegisterOpCode code)
    {
        CoflowCollectionId id = CoflowCollectionId.FromPacked((ulong)Registers.ReadIntegerRelative(site.Collection.IntegerBase));
        if (code == CoflowRegisterOpCode.CollectionCount)
        {
            Registers.WriteIntegerRelative(site.Target.IntegerBase, CollectionItemCount(id));
            return;
        }
        int index = checked((int)Registers.ReadIntegerRelative((site.Index ?? throw new InvalidOperationException("A collection item read has no index register.")).IntegerBase));
        CoflowValueRegister target = OffsetRelative(site.Target);
        var arena = _values.Resolve(id);
        switch (code)
        {
            case CoflowRegisterOpCode.ArrayItem:
                arena.CopyArrayItem(id, index, this, target);
                break;
            case CoflowRegisterOpCode.DictionaryKey:
                arena.CopyDictionaryKey(id, index, this, target);
                break;
            default:
                arena.CopyDictionaryValue(id, index, this, target);
                break;
        }
    }

    internal void ProjectDictionary(CoflowRegisterCollectionProjectionSite site, bool values)
    {
        CoflowCollectionId sourceId = CoflowCollectionId.FromPacked((ulong)Registers.ReadIntegerRelative(site.Source.IntegerBase));
        CoflowCollectionId projectionId = _values.Current.AddDictionaryProjection(
            CollectionArena(sourceId), sourceId, values);
        Registers.WriteIntegerRelative(site.Target.IntegerBase, (long)projectionId.Packed);
    }

    internal void ExecuteCollectionBuiltin(CoflowRegisterCollectionBuiltinSite site)
    {
        CoflowBuiltinKind kind = site.Builtin.Kind;
        CoflowCollectionId collectionId = CollectionId(site.Receiver);
        CoflowCollectionArena arena = CollectionArena(collectionId);
        int count = CollectionItemCount(collectionId);
        Budget.CollectionWork(kind == CoflowBuiltinKind.CollectionUnique
            ? checked((long)count * Math.Max(0, count - 1) / 2)
            : count);
        bool second = kind == CoflowBuiltinKind.DictionaryContainsValue;
        if (kind - 4 <= CoflowBuiltinKind.DictionaryKeys)
        {
            bool value = false;
            for (int index = 0; index < count; index++)
            {
                if (arena.ValueEqualsRegister(collectionId, index, second, this, site.Argument!.Value))
                {
                    value = true;
                    break;
                }
            }
            WriteBooleanRelative(site.Target.IntegerBase, value);
            return;
        }
        if (kind == CoflowBuiltinKind.CollectionUnique)
        {
            for (int left = 0; left < count; left++)
            {
                for (int right = left + 1; right < count; right++)
                {
                    if (arena.ValueEquals(collectionId, left, second: false,
                            arena, collectionId, right, otherSecond: false))
                    {
                        WriteBooleanRelative(site.Target.IntegerBase, value: false);
                        return;
                    }
                }
            }
            WriteBooleanRelative(site.Target.IntegerBase, value: true);
            return;
        }
        if (kind is CoflowBuiltinKind.CollectionMin or CoflowBuiltinKind.CollectionMax)
        {
            if (count == 0)
                throw new InvalidOperationException("aggregate requires a non-empty array");
            int selected = 0;
            for (int index = 1; index < count; index++)
            {
                int comparison = arena.Compare(collectionId, index, selected);
                if (kind == CoflowBuiltinKind.CollectionMin ? comparison < 0 : comparison > 0)
                    selected = index;
            }
            arena.CopyArrayItem(collectionId, selected, this, OffsetRelative(site.Target));
            return;
        }
        switch (kind)
        {
            case CoflowBuiltinKind.CollectionSumInteger:
                {
                    long sum = 0L;
                    for (int index = 0; index < count; index++)
                        sum = checked(sum + arena.ReadInteger(collectionId, index));
                    Registers.WriteIntegerRelative(site.Target.IntegerBase, sum);
                    return;
                }
            case CoflowBuiltinKind.CollectionSumFloat:
                {
                    double sum = 0.0;
                    for (int index = 0; index < count; index++)
                        sum += arena.ReadFloat(collectionId, index);
                    Registers.WriteFloatRelative(site.Target.FloatBase, sum);
                    return;
                }
            case CoflowBuiltinKind.CollectionSorted:
            case CoflowBuiltinKind.CollectionStrictlySorted:
                bool strict = kind == CoflowBuiltinKind.CollectionStrictlySorted;
                for (int index = 1; index < count; index++)
                {
                    int comparison = arena.Compare(collectionId, index - 1, index);
                    if (strict ? comparison >= 0 : comparison > 0)
                    {
                        WriteBooleanRelative(site.Target.IntegerBase, value: false);
                        return;
                    }
                }
                WriteBooleanRelative(site.Target.IntegerBase, value: true);
                return;
            default:
                break;
        }
        CoflowCollectionId argumentId = CollectionId(site.Argument!.Value);
        var result = kind switch
        {
            CoflowBuiltinKind.CollectionIntersects => Overlaps(collectionId, argumentId),
            CoflowBuiltinKind.CollectionDisjoint => !Overlaps(collectionId, argumentId),
            CoflowBuiltinKind.CollectionSubset => IsSubset(collectionId, argumentId),
            CoflowBuiltinKind.CollectionSuperset => IsSubset(argumentId, collectionId),
            _ => throw new InvalidOperationException($"Unsupported collection builtin `{kind}`."),
        };
        WriteBooleanRelative(site.Target.IntegerBase, result);
    }

    private CoflowCollectionId CollectionId(CoflowValueRegister register)
    {
        return CoflowCollectionId.FromPacked((ulong)Registers.ReadIntegerRelative(register.IntegerBase));
    }

    internal CoflowCollectionArena CollectionArena(CoflowCollectionId id)
    {
        return _values.Resolve(id);
    }

    private bool Overlaps(CoflowCollectionId left, CoflowCollectionId right)
    {
        CoflowCollectionArena leftArena = CollectionArena(left);
        CoflowCollectionArena rightArena = CollectionArena(right);
        for (int leftIndex = 0; leftIndex < CollectionItemCount(left); leftIndex++)
        {
            for (int rightIndex = 0; rightIndex < CollectionItemCount(right); rightIndex++)
            {
                if (leftArena.ValueEquals(left, leftIndex, second: false,
                        rightArena, right, rightIndex, otherSecond: false))
                {
                    return true;
                }
            }
        }
        return false;
    }

    private bool IsSubset(CoflowCollectionId left, CoflowCollectionId right)
    {
        CoflowCollectionArena leftArena = CollectionArena(left);
        CoflowCollectionArena rightArena = CollectionArena(right);
        for (int leftIndex = 0; leftIndex < CollectionItemCount(left); leftIndex++)
        {
            bool found = false;
            for (int rightIndex = 0; rightIndex < CollectionItemCount(right); rightIndex++)
            {
                if (leftArena.ValueEquals(left, leftIndex, second: false,
                        rightArena, right, rightIndex, otherSecond: false))
                {
                    found = true;
                    break;
                }
            }
            if (!found)
            {
                return false;
            }
        }
        return true;
    }

    internal void ArrayBuilder(CoflowRegisterArrayBuilderSite site, bool append)
    {
        if (!append)
        {
            int capacity = checked((int)Registers.ReadIntegerRelative(site.CollectionOrCapacity.IntegerBase));
            CoflowValueShape elementShape = CoflowValueShape.Of(site.Target.Shape.Type.GetGenericArguments()[0]);
            CoflowCollectionId collectionId = _values.Current.BeginArray(elementShape, capacity);
            Registers.WriteIntegerRelative(site.Target.IntegerBase, (long)collectionId.Packed);
            return;
        }
        CoflowValueRegister source = site.Item ?? throw new InvalidOperationException("An array append site has no item register.");
        CoflowCollectionId id = CoflowCollectionId.FromPacked((ulong)Registers.ReadIntegerRelative(site.CollectionOrCapacity.IntegerBase));
        if (!_values.Current.Contains(id))
        {
            throw new InvalidOperationException("Only an invocation array builder can be appended.");
        }
        _values.Current.AppendArray(id, this, source);
    }

    private static void RequireSamePhysicalLayout(CoflowValueShape actual, CoflowValueShape expected)
    {
        if (actual.IntegerCount != expected.IntegerCount || actual.FloatCount != expected.FloatCount || actual.ReferenceCount != expected.ReferenceCount)
        {
            throw new InvalidOperationException($"encoded value layout mismatch: `{actual.Type}` to `{expected.Type}`");
        }
    }

    internal void WriteBoxed(CoflowValueRegister target, object value)
    {
        if (target.Shape.Kind == CoflowValueShapeKind.Scalar && target.Shape.ScalarKind == CoflowRegisterKind.Reference)
        {
            Registers.WriteReference(new CoflowRegister(CoflowRegisterKind.Reference, target.ReferenceBase), value);
            return;
        }
        if (target.Shape.Kind == CoflowValueShapeKind.Struct &&
            CoflowSchemaRuntimeContext.TryGetStructCodec(target.Shape.Type, out CoflowStructDescriptor structDescriptor))
        {
            structDescriptor.WriteObject(this, target, value);
            return;
        }
        if (target.Shape.Kind == CoflowValueShapeKind.Record &&
            CoflowSchemaRuntimeContext.TryGetTypeCodec(target.Shape.Type, out CoflowTypeDescriptor typeDescriptor))
        {
            Registers.WriteInteger(new CoflowRegister(CoflowRegisterKind.Integer, target.IntegerBase),
                (long)typeDescriptor.GetValueIdObject(value).Packed);
            return;
        }
        throw new InvalidOperationException($"Unsupported boxed receiver type `{target.Shape.Type}`.");
    }

    private void EnterBoundFromRegisters(CoflowProgram target, object receiver, IReadOnlyList<CoflowValueRegister> arguments, bool tail, CoflowValueRegister returnTarget)
    {
        CoflowRegisterWindow previous = _registers.Window;
        if (!tail)
        {
            PushFrame(returnTarget);
        }
        _registers.EnterAfter(previous);
        Program = target;
        Pc = 0;
        _registers.Reserve(target.RegisterProgram, _budgetLease);
        for (int index = 0; index < arguments.Count; index++)
        {
            CoflowValueRegister source = CoflowRegisterStorage.Absolute(arguments[index], previous);
            Copy(source, Parameter(index));
        }
        WriteBoxed(Parameter(arguments.Count), receiver);
        if (tail)
        {
            _registers.CompactTailWindow(target.RegisterProgram, previous);
        }
    }

    private void EnterClosureFromRegisters(CoflowClosure closure, IReadOnlyList<CoflowValueRegister> arguments, bool tail, CoflowValueRegister returnTarget)
    {
        _values.AddCaptured(closure.Collections);
        CoflowRegisterWindow previous = _registers.Window;
        if (!tail)
        {
            PushFrame(returnTarget);
        }
        _registers.EnterAfter(previous);
        Program = closure.Program;
        Pc = 0;
        _registers.Reserve(closure.Program.RegisterProgram, _budgetLease);
        for (int index = 0; index < arguments.Count; index++)
        {
            CoflowValueRegister source = CoflowRegisterStorage.Absolute(arguments[index], previous);
            Copy(source, Parameter(index));
        }
        WriteCaptures(closure, arguments.Count);
        if (tail)
        {
            _registers.CompactTailWindow(closure.Program.RegisterProgram, previous);
        }
    }

    internal bool ReturnRegister<TResult>(CoflowValueRegister source, out TResult root)
    {
        if (_frames.Count == 0)
        {
            root = CoflowBoundaryCodec<TResult>.ReadRelative(this, source);
            if (_environment is not null)
                root = _environment.PromoteResult(root, _values.Current);
            return true;
        }
        CoflowFrame frame = _frames.Pop();
        Copy(OffsetRelative(source), frame.ReturnTarget);
        _registers.ClearCurrentReferences();
        _budgetLease.ExitFrame();
        Program = frame.Program;
        Pc = frame.ReturnPc;
        _registers.Restore(frame, Program.RegisterProgram);
        root = default!;
        return false;
    }

    internal void MakeClosure(CoflowRegisterClosureSite site)
    {
        CoflowClosureTemplate template = site.Template;
        Budget.ClosureLanes(checked(template.IntegerCount + template.FloatCount + template.ReferenceCount));
        CoflowClosure coflowClosure = CoflowClosure.Create(Environment.Owner, template.Program, template.Captures,
            _values.FreezeClosureCollections(), template.IntegerCount, template.FloatCount, template.ReferenceCount);
        for (int index = 0; index < template.CaptureCount; index++)
        {
            Capture(OffsetRelative(site.Captures[index]), template.Captures[index], coflowClosure);
        }
        coflowClosure.RetainReachableCollections();
        var environmentId = Environment.AttachClosure(coflowClosure);
        Registers.WriteIntegerRelative(site.Target.IntegerBase,
            new CoflowFunctionId(Environment.SnapshotId,
                CoflowFunctionKind.Closure, template.TargetIndex).Packed);
        Registers.WriteIntegerRelative(site.Target.IntegerBase + 1, unchecked((long)environmentId.Packed));
    }

    private void Capture(CoflowValueRegister source, CoflowCaptureLayout target, CoflowClosure closure)
    {
        if (source.Shape.Kind == CoflowValueShapeKind.Unit)
        {
            return;
        }
        if (source.Shape.Kind is CoflowValueShapeKind.Scalar or
            CoflowValueShapeKind.Collection or CoflowValueShapeKind.Record)
        {
            switch (source.Shape.ScalarKind)
            {
                case CoflowRegisterKind.Integer:
                    closure.SetInteger(target.IntegerBase, Registers.ReadInteger(source.Scalar));
                    break;
                case CoflowRegisterKind.Float:
                    closure.SetFloat(target.FloatBase, Registers.ReadFloat(source.Scalar));
                    break;
                default:
                    closure.SetReference(target.ReferenceBase, Registers.ReadReference(source.Scalar));
                    break;
            }
        }
        else if (source.Shape.Kind is CoflowValueShapeKind.Struct or CoflowValueShapeKind.Function)
        {
            for (int lane = 0; lane < source.Shape.IntegerCount; lane++)
            {
                closure.SetInteger(target.IntegerBase + lane,
                    Registers.ReadInteger(new CoflowRegister(CoflowRegisterKind.Integer, source.IntegerBase + lane)));
            }
            for (int lane = 0; lane < source.Shape.FloatCount; lane++)
            {
                closure.SetFloat(target.FloatBase + lane,
                    Registers.ReadFloat(new CoflowRegister(CoflowRegisterKind.Float, source.FloatBase + lane)));
            }
            for (int lane = 0; lane < source.Shape.ReferenceCount; lane++)
            {
                closure.SetReference(target.ReferenceBase + lane,
                    Registers.ReadReference(new CoflowRegister(CoflowRegisterKind.Reference, source.ReferenceBase + lane)));
            }
        }
        else
        {
            closure.SetInteger(target.IntegerBase, Registers.ReadInteger(source.Tag));
            Capture(source.First, new CoflowCaptureLayout(source.Shape.First!, target.IntegerBase + 1, target.FloatBase, target.ReferenceBase), closure);
            if (source.Shape.Kind == CoflowValueShapeKind.Result)
            {
                Capture(source.Second, new CoflowCaptureLayout(source.Shape.Second!, target.IntegerBase + 1 + source.Shape.First!.IntegerCount, target.FloatBase + source.Shape.First.FloatCount, target.ReferenceBase + source.Shape.First.ReferenceCount), closure);
            }
        }
    }

    private void Restore(CoflowCaptureLayout source, CoflowValueRegister target, CoflowClosure closure)
    {
        if (target.Shape.Type != source.Shape.Type)
        {
            throw new InvalidOperationException("closure capture type mismatch");
        }
        if (target.Shape.Kind == CoflowValueShapeKind.Unit)
        {
            return;
        }
        if (target.Shape.Kind is CoflowValueShapeKind.Scalar or
            CoflowValueShapeKind.Collection or CoflowValueShapeKind.Record)
        {
            switch (target.Shape.ScalarKind)
            {
                case CoflowRegisterKind.Integer:
                    Registers.WriteInteger(target.Scalar, closure.Integer(source.IntegerBase));
                    break;
                case CoflowRegisterKind.Float:
                    Registers.WriteFloat(target.Scalar, closure.Float(source.FloatBase));
                    break;
                default:
                    Registers.WriteReference(target.Scalar, closure.Reference(source.ReferenceBase));
                    break;
            }
        }
        else if (target.Shape.Kind is CoflowValueShapeKind.Struct or CoflowValueShapeKind.Function)
        {
            for (int lane = 0; lane < target.Shape.IntegerCount; lane++)
            {
                Registers.WriteInteger(new CoflowRegister(CoflowRegisterKind.Integer, target.IntegerBase + lane),
                    closure.Integer(source.IntegerBase + lane));
            }
            for (int lane = 0; lane < target.Shape.FloatCount; lane++)
            {
                Registers.WriteFloat(new CoflowRegister(CoflowRegisterKind.Float, target.FloatBase + lane),
                    closure.Float(source.FloatBase + lane));
            }
            for (int lane = 0; lane < target.Shape.ReferenceCount; lane++)
            {
                Registers.WriteReference(new CoflowRegister(CoflowRegisterKind.Reference, target.ReferenceBase + lane),
                    closure.Reference(source.ReferenceBase + lane));
            }
        }
        else
        {
            Registers.WriteInteger(target.Tag, closure.Integer(source.IntegerBase));
            Restore(new CoflowCaptureLayout(target.Shape.First!, source.IntegerBase + 1, source.FloatBase, source.ReferenceBase), target.First, closure);
            if (target.Shape.Kind == CoflowValueShapeKind.Result)
            {
                Restore(new CoflowCaptureLayout(target.Shape.Second!, source.IntegerBase + 1 + target.Shape.First!.IntegerCount, source.FloatBase + target.Shape.First.FloatCount, source.ReferenceBase + target.Shape.First.ReferenceCount), target.Second, closure);
            }
        }
    }

    internal void WriteCaptures(CoflowClosure closure, int parameterOffset)
    {
        for (int i = 0; i < closure.Captures.Count; i++)
        {
            Restore(closure.Captures[i], Parameter(parameterOffset + i), closure);
        }
    }

    private void PushFrame(CoflowValueRegister returnTarget)
    {
        _budgetLease.EnterFrame();
        CoflowRegisterWindow window = _registers.Window;
        _frames.Push(new CoflowFrame
        {
            Program = Program,
            ReturnPc = Pc,
            IntegerBase = window.IntegerBase,
            FloatBase = window.FloatBase,
            ReferenceBase = window.ReferenceBase,
            ReturnTarget = returnTarget
        });
    }

    public void Dispose()
    {
        _budgetLease.Release();
        _registers.ClearAndTrim();
        _frames.ClearAndTrim();
        _values.Clear();
        Program = null!;
        _environment = null;
        Runtime = null;
        CoflowVm.ReturnContext(this);
    }
}
}
