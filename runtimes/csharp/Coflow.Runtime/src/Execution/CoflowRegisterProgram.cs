namespace Coflow.Runtime.CompilerServices;

/// <summary>最终程序持有的只读连续数据；构造时复制，避免验证后的编码被调用方修改。</summary>
internal sealed class CoflowFrozenArray<T> : IReadOnlyList<T>
{
    private readonly T[] _items;

    private CoflowFrozenArray(T[] items, bool takeOwnership) =>
        _items = takeOwnership ? items : (T[])items.Clone();

    internal static CoflowFrozenArray<T> Empty { get; } = new(Array.Empty<T>(), true);
    internal static CoflowFrozenArray<T> CopyOf(T[] items) =>
        items.Length == 0 ? Empty : new(items, false);
    internal static CoflowFrozenArray<T> Owned(T[] items) =>
        items.Length == 0 ? Empty : new(items, true);
    public static implicit operator CoflowFrozenArray<T>(T[] items) => CopyOf(items);

    internal int Length => _items.Length;
    public int Count => _items.Length;
    public T this[int index] => _items[index];
    public IEnumerator<T> GetEnumerator() => ((IEnumerable<T>)_items).GetEnumerator();
    System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => _items.GetEnumerator();
}

internal enum CoflowRegisterKind : byte { Integer, Float, Reference }

internal readonly record struct CoflowRegister(CoflowRegisterKind Kind, int Index);

internal enum CoflowValueShapeKind : byte { Scalar, Unit, Option, Result, Struct, Collection, Record, Function }

internal sealed class CoflowValueShape
{
    private static readonly IReadOnlyDictionary<Type, CoflowValueShape> BaseLayouts =
        new Dictionary<Type, CoflowValueShape>
        {
            [typeof(Unit)] = new(typeof(Unit), CoflowValueShapeKind.Unit, null, null, null),
            [typeof(long)] = new(typeof(long), CoflowValueShapeKind.Scalar, CoflowRegisterKind.Integer, null, null),
            [typeof(bool)] = new(typeof(bool), CoflowValueShapeKind.Scalar, CoflowRegisterKind.Integer, null, null),
            [typeof(double)] = new(typeof(double), CoflowValueShapeKind.Scalar, CoflowRegisterKind.Float, null, null),
            [typeof(string)] = new(typeof(string), CoflowValueShapeKind.Scalar, CoflowRegisterKind.Reference, null, null),
        };

    internal CoflowValueShape(
        Type type,
        CoflowValueShapeKind kind,
        CoflowRegisterKind? scalarKind,
        CoflowValueShape? first,
        CoflowValueShape? second,
        int? integerCount = null,
        int? floatCount = null,
        int? referenceCount = null)
    {
        Type = type;
        Kind = kind;
        ScalarKind = scalarKind;
        First = first;
        Second = second;
        IntegerCount = integerCount ??
            ((kind is CoflowValueShapeKind.Option or CoflowValueShapeKind.Result ? 1 : 0) +
             (scalarKind == CoflowRegisterKind.Integer ? 1 : 0) +
             (first?.IntegerCount ?? 0) + (second?.IntegerCount ?? 0));
        FloatCount = floatCount ?? ((scalarKind == CoflowRegisterKind.Float ? 1 : 0) +
            (first?.FloatCount ?? 0) + (second?.FloatCount ?? 0));
        ReferenceCount = referenceCount ?? ((scalarKind == CoflowRegisterKind.Reference ? 1 : 0) +
            (first?.ReferenceCount ?? 0) + (second?.ReferenceCount ?? 0));
    }

    internal Type Type { get; }
    internal CoflowValueShapeKind Kind { get; }
    internal CoflowRegisterKind? ScalarKind { get; }
    internal CoflowValueShape? First { get; }
    internal CoflowValueShape? Second { get; }
    internal int IntegerCount { get; }
    internal int FloatCount { get; }
    internal int ReferenceCount { get; }

    internal static CoflowValueShape Of(Type type)
    {
        if (CoflowLayoutCompilation.TryGet(type, out var layout) ||
            CoflowInvocationContext.TryGetLayout(type, out layout) ||
            (CoflowSchemaRuntimeContext.TryGet(out var runtime) && runtime.TryGetLayout(type, out layout)) ||
            BaseLayouts.TryGetValue(type, out layout)) return layout;
        if (CoflowLayoutCompilation.IsActive)
        {
            CoflowLayoutCompilation.Declare(type);
            if (CoflowLayoutCompilation.TryGet(type, out layout)) return layout;
        }
        throw new InvalidOperationException($"Schema does not declare a VM layout for `{type}`.");
    }

    internal static CoflowRegisterKind Scalar(Type type) => Of(type).ScalarKind ??
        throw new InvalidOperationException($"`{type}` has no scalar VM layout.");

    internal static void RequireCompatible(CoflowValueShape current, CoflowValueShape layout)
    {
        if (current.Kind == layout.Kind && current.IntegerCount == layout.IntegerCount &&
            current.FloatCount == layout.FloatCount && current.ReferenceCount == layout.ReferenceCount)
            return;
        throw new InvalidOperationException($"Conflicting VM layouts were registered for `{layout.Type}`.");
    }
}
internal static class CoflowLayoutCompilation
{
    [ThreadStatic]
    private static CoflowLayoutRegistry? _registry;

    internal static bool IsActive => _registry is not null;

    internal static Scope Enter(CoflowLayoutRegistry registry)
    {
        if (_registry is not null) throw new InvalidOperationException("Coflow layout compilation cannot be nested.");
        _registry = registry;
        return new Scope();
    }

    internal static bool TryGet(Type type, out CoflowValueShape layout)
    {
        if (_registry is not null) return _registry.TryGet(type, out layout);
        layout = null!;
        return false;
    }

    internal static void Register(CoflowValueShape layout) =>
        (_registry ?? throw new InvalidOperationException("No Coflow layout compilation is active."))
        .Register(layout);

    internal static void Declare(Type type)
    {
        // CLR Type 仅作为编译器的 closed type key；布局组成在发布前完成，执行期禁止进入这里。
        if (type.IsEnum)
        {
            Register(new CoflowValueShape(type, CoflowValueShapeKind.Scalar,
                CoflowRegisterKind.Integer, null, null));
            return;
        }
        if (!type.IsGenericType) return;
        var definition = type.GetGenericTypeDefinition();
        var arguments = type.GetGenericArguments();
        foreach (var argument in arguments) _ = CoflowValueShape.Of(argument);
        if (definition == typeof(Option<>))
            Register(new CoflowValueShape(type, CoflowValueShapeKind.Option,
                null, CoflowValueShape.Of(arguments[0]), null));
        else if (definition == typeof(Result<,>))
            Register(new CoflowValueShape(type, CoflowValueShapeKind.Result,
                null, CoflowValueShape.Of(arguments[0]), CoflowValueShape.Of(arguments[1])));
        else if (definition == typeof(IReadOnlyList<>) || definition == typeof(IReadOnlyDictionary<,>))
            Register(new CoflowValueShape(type, CoflowValueShapeKind.Collection,
                CoflowRegisterKind.Integer, null, null));
        else if (CoflowFunctionHandle.IsFunctionType(type))
            Register(new CoflowValueShape(type, CoflowValueShapeKind.Function,
                null, null, null, 2, 0, 0));
    }

    internal readonly struct Scope : IDisposable
    {
        public void Dispose()
        {
            if (_registry is null) throw new InvalidOperationException("Coflow layout compilation scope is unbalanced.");
            _registry = null;
        }
    }
}

internal sealed class CoflowLayoutRegistry
{
    private readonly Dictionary<Type, CoflowValueShape> _layouts = new();

    internal bool TryGet(Type type, out CoflowValueShape layout) => _layouts.TryGetValue(type, out layout!);

    internal void Register(CoflowValueShape layout)
    {
        if (_layouts.TryGetValue(layout.Type, out var current))
        {
            CoflowValueShape.RequireCompatible(current, layout);
            return;
        }
        _layouts.Add(layout.Type, layout);
    }

    internal CoflowLayoutRegistry Clone()
    {
        var clone = new CoflowLayoutRegistry();
        foreach (var layout in _layouts.Values) clone.Register(layout);
        return clone;
    }
}

internal readonly record struct CoflowValueRegister(
    CoflowValueShape Shape,
    int IntegerBase,
    int FloatBase,
    int ReferenceBase)
{
    internal CoflowRegister Scalar => Shape.ScalarKind switch
    {
        CoflowRegisterKind.Integer => new(CoflowRegisterKind.Integer, IntegerBase),
        CoflowRegisterKind.Float => new(CoflowRegisterKind.Float, FloatBase),
        CoflowRegisterKind.Reference => new(CoflowRegisterKind.Reference, ReferenceBase),
        _ => throw new InvalidOperationException($"`{Shape.Type}` is not a scalar VM value."),
    };

    internal CoflowRegister Tag => Shape.Kind is CoflowValueShapeKind.Option or CoflowValueShapeKind.Result
        ? new(CoflowRegisterKind.Integer, IntegerBase)
        : throw new InvalidOperationException($"`{Shape.Type}` has no tag register.");

    internal CoflowValueRegister First => Child(Shape.First, 1);
    internal CoflowValueRegister Second => Child(
        Shape.Second,
        1 + (Shape.First?.IntegerCount ?? 0),
        Shape.First?.FloatCount ?? 0,
        Shape.First?.ReferenceCount ?? 0);

    private CoflowValueRegister Child(
        CoflowValueShape? child,
        int integerOffset,
        int floatOffset = 0,
        int referenceOffset = 0) => child is null
            ? throw new InvalidOperationException($"`{Shape.Type}` has no requested payload.")
            : new(child, IntegerBase + integerOffset, FloatBase + floatOffset, ReferenceBase + referenceOffset);
}

internal enum CoflowRegisterOpCode : byte
{
    Nop,
    ConstantInteger, ConstantFloat, ConstantReference, ConstantValue,
    MoveInteger, MoveFloat, MoveReference, ClearReference, MoveValue,
    LoadHostFieldInteger, LoadHostFieldFloat, LoadHostFieldReference, LoadHostFieldValue,
    LoadArenaFieldInteger, LoadArenaFieldFloat, LoadArenaFieldReference, LoadArenaFieldValue, Native,
    MakeArray, MakeDictionary, ArrayIndex, DictionaryIndex,
    CollectionCount, ArrayItem, DictionaryKey, DictionaryValue, DictionaryKeys, DictionaryValues,
    CollectionBuiltin,
    BeginArrayBuilder, AppendArrayBuilder,
    MakeOptionNone, MakeOptionSome, MakeResultOk, MakeResultErr,
    ReadValueTag, ReadFirstPayload, ReadSecondPayload, Propagate,
    MakeClosure,
    ConvertIntToFloat, ConvertFloatToInt, IsType, IsArenaType,
    NegateInt, NegateFloat, Not, BitNot,
    AddInt, AddFloat, AddString, SubtractInt, SubtractFloat, MultiplyInt, MultiplyFloat,
    DivideInt, DivideFloat, IntegerDivide, Remainder, PowerInt, PowerFloat,
    ShiftLeft, ShiftRight, BitAnd, BitXor, BitOr,
    LessInt, LessFloat, LessString, LessOrEqualInt, LessOrEqualFloat, LessOrEqualString,
    GreaterInt, GreaterFloat, GreaterString,
    GreaterOrEqualInt, GreaterOrEqualFloat, GreaterOrEqualString,
    EqualInteger, EqualFloat, EqualReference,
    JumpIfFalse, JumpIfTrue, Jump,
    Call, CallIndirect, TailCall, TailCallIndirect, Return,
}

internal readonly record struct CoflowRegisterInstruction(
    CoflowRegisterOpCode Code,
    int A = 0,
    int B = 0,
    int C = 0);

internal sealed record CoflowRegisterValueTransfer(
    CoflowValueRegister Source,
    CoflowValueRegister Target);

internal sealed record CoflowRegisterConstantSite(
    CoflowEncodedValue Value,
    CoflowValueRegister Target);

internal sealed record CoflowRegisterFieldValueSite(
    CoflowFieldAccess Access,
    CoflowValueRegister Target);

internal sealed record CoflowRegisterTargetSite(CoflowValueRegister Target);

internal sealed class CoflowRegisterCollectionSite
{
    internal CoflowRegisterCollectionSite(
        CoflowValueRegister[] first,
        CoflowValueRegister[]? second,
        CoflowValueRegister target)
    {
        First = CoflowFrozenArray<CoflowValueRegister>.CopyOf(first);
        Second = second is null ? null : CoflowFrozenArray<CoflowValueRegister>.CopyOf(second);
        Target = target;
    }

    internal CoflowFrozenArray<CoflowValueRegister> First { get; }
    internal CoflowFrozenArray<CoflowValueRegister>? Second { get; }
    internal CoflowValueRegister Target { get; }
}

internal sealed record CoflowRegisterArrayIndexSite(
    CoflowValueRegister Collection,
    CoflowValueRegister Index,
    CoflowValueRegister Target);

internal sealed record CoflowRegisterCollectionReadSite(
    CoflowValueRegister Collection,
    CoflowValueRegister? Index,
    CoflowValueRegister Target);

internal sealed record CoflowRegisterCollectionProjectionSite(
    CoflowValueRegister Source,
    CoflowValueRegister Target);

internal sealed record CoflowRegisterCollectionBuiltinSite(
    CoflowBuiltin Builtin,
    CoflowValueRegister Receiver,
    CoflowValueRegister? Argument,
    CoflowValueRegister Target);

internal sealed record CoflowRegisterArrayBuilderSite(
    CoflowValueRegister CollectionOrCapacity,
    CoflowValueRegister? Item,
    CoflowValueRegister Target);

internal sealed record CoflowRegisterPropagateSite(
    CoflowValueRegister Source,
    CoflowValueRegister Payload,
    CoflowValueRegister ReturnValue);

internal sealed class CoflowRegisterClosureSite
{
    internal CoflowRegisterClosureSite(
        CoflowClosureTemplate template,
        CoflowValueRegister[] captures,
        CoflowValueRegister target)
    {
        Template = template;
        Captures = CoflowFrozenArray<CoflowValueRegister>.CopyOf(captures);
        Target = target;
    }

    internal CoflowClosureTemplate Template { get; }
    internal CoflowFrozenArray<CoflowValueRegister> Captures { get; }
    internal CoflowValueRegister Target { get; }
}

internal sealed class CoflowRegisterCallSite(
    int programIndex,
    CoflowFunctionSignature signature,
    Type[] vmParameterTypes,
    CoflowValueRegister[] sourceArguments,
    CoflowValueRegister[] windowArguments,
    bool[] copyArguments,
    CoflowValueRegister result,
    int integerWindowBase,
    int floatWindowBase,
    int referenceWindowBase)
{
    internal int ProgramIndex { get; } = programIndex;
    internal CoflowFunctionSignature Signature { get; } = signature;
    internal CoflowFrozenArray<Type> VmParameterTypes { get; } = CoflowFrozenArray<Type>.CopyOf(vmParameterTypes);
    internal CoflowFrozenArray<CoflowValueRegister> SourceArguments { get; } = CoflowFrozenArray<CoflowValueRegister>.CopyOf(sourceArguments);
    internal CoflowFrozenArray<CoflowValueRegister> Arguments { get; } = CoflowFrozenArray<CoflowValueRegister>.CopyOf(windowArguments);
    internal CoflowFrozenArray<bool> CopyArguments { get; } = CoflowFrozenArray<bool>.CopyOf(copyArguments);
    internal CoflowValueRegister Result { get; } = result;
    internal int IntegerWindowBase { get; } = integerWindowBase;
    internal int FloatWindowBase { get; } = floatWindowBase;
    internal int ReferenceWindowBase { get; } = referenceWindowBase;

    internal CoflowRegisterCallSite Relink(int linkedProgramIndex) => new(
        linkedProgramIndex,
        Signature,
        VmParameterTypes.ToArray(),
        SourceArguments.ToArray(),
        Arguments.ToArray(),
        CopyArguments.ToArray(),
        Result,
        IntegerWindowBase,
        FloatWindowBase,
        ReferenceWindowBase);
}

internal sealed class CoflowRegisterIndirectCallSite
{
    internal CoflowRegisterIndirectCallSite(
        CoflowValueRegister callable,
        CoflowValueRegister[] arguments,
        CoflowValueRegister result,
        Type resultType)
    {
        Callable = callable;
        Arguments = CoflowFrozenArray<CoflowValueRegister>.CopyOf(arguments);
        Result = result;
        ResultType = resultType;
    }

    internal CoflowValueRegister Callable { get; }
    internal CoflowFrozenArray<CoflowValueRegister> Arguments { get; }
    internal CoflowValueRegister Result { get; }
    internal Type ResultType { get; }
}

internal sealed class CoflowRegisterOperations
{
    internal CoflowFrozenArray<object?> References { get; init; } = CoflowFrozenArray<object?>.Empty;
    internal CoflowFrozenArray<CoflowRegisterConstantSite> Constants { get; init; } = CoflowFrozenArray<CoflowRegisterConstantSite>.Empty;
    internal CoflowFrozenArray<CoflowRegisterValueTransfer> Transfers { get; init; } = CoflowFrozenArray<CoflowRegisterValueTransfer>.Empty;
    internal CoflowFrozenArray<CoflowFieldAccess> Fields { get; init; } = CoflowFrozenArray<CoflowFieldAccess>.Empty;
    internal CoflowFrozenArray<CoflowRegisterFieldValueSite> FieldValues { get; init; } = CoflowFrozenArray<CoflowRegisterFieldValueSite>.Empty;
    internal CoflowFrozenArray<CoflowNativeCallSite> NativeCalls { get; init; } = CoflowFrozenArray<CoflowNativeCallSite>.Empty;
    internal CoflowFrozenArray<CoflowRegisterCollectionSite> Collections { get; init; } = CoflowFrozenArray<CoflowRegisterCollectionSite>.Empty;
    internal CoflowFrozenArray<CoflowRegisterArrayIndexSite> Indexes { get; init; } = CoflowFrozenArray<CoflowRegisterArrayIndexSite>.Empty;
    internal CoflowFrozenArray<CoflowRegisterCollectionReadSite> CollectionReads { get; init; } = CoflowFrozenArray<CoflowRegisterCollectionReadSite>.Empty;
    internal CoflowFrozenArray<CoflowRegisterCollectionProjectionSite> Projections { get; init; } = CoflowFrozenArray<CoflowRegisterCollectionProjectionSite>.Empty;
    internal CoflowFrozenArray<CoflowRegisterCollectionBuiltinSite> CollectionBuiltins { get; init; } = CoflowFrozenArray<CoflowRegisterCollectionBuiltinSite>.Empty;
    internal CoflowFrozenArray<CoflowRegisterArrayBuilderSite> ArrayBuilders { get; init; } = CoflowFrozenArray<CoflowRegisterArrayBuilderSite>.Empty;
    internal CoflowFrozenArray<CoflowRegisterTargetSite> Targets { get; init; } = CoflowFrozenArray<CoflowRegisterTargetSite>.Empty;
    internal CoflowFrozenArray<CoflowRegisterPropagateSite> Propagates { get; init; } = CoflowFrozenArray<CoflowRegisterPropagateSite>.Empty;
    internal CoflowFrozenArray<CoflowRegisterClosureSite> Closures { get; init; } = CoflowFrozenArray<CoflowRegisterClosureSite>.Empty;
    internal CoflowFrozenArray<Type> Types { get; init; } = CoflowFrozenArray<Type>.Empty;
    internal CoflowFrozenArray<CoflowRegisterCallSite> Calls { get; init; } = CoflowFrozenArray<CoflowRegisterCallSite>.Empty;
    internal CoflowFrozenArray<CoflowRegisterIndirectCallSite> IndirectCalls { get; init; } = CoflowFrozenArray<CoflowRegisterIndirectCallSite>.Empty;

    internal sealed class Builder
    {
        private readonly List<object?> _references = new();
        private readonly List<CoflowRegisterConstantSite> _constants = new();
        private readonly List<CoflowRegisterValueTransfer> _transfers = new();
        private readonly List<CoflowFieldAccess> _fields = new();
        private readonly List<CoflowRegisterFieldValueSite> _fieldValues = new();
        private readonly List<CoflowNativeCallSite> _nativeCalls = new();
        private readonly List<CoflowRegisterCollectionSite> _collections = new();
        private readonly List<CoflowRegisterArrayIndexSite> _indexes = new();
        private readonly List<CoflowRegisterCollectionReadSite> _collectionReads = new();
        private readonly List<CoflowRegisterCollectionProjectionSite> _projections = new();
        private readonly List<CoflowRegisterCollectionBuiltinSite> _collectionBuiltins = new();
        private readonly List<CoflowRegisterArrayBuilderSite> _arrayBuilders = new();
        private readonly List<CoflowRegisterTargetSite> _targets = new();
        private readonly List<CoflowRegisterPropagateSite> _propagates = new();
        private readonly List<CoflowRegisterClosureSite> _closures = new();
        private readonly List<Type> _types = new();
        private readonly List<CoflowRegisterCallSite> _calls = new();
        private readonly List<CoflowRegisterIndirectCallSite> _indirectCalls = new();

        internal int Add(CoflowRegisterOpCode code, object? operation) =>
            CoflowRegisterInstructionSpec.Descriptor(code) switch
        {
            CoflowRegisterDescriptorKind.Reference => Add(_references, operation),
            CoflowRegisterDescriptorKind.Constant => Add(_constants, (CoflowRegisterConstantSite)operation!),
            CoflowRegisterDescriptorKind.Transfer => Add(_transfers, (CoflowRegisterValueTransfer)operation!),
            CoflowRegisterDescriptorKind.Field => Add(_fields, (CoflowFieldAccess)operation!),
            CoflowRegisterDescriptorKind.FieldValue => Add(_fieldValues, (CoflowRegisterFieldValueSite)operation!),
            CoflowRegisterDescriptorKind.NativeCall => Add(_nativeCalls, (CoflowNativeCallSite)operation!),
            CoflowRegisterDescriptorKind.Collection => Add(_collections, (CoflowRegisterCollectionSite)operation!),
            CoflowRegisterDescriptorKind.Index => Add(_indexes, (CoflowRegisterArrayIndexSite)operation!),
            CoflowRegisterDescriptorKind.CollectionRead => Add(_collectionReads, (CoflowRegisterCollectionReadSite)operation!),
            CoflowRegisterDescriptorKind.Projection => Add(_projections, (CoflowRegisterCollectionProjectionSite)operation!),
            CoflowRegisterDescriptorKind.CollectionBuiltin => Add(_collectionBuiltins, (CoflowRegisterCollectionBuiltinSite)operation!),
            CoflowRegisterDescriptorKind.ArrayBuilder => Add(_arrayBuilders, (CoflowRegisterArrayBuilderSite)operation!),
            CoflowRegisterDescriptorKind.Target => Add(_targets, (CoflowRegisterTargetSite)operation!),
            CoflowRegisterDescriptorKind.Propagate => Add(_propagates, (CoflowRegisterPropagateSite)operation!),
            CoflowRegisterDescriptorKind.Closure => Add(_closures, (CoflowRegisterClosureSite)operation!),
            CoflowRegisterDescriptorKind.Type => Add(_types, (Type)operation!),
            CoflowRegisterDescriptorKind.Call => Add(_calls, (CoflowRegisterCallSite)operation!),
            CoflowRegisterDescriptorKind.IndirectCall => Add(_indirectCalls, (CoflowRegisterIndirectCallSite)operation!),
            _ => throw new InvalidOperationException($"Opcode `{code}` has no typed descriptor array."),
        };

        private static int Add<T>(List<T> target, T value)
        {
            var index = target.Count;
            target.Add(value);
            return index;
        }

        internal CoflowRegisterOperations Build() => new()
        {
            References = CoflowFrozenArray<object?>.Owned(_references.ToArray()),
            Constants = CoflowFrozenArray<CoflowRegisterConstantSite>.Owned(_constants.ToArray()),
            Transfers = CoflowFrozenArray<CoflowRegisterValueTransfer>.Owned(_transfers.ToArray()),
            Fields = CoflowFrozenArray<CoflowFieldAccess>.Owned(_fields.ToArray()),
            FieldValues = CoflowFrozenArray<CoflowRegisterFieldValueSite>.Owned(_fieldValues.ToArray()),
            NativeCalls = CoflowFrozenArray<CoflowNativeCallSite>.Owned(_nativeCalls.ToArray()),
            Collections = CoflowFrozenArray<CoflowRegisterCollectionSite>.Owned(_collections.ToArray()),
            Indexes = CoflowFrozenArray<CoflowRegisterArrayIndexSite>.Owned(_indexes.ToArray()),
            CollectionReads = CoflowFrozenArray<CoflowRegisterCollectionReadSite>.Owned(_collectionReads.ToArray()),
            Projections = CoflowFrozenArray<CoflowRegisterCollectionProjectionSite>.Owned(_projections.ToArray()),
            CollectionBuiltins = CoflowFrozenArray<CoflowRegisterCollectionBuiltinSite>.Owned(_collectionBuiltins.ToArray()),
            ArrayBuilders = CoflowFrozenArray<CoflowRegisterArrayBuilderSite>.Owned(_arrayBuilders.ToArray()),
            Targets = CoflowFrozenArray<CoflowRegisterTargetSite>.Owned(_targets.ToArray()),
            Propagates = CoflowFrozenArray<CoflowRegisterPropagateSite>.Owned(_propagates.ToArray()),
            Closures = CoflowFrozenArray<CoflowRegisterClosureSite>.Owned(_closures.ToArray()),
            Types = CoflowFrozenArray<Type>.Owned(_types.ToArray()),
            Calls = CoflowFrozenArray<CoflowRegisterCallSite>.Owned(_calls.ToArray()),
            IndirectCalls = CoflowFrozenArray<CoflowRegisterIndirectCallSite>.Owned(_indirectCalls.ToArray()),
        };
    }
}

internal sealed class CoflowRegisterProgram
{
    internal CoflowRegisterProgram(
        CoflowValueRegister[] parameters,
        CoflowRegisterInstruction[] instructions,
        CfdSpan?[] instructionSpans,
        long[] immediates,
        CoflowRegisterOperations operations,
        int integerRegisterCount,
        int floatRegisterCount,
        int referenceRegisterCount)
    {
        Parameters = CoflowFrozenArray<CoflowValueRegister>.CopyOf(parameters);
        Instructions = CoflowFrozenArray<CoflowRegisterInstruction>.CopyOf(instructions);
        InstructionSpans = CoflowFrozenArray<CfdSpan?>.CopyOf(instructionSpans);
        Immediates = CoflowFrozenArray<long>.CopyOf(immediates);
        Operations = operations;
        ParameterIntegerCount = parameters.Sum(value => value.Shape.IntegerCount);
        ParameterFloatCount = parameters.Sum(value => value.Shape.FloatCount);
        ParameterReferenceCount = parameters.Sum(value => value.Shape.ReferenceCount);
        IntegerRegisterCount = integerRegisterCount;
        FloatRegisterCount = floatRegisterCount;
        ReferenceRegisterCount = referenceRegisterCount;
        CoflowExecutableVerifier.Verify(this);
    }

    private CoflowRegisterProgram(
        CoflowFrozenArray<CoflowValueRegister> parameters,
        CoflowFrozenArray<CoflowRegisterInstruction> instructions,
        CoflowFrozenArray<CfdSpan?> instructionSpans,
        CoflowFrozenArray<long> immediates,
        CoflowRegisterOperations operations,
        int integerRegisterCount,
        int floatRegisterCount,
        int referenceRegisterCount)
    {
        Parameters = parameters;
        Instructions = instructions;
        InstructionSpans = instructionSpans;
        Immediates = immediates;
        Operations = operations;
        ParameterIntegerCount = parameters.Sum(value => value.Shape.IntegerCount);
        ParameterFloatCount = parameters.Sum(value => value.Shape.FloatCount);
        ParameterReferenceCount = parameters.Sum(value => value.Shape.ReferenceCount);
        IntegerRegisterCount = integerRegisterCount;
        FloatRegisterCount = floatRegisterCount;
        ReferenceRegisterCount = referenceRegisterCount;
        CoflowExecutableVerifier.Verify(this);
    }

    internal CoflowRegisterProgram Relink(CoflowRegisterOperations operations) => new(
        Parameters, Instructions, InstructionSpans, Immediates, operations,
        IntegerRegisterCount, FloatRegisterCount, ReferenceRegisterCount);

    internal CoflowFrozenArray<CoflowValueRegister> Parameters { get; }
    internal CoflowFrozenArray<CoflowRegisterInstruction> Instructions { get; }
    internal CoflowFrozenArray<CfdSpan?> InstructionSpans { get; }
    internal CoflowFrozenArray<long> Immediates { get; }
    internal CoflowRegisterOperations Operations { get; }
    internal int IntegerRegisterCount { get; }
    internal int FloatRegisterCount { get; }
    internal int ReferenceRegisterCount { get; }
    internal int ParameterIntegerCount { get; }
    internal int ParameterFloatCount { get; }
    internal int ParameterReferenceCount { get; }
}
