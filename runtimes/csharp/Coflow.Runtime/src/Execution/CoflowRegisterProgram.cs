namespace Coflow.Runtime.CompilerServices;

internal enum CoflowRegisterKind : byte { Integer, Float, Reference }

internal readonly record struct CoflowRegister(CoflowRegisterKind Kind, int Index);

internal enum CoflowValueShapeKind : byte { Scalar, Unit, Option, Result, Struct, Collection, Record, Function }

internal sealed class CoflowValueShape
{
    private static readonly System.Collections.Concurrent.ConcurrentDictionary<Type, CoflowValueShape> BaseLayouts = new(
        new Dictionary<Type, CoflowValueShape>
        {
            [typeof(Unit)] = new(typeof(Unit), CoflowValueShapeKind.Unit, null, null, null),
            [typeof(long)] = new(typeof(long), CoflowValueShapeKind.Scalar, CoflowRegisterKind.Integer, null, null),
            [typeof(bool)] = new(typeof(bool), CoflowValueShapeKind.Scalar, CoflowRegisterKind.Integer, null, null),
            [typeof(double)] = new(typeof(double), CoflowValueShapeKind.Scalar, CoflowRegisterKind.Float, null, null),
            [typeof(string)] = new(typeof(string), CoflowValueShapeKind.Scalar, CoflowRegisterKind.Reference, null, null),
        });

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
            BaseLayouts.TryGetValue(type, out layout)) return layout;
        if (CoflowLayoutCompilation.IsActive)
        {
            CoflowLayoutCompilation.Declare(type);
            if (CoflowLayoutCompilation.TryGet(type, out layout)) return layout;
        }
        throw new InvalidOperationException($"Schema does not declare a VM layout for `{type}`.");
    }

    internal static void RegisterScalar(Type type, CoflowRegisterKind kind) =>
        Register(new(type, CoflowValueShapeKind.Scalar, kind, null, null));

    internal static void RegisterOption(Type type, Type itemType) =>
        Register(new(type, CoflowValueShapeKind.Option, null, Of(itemType), null));

    internal static void RegisterResult(Type type, Type okType, Type errorType) =>
        Register(new(type, CoflowValueShapeKind.Result, null, Of(okType), Of(errorType)));

    internal static void RegisterCollection(Type type) =>
        Register(new(type, CoflowValueShapeKind.Collection, CoflowRegisterKind.Integer, null, null));

    internal static void RegisterFunction(Type type) =>
        Register(new(type, CoflowValueShapeKind.Function, null, null, null, 2, 0, 0));

    internal static void RegisterRecord(Type type) =>
        Register(new(type, CoflowValueShapeKind.Record, CoflowRegisterKind.Integer, null, null));

    internal static void RegisterStruct(Type type, int integers, int floats, int references) =>
        Register(new(type, CoflowValueShapeKind.Struct, null, null, null, integers, floats, references));

    internal static CoflowRegisterKind Scalar(Type type) => Of(type).ScalarKind ??
        throw new InvalidOperationException($"`{type}` has no scalar VM layout.");

    private static void Register(CoflowValueShape layout)
    {
        var current = BaseLayouts.GetOrAdd(layout.Type, layout);
        RequireCompatible(current, layout);
    }

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
}

/// <summary>由生成 Schema 在模块初始化时注册 closed value type 的固定 VM 布局。</summary>
[System.ComponentModel.EditorBrowsable(System.ComponentModel.EditorBrowsableState.Never)]
public static class CoflowValueLayout
{
    public static void RegisterEnum<T>() where T : struct, Enum =>
        CoflowValueShape.RegisterScalar(typeof(T), CoflowRegisterKind.Integer);
    public static void RegisterOption<T>() =>
        CoflowValueShape.RegisterOption(typeof(Option<T>), typeof(T));
    public static void RegisterResult<TOk, TError>() =>
        CoflowValueShape.RegisterResult(typeof(Result<TOk, TError>), typeof(TOk), typeof(TError));
    public static void RegisterArray<T>() =>
        CoflowValueShape.RegisterCollection(typeof(IReadOnlyList<T>));
    public static void RegisterDictionary<TKey, TValue>() where TKey : notnull =>
        CoflowValueShape.RegisterCollection(typeof(IReadOnlyDictionary<TKey, TValue>));
    public static void RegisterFunction<TFunction>() => CoflowValueShape.RegisterFunction(typeof(TFunction));
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
    MoveInteger, MoveFloat, MoveReference, MoveValue,
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

internal readonly record struct CoflowLoweredInstruction(
    CoflowRegisterOpCode Code,
    int A = 0,
    int B = 0,
    int C = 0,
    long Immediate = 0,
    object? Operation = null);

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

internal sealed record CoflowRegisterCollectionSite(
    CoflowValueRegister[] First,
    CoflowValueRegister[]? Second,
    CoflowValueRegister Target);

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

internal sealed record CoflowRegisterClosureSite(
    CoflowClosureTemplate Template,
    CoflowValueRegister[] Captures,
    CoflowValueRegister Target);

internal sealed class CoflowRegisterCallSite(
    int programIndex,
    CoflowFunctionSignature signature,
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
    internal CoflowValueRegister[] SourceArguments { get; } = sourceArguments;
    internal CoflowValueRegister[] Arguments { get; } = windowArguments;
    internal bool[] CopyArguments { get; } = copyArguments;
    internal CoflowValueRegister Result { get; } = result;
    internal int IntegerWindowBase { get; } = integerWindowBase;
    internal int FloatWindowBase { get; } = floatWindowBase;
    internal int ReferenceWindowBase { get; } = referenceWindowBase;
}

internal sealed record CoflowRegisterIndirectCallSite(
    CoflowValueRegister Callable,
    CoflowValueRegister[] Arguments,
    CoflowValueRegister Result,
    Type ResultType);

internal sealed record CoflowLoweringInput(
    CoflowFunctionIdentity Identity,
    CoflowInstruction[] Instructions,
    CfdSpan?[] InstructionSpans,
    object?[] Operations,
    CoflowEncodedValue?[] EncodedConstants,
    Type[] ParameterTypes,
    Type ReturnType,
    int LocalCount,
    IReadOnlyDictionary<CoflowFunctionIdentity, int> FunctionIndexes);

internal sealed class CoflowRegisterOperations
{
    internal object?[] References { get; init; } = Array.Empty<object?>();
    internal CoflowRegisterConstantSite[] Constants { get; init; } = Array.Empty<CoflowRegisterConstantSite>();
    internal CoflowRegisterValueTransfer[] Transfers { get; init; } = Array.Empty<CoflowRegisterValueTransfer>();
    internal CoflowFieldAccess[] Fields { get; init; } = Array.Empty<CoflowFieldAccess>();
    internal CoflowRegisterFieldValueSite[] FieldValues { get; init; } = Array.Empty<CoflowRegisterFieldValueSite>();
    internal CoflowNativeCallSite[] NativeCalls { get; init; } = Array.Empty<CoflowNativeCallSite>();
    internal CoflowRegisterCollectionSite[] Collections { get; init; } = Array.Empty<CoflowRegisterCollectionSite>();
    internal CoflowRegisterArrayIndexSite[] Indexes { get; init; } = Array.Empty<CoflowRegisterArrayIndexSite>();
    internal CoflowRegisterCollectionReadSite[] CollectionReads { get; init; } = Array.Empty<CoflowRegisterCollectionReadSite>();
    internal CoflowRegisterCollectionProjectionSite[] Projections { get; init; } = Array.Empty<CoflowRegisterCollectionProjectionSite>();
    internal CoflowRegisterCollectionBuiltinSite[] CollectionBuiltins { get; init; } = Array.Empty<CoflowRegisterCollectionBuiltinSite>();
    internal CoflowRegisterArrayBuilderSite[] ArrayBuilders { get; init; } = Array.Empty<CoflowRegisterArrayBuilderSite>();
    internal CoflowRegisterTargetSite[] Targets { get; init; } = Array.Empty<CoflowRegisterTargetSite>();
    internal CoflowRegisterPropagateSite[] Propagates { get; init; } = Array.Empty<CoflowRegisterPropagateSite>();
    internal CoflowRegisterClosureSite[] Closures { get; init; } = Array.Empty<CoflowRegisterClosureSite>();
    internal Type[] Types { get; init; } = Array.Empty<Type>();
    internal CoflowRegisterCallSite[] Calls { get; init; } = Array.Empty<CoflowRegisterCallSite>();
    internal CoflowRegisterIndirectCallSite[] IndirectCalls { get; init; } = Array.Empty<CoflowRegisterIndirectCallSite>();

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

        internal int Add(CoflowRegisterOpCode code, object? operation) => code switch
        {
            CoflowRegisterOpCode.ConstantReference => Add(_references, operation),
            CoflowRegisterOpCode.ConstantValue => Add(_constants, (CoflowRegisterConstantSite)operation!),
            CoflowRegisterOpCode.MoveValue or CoflowRegisterOpCode.MakeOptionSome or
                CoflowRegisterOpCode.MakeResultOk or CoflowRegisterOpCode.MakeResultErr or
                CoflowRegisterOpCode.ReadFirstPayload or CoflowRegisterOpCode.ReadSecondPayload =>
                Add(_transfers, (CoflowRegisterValueTransfer)operation!),
            CoflowRegisterOpCode.LoadHostFieldInteger or CoflowRegisterOpCode.LoadHostFieldFloat or
                CoflowRegisterOpCode.LoadHostFieldReference or CoflowRegisterOpCode.LoadArenaFieldInteger or
                CoflowRegisterOpCode.LoadArenaFieldFloat or CoflowRegisterOpCode.LoadArenaFieldReference =>
                Add(_fields, (CoflowFieldAccess)operation!),
            CoflowRegisterOpCode.LoadHostFieldValue or CoflowRegisterOpCode.LoadArenaFieldValue =>
                Add(_fieldValues, (CoflowRegisterFieldValueSite)operation!),
            CoflowRegisterOpCode.Native => Add(_nativeCalls, (CoflowNativeCallSite)operation!),
            CoflowRegisterOpCode.MakeArray or CoflowRegisterOpCode.MakeDictionary =>
                Add(_collections, (CoflowRegisterCollectionSite)operation!),
            CoflowRegisterOpCode.ArrayIndex or CoflowRegisterOpCode.DictionaryIndex =>
                Add(_indexes, (CoflowRegisterArrayIndexSite)operation!),
            CoflowRegisterOpCode.CollectionCount or CoflowRegisterOpCode.ArrayItem or
                CoflowRegisterOpCode.DictionaryKey or CoflowRegisterOpCode.DictionaryValue =>
                Add(_collectionReads, (CoflowRegisterCollectionReadSite)operation!),
            CoflowRegisterOpCode.DictionaryKeys or CoflowRegisterOpCode.DictionaryValues =>
                Add(_projections, (CoflowRegisterCollectionProjectionSite)operation!),
            CoflowRegisterOpCode.CollectionBuiltin =>
                Add(_collectionBuiltins, (CoflowRegisterCollectionBuiltinSite)operation!),
            CoflowRegisterOpCode.BeginArrayBuilder or CoflowRegisterOpCode.AppendArrayBuilder =>
                Add(_arrayBuilders, (CoflowRegisterArrayBuilderSite)operation!),
            CoflowRegisterOpCode.MakeOptionNone or CoflowRegisterOpCode.Return =>
                Add(_targets, (CoflowRegisterTargetSite)operation!),
            CoflowRegisterOpCode.Propagate => Add(_propagates, (CoflowRegisterPropagateSite)operation!),
            CoflowRegisterOpCode.MakeClosure => Add(_closures, (CoflowRegisterClosureSite)operation!),
            CoflowRegisterOpCode.IsType or CoflowRegisterOpCode.IsArenaType => Add(_types, (Type)operation!),
            CoflowRegisterOpCode.Call or CoflowRegisterOpCode.TailCall =>
                Add(_calls, (CoflowRegisterCallSite)operation!),
            CoflowRegisterOpCode.CallIndirect or CoflowRegisterOpCode.TailCallIndirect =>
                Add(_indirectCalls, (CoflowRegisterIndirectCallSite)operation!),
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
            References = _references.ToArray(),
            Constants = _constants.ToArray(),
            Transfers = _transfers.ToArray(),
            Fields = _fields.ToArray(),
            FieldValues = _fieldValues.ToArray(),
            NativeCalls = _nativeCalls.ToArray(),
            Collections = _collections.ToArray(),
            Indexes = _indexes.ToArray(),
            CollectionReads = _collectionReads.ToArray(),
            Projections = _projections.ToArray(),
            CollectionBuiltins = _collectionBuiltins.ToArray(),
            ArrayBuilders = _arrayBuilders.ToArray(),
            Targets = _targets.ToArray(),
            Propagates = _propagates.ToArray(),
            Closures = _closures.ToArray(),
            Types = _types.ToArray(),
            Calls = _calls.ToArray(),
            IndirectCalls = _indirectCalls.ToArray(),
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
    }

    internal CoflowValueRegister[] Parameters { get; }
    internal CoflowRegisterInstruction[] Instructions { get; }
    internal CfdSpan?[] InstructionSpans { get; }
    internal long[] Immediates { get; }
    internal CoflowRegisterOperations Operations { get; }
    internal int IntegerRegisterCount { get; }
    internal int FloatRegisterCount { get; }
    internal int ReferenceRegisterCount { get; }
    internal int ParameterIntegerCount { get; }
    internal int ParameterFloatCount { get; }
    internal int ParameterReferenceCount { get; }
}
