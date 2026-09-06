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

internal static class CoflowRegisterLowering
{
    internal static CoflowRegisterProgram Lower(CoflowLoweringInput program)
    {
        var instructions = program.Instructions;
        var states = new Type[instructions.Length + 1][];
        var locals = new Type?[program.LocalCount];
        states[0] = Array.Empty<Type>();
        var changed = true;
        for (var pass = 0; changed && pass <= instructions.Length + program.LocalCount + 1; pass++)
        {
            changed = false;
            for (var pc = 0; pc < instructions.Length; pc++)
            {
                var before = states[pc];
                if (before is null) continue;
                foreach (var successor in Transfer(program, pc, before, locals))
                    changed |= Merge(program, states, successor.Pc, successor.Stack);
            }
        }
        if (states.Take(instructions.Length).Any(value => value is null))
            throw Invalid(program, "program contains unreachable or invalid instruction boundaries");

        var integer = 0;
        var floating = 0;
        var reference = 0;
        var parameters = Allocate(program.ParameterTypes, ref integer, ref floating, ref reference);
        var localTypes = locals.Select(value => value ?? typeof(Unit)).ToArray();
        var localRegisters = Allocate(localTypes, ref integer, ref floating, ref reference);
        var returnValue = Allocate(program.ReturnType, ref integer, ref floating, ref reference);
        var maxDepth = states.Where(value => value is not null).Max(value => value!.Length);
        var temporaries = new CoflowValueRegister[maxDepth];
        for (var depth = 0; depth < maxDepth; depth++)
        {
            var shapes = states.Where(value => value is not null && value.Length > depth)
                .Select(value => CoflowValueShape.Of(value![depth])).ToArray();
            var integerWidth = shapes.Max(value => value.IntegerCount);
            var floatWidth = shapes.Max(value => value.FloatCount);
            var referenceWidth = shapes.Max(value => value.ReferenceCount);
            temporaries[depth] = new CoflowValueRegister(
                CoflowValueShape.Of(typeof(Unit)), integer, floating, reference);
            integer += integerWidth;
            floating += floatWidth;
            reference += referenceWidth;
        }
        var directSignatures = instructions
            .Where(instruction => instruction.Code is CoflowOpCode.Call or CoflowOpCode.TailCall)
            .Select(instruction => (CoflowCallSite)program.Operations[instruction.Operand]!)
            .Select(call => (IReadOnlyList<Type>)call.VmParameterTypes)
            .ToArray();
        var outgoingIntegerBase = integer;
        var outgoingFloatBase = floating;
        var outgoingReferenceBase = reference;
        if (directSignatures.Length != 0)
        {
            integer += directSignatures.Max(types => types.Sum(type => CoflowValueShape.Of(type).IntegerCount));
            floating += directSignatures.Max(types => types.Sum(type => CoflowValueShape.Of(type).FloatCount));
            reference += directSignatures.Max(types => types.Sum(type => CoflowValueShape.Of(type).ReferenceCount));
        }
        var (executable, instructionSpans, immediates, operations) = LowerInstructions(
            program,
            states.Take(instructions.Length).Select(value => value!).ToArray(),
            parameters,
            localRegisters,
            returnValue,
            temporaries,
            outgoingIntegerBase,
            outgoingFloatBase,
            outgoingReferenceBase);
        CompactPhysicalRegisters(
            parameters,
            executable,
            operations,
            ref integer,
            ref floating,
            ref reference);
        return new CoflowRegisterProgram(
            parameters,
            executable,
            instructionSpans,
            immediates,
            operations,
            integer,
            floating,
            reference);
    }

    private static void CompactPhysicalRegisters(
        CoflowValueRegister[] parameters,
        CoflowRegisterInstruction[] instructions,
        CoflowRegisterOperations operations,
        ref int integerCount,
        ref int floatCount,
        ref int referenceCount)
    {
        var integers = new bool[integerCount];
        var floats = new bool[floatCount];
        var references = new bool[referenceCount];

        void Mark(CoflowValueRegister value)
        {
            MarkRange(integers, value.IntegerBase, value.Shape.IntegerCount);
            MarkRange(floats, value.FloatBase, value.Shape.FloatCount);
            MarkRange(references, value.ReferenceBase, value.Shape.ReferenceCount);
        }
        foreach (var parameter in parameters) Mark(parameter);
        foreach (var instruction in instructions)
        {
            MarkOperand(instruction.A, OperandKind(instruction.Code, 0), integers, floats, references);
            MarkOperand(instruction.B, OperandKind(instruction.Code, 1), integers, floats, references);
            MarkOperand(instruction.C, OperandKind(instruction.Code, 2), integers, floats, references);
        }
        VisitOperationRegisters(operations, Mark);

        var integerMap = DenseMap(integers, out integerCount);
        var floatMap = DenseMap(floats, out floatCount);
        var referenceMap = DenseMap(references, out referenceCount);
        CoflowValueRegister Map(CoflowValueRegister value) => value with
        {
            IntegerBase = MapBase(integerMap, value.IntegerBase, value.Shape.IntegerCount),
            FloatBase = MapBase(floatMap, value.FloatBase, value.Shape.FloatCount),
            ReferenceBase = MapBase(referenceMap, value.ReferenceBase, value.Shape.ReferenceCount),
        };

        for (var index = 0; index < parameters.Length; index++) parameters[index] = Map(parameters[index]);
        for (var index = 0; index < instructions.Length; index++)
        {
            var instruction = instructions[index];
            instructions[index] = instruction with
            {
                A = MapOperand(instruction.A, OperandKind(instruction.Code, 0),
                    integerMap, floatMap, referenceMap),
                B = MapOperand(instruction.B, OperandKind(instruction.Code, 1),
                    integerMap, floatMap, referenceMap),
                C = MapOperand(instruction.C, OperandKind(instruction.Code, 2),
                    integerMap, floatMap, referenceMap),
            };
        }
        MapOperationRegisters(operations, Map);
    }

    private static void VisitOperationRegisters(
        CoflowRegisterOperations operations,
        Action<CoflowValueRegister> visit)
    {
        foreach (var value in operations.Constants) VisitOperation(value, visit);
        foreach (var value in operations.Transfers) VisitOperation(value, visit);
        foreach (var value in operations.FieldValues) VisitOperation(value, visit);
        foreach (var value in operations.NativeCalls) VisitOperation(value, visit);
        foreach (var value in operations.Collections) VisitOperation(value, visit);
        foreach (var value in operations.Indexes) VisitOperation(value, visit);
        foreach (var value in operations.CollectionReads) VisitOperation(value, visit);
        foreach (var value in operations.Projections) VisitOperation(value, visit);
        foreach (var value in operations.CollectionBuiltins) VisitOperation(value, visit);
        foreach (var value in operations.ArrayBuilders) VisitOperation(value, visit);
        foreach (var value in operations.Targets) VisitOperation(value, visit);
        foreach (var value in operations.Propagates) VisitOperation(value, visit);
        foreach (var value in operations.Closures) VisitOperation(value, visit);
        foreach (var value in operations.Calls) VisitOperation(value, visit);
        foreach (var value in operations.IndirectCalls) VisitOperation(value, visit);
    }

    private static void MapOperationRegisters(
        CoflowRegisterOperations operations,
        Func<CoflowValueRegister, CoflowValueRegister> map)
    {
        MapArray(operations.Constants, map);
        MapArray(operations.Transfers, map);
        MapArray(operations.FieldValues, map);
        MapArray(operations.NativeCalls, map);
        MapArray(operations.Collections, map);
        MapArray(operations.Indexes, map);
        MapArray(operations.CollectionReads, map);
        MapArray(operations.Projections, map);
        MapArray(operations.CollectionBuiltins, map);
        MapArray(operations.ArrayBuilders, map);
        MapArray(operations.Targets, map);
        MapArray(operations.Propagates, map);
        MapArray(operations.Closures, map);
        for (var index = 0; index < operations.Calls.Length; index++)
            operations.Calls[index] = MapCallSite(operations.Calls[index], map);
        MapArray(operations.IndirectCalls, map);
    }

    private static void MapArray<T>(T[] values,
        Func<CoflowValueRegister, CoflowValueRegister> map) where T : class
    {
        for (var index = 0; index < values.Length; index++)
            values[index] = (T)MapOperation(values[index], map)!;
    }

    private static void MarkRange(bool[] used, int start, int count)
    {
        for (var index = 0; index < count; index++) used[start + index] = true;
    }

    private static int[] DenseMap(bool[] used, out int count)
    {
        var result = new int[used.Length];
        count = 0;
        for (var index = 0; index < used.Length; index++)
            result[index] = used[index] ? count++ : -1;
        return result;
    }

    private static int MapBase(int[] map, int value, int count) => count == 0 ? 0 : map[value];

    private static void MarkOperand(
        int value,
        CoflowRegisterKind? kind,
        bool[] integers,
        bool[] floats,
        bool[] references)
    {
        if (kind is null) return;
        (kind == CoflowRegisterKind.Integer ? integers :
            kind == CoflowRegisterKind.Float ? floats : references)[value] = true;
    }

    private static int MapOperand(
        int value,
        CoflowRegisterKind? kind,
        int[] integers,
        int[] floats,
        int[] references) => kind switch
        {
            CoflowRegisterKind.Integer => integers[value],
            CoflowRegisterKind.Float => floats[value],
            CoflowRegisterKind.Reference => references[value],
            _ => value,
        };

    private static CoflowRegisterKind? OperandKind(CoflowRegisterOpCode code, int operand)
    {
        if (operand == 0)
        {
            if (code is CoflowRegisterOpCode.ConstantInteger or CoflowRegisterOpCode.MoveInteger or
                CoflowRegisterOpCode.LoadHostFieldInteger or CoflowRegisterOpCode.LoadArenaFieldInteger or
                CoflowRegisterOpCode.LoadArenaFieldValue or CoflowRegisterOpCode.ReadValueTag or
                CoflowRegisterOpCode.ConvertFloatToInt or CoflowRegisterOpCode.IsType or CoflowRegisterOpCode.IsArenaType or
                CoflowRegisterOpCode.NegateInt or CoflowRegisterOpCode.Not or CoflowRegisterOpCode.BitNot or
                CoflowRegisterOpCode.AddInt or CoflowRegisterOpCode.SubtractInt or
                CoflowRegisterOpCode.MultiplyInt or CoflowRegisterOpCode.DivideInt or
                CoflowRegisterOpCode.IntegerDivide or CoflowRegisterOpCode.Remainder or
                CoflowRegisterOpCode.PowerInt or CoflowRegisterOpCode.ShiftLeft or
                CoflowRegisterOpCode.ShiftRight or CoflowRegisterOpCode.BitAnd or
                CoflowRegisterOpCode.BitXor or CoflowRegisterOpCode.BitOr or
                CoflowRegisterOpCode.LessInt or CoflowRegisterOpCode.LessOrEqualInt or
                CoflowRegisterOpCode.GreaterInt or CoflowRegisterOpCode.GreaterOrEqualInt or
                CoflowRegisterOpCode.EqualInteger or CoflowRegisterOpCode.LessFloat or
                CoflowRegisterOpCode.LessOrEqualFloat or CoflowRegisterOpCode.GreaterFloat or
                CoflowRegisterOpCode.GreaterOrEqualFloat or CoflowRegisterOpCode.EqualFloat or
                CoflowRegisterOpCode.LessString or CoflowRegisterOpCode.LessOrEqualString or
                CoflowRegisterOpCode.GreaterString or CoflowRegisterOpCode.GreaterOrEqualString or
                CoflowRegisterOpCode.EqualReference or CoflowRegisterOpCode.JumpIfFalse or
                CoflowRegisterOpCode.JumpIfTrue) return CoflowRegisterKind.Integer;
            if (code is CoflowRegisterOpCode.ConstantFloat or CoflowRegisterOpCode.MoveFloat or
                CoflowRegisterOpCode.LoadHostFieldFloat or CoflowRegisterOpCode.LoadArenaFieldFloat or CoflowRegisterOpCode.ConvertIntToFloat or
                CoflowRegisterOpCode.NegateFloat or CoflowRegisterOpCode.AddFloat or
                CoflowRegisterOpCode.SubtractFloat or CoflowRegisterOpCode.MultiplyFloat or
                CoflowRegisterOpCode.DivideFloat or CoflowRegisterOpCode.PowerFloat)
                return CoflowRegisterKind.Float;
            if (code is CoflowRegisterOpCode.ConstantReference or CoflowRegisterOpCode.MoveReference or
                CoflowRegisterOpCode.LoadHostFieldReference or CoflowRegisterOpCode.LoadHostFieldValue or
                CoflowRegisterOpCode.LoadArenaFieldReference or
                CoflowRegisterOpCode.AddString)
                return CoflowRegisterKind.Reference;
            return null;
        }
        if (operand == 1)
        {
            if (code is CoflowRegisterOpCode.MoveInteger or CoflowRegisterOpCode.LoadArenaFieldInteger or
                CoflowRegisterOpCode.LoadArenaFieldFloat or CoflowRegisterOpCode.LoadArenaFieldReference or
                CoflowRegisterOpCode.IsArenaType or
                CoflowRegisterOpCode.ReadValueTag or
                CoflowRegisterOpCode.ConvertIntToFloat or CoflowRegisterOpCode.NegateInt or
                CoflowRegisterOpCode.Not or CoflowRegisterOpCode.BitNot or
                CoflowRegisterOpCode.AddInt or CoflowRegisterOpCode.SubtractInt or
                CoflowRegisterOpCode.MultiplyInt or CoflowRegisterOpCode.DivideInt or
                CoflowRegisterOpCode.IntegerDivide or CoflowRegisterOpCode.Remainder or
                CoflowRegisterOpCode.PowerInt or CoflowRegisterOpCode.ShiftLeft or
                CoflowRegisterOpCode.ShiftRight or CoflowRegisterOpCode.BitAnd or
                CoflowRegisterOpCode.BitXor or CoflowRegisterOpCode.BitOr or
                CoflowRegisterOpCode.LessInt or CoflowRegisterOpCode.LessOrEqualInt or
                CoflowRegisterOpCode.GreaterInt or CoflowRegisterOpCode.GreaterOrEqualInt or
                CoflowRegisterOpCode.EqualInteger) return CoflowRegisterKind.Integer;
            if (code is CoflowRegisterOpCode.MoveFloat or CoflowRegisterOpCode.ConvertFloatToInt or
                CoflowRegisterOpCode.NegateFloat or CoflowRegisterOpCode.AddFloat or
                CoflowRegisterOpCode.SubtractFloat or CoflowRegisterOpCode.MultiplyFloat or
                CoflowRegisterOpCode.DivideFloat or CoflowRegisterOpCode.PowerFloat or
                CoflowRegisterOpCode.LessFloat or CoflowRegisterOpCode.LessOrEqualFloat or
                CoflowRegisterOpCode.GreaterFloat or CoflowRegisterOpCode.GreaterOrEqualFloat or
                CoflowRegisterOpCode.EqualFloat) return CoflowRegisterKind.Float;
            if (code is CoflowRegisterOpCode.LoadHostFieldInteger or CoflowRegisterOpCode.LoadHostFieldFloat or
                CoflowRegisterOpCode.LoadHostFieldReference or CoflowRegisterOpCode.MoveReference or
                CoflowRegisterOpCode.IsType or CoflowRegisterOpCode.AddString or
                CoflowRegisterOpCode.LessString or CoflowRegisterOpCode.LessOrEqualString or
                CoflowRegisterOpCode.GreaterString or CoflowRegisterOpCode.GreaterOrEqualString or
                CoflowRegisterOpCode.EqualReference) return CoflowRegisterKind.Reference;
            return null;
        }
        if (code is CoflowRegisterOpCode.AddInt or CoflowRegisterOpCode.SubtractInt or
            CoflowRegisterOpCode.MultiplyInt or CoflowRegisterOpCode.DivideInt or
            CoflowRegisterOpCode.IntegerDivide or CoflowRegisterOpCode.Remainder or
            CoflowRegisterOpCode.PowerInt or CoflowRegisterOpCode.ShiftLeft or
            CoflowRegisterOpCode.ShiftRight or CoflowRegisterOpCode.BitAnd or
            CoflowRegisterOpCode.BitXor or CoflowRegisterOpCode.BitOr or
            CoflowRegisterOpCode.LessInt or CoflowRegisterOpCode.LessOrEqualInt or
            CoflowRegisterOpCode.GreaterInt or CoflowRegisterOpCode.GreaterOrEqualInt or
            CoflowRegisterOpCode.EqualInteger) return CoflowRegisterKind.Integer;
        if (code is CoflowRegisterOpCode.AddFloat or CoflowRegisterOpCode.SubtractFloat or
            CoflowRegisterOpCode.MultiplyFloat or CoflowRegisterOpCode.DivideFloat or
            CoflowRegisterOpCode.PowerFloat or CoflowRegisterOpCode.LessFloat or
            CoflowRegisterOpCode.LessOrEqualFloat or CoflowRegisterOpCode.GreaterFloat or
            CoflowRegisterOpCode.GreaterOrEqualFloat or CoflowRegisterOpCode.EqualFloat)
            return CoflowRegisterKind.Float;
        if (code is CoflowRegisterOpCode.AddString or CoflowRegisterOpCode.LessString or
            CoflowRegisterOpCode.LessOrEqualString or CoflowRegisterOpCode.GreaterString or
            CoflowRegisterOpCode.GreaterOrEqualString or CoflowRegisterOpCode.EqualReference)
            return CoflowRegisterKind.Reference;
        return null;
    }

    private static void VisitOperation(object? operation, Action<CoflowValueRegister> visit)
    {
        switch (operation)
        {
            case CoflowRegisterValueTransfer value: visit(value.Source); visit(value.Target); break;
            case CoflowRegisterCollectionSite value:
                foreach (var collectionItem in value.First) visit(collectionItem);
                if (value.Second is { } second)
                    foreach (var collectionItem in second) visit(collectionItem);
                visit(value.Target); break;
            case CoflowRegisterArrayIndexSite value:
                visit(value.Collection); visit(value.Index); visit(value.Target); break;
            case CoflowRegisterCollectionReadSite value:
                visit(value.Collection);
                if (value.Index is { } itemIndex) visit(itemIndex);
                visit(value.Target); break;
            case CoflowRegisterCollectionProjectionSite value:
                visit(value.Source); visit(value.Target); break;
            case CoflowRegisterCollectionBuiltinSite value:
                visit(value.Receiver);
                if (value.Argument is { } builtinArgument) visit(builtinArgument);
                visit(value.Target); break;
            case CoflowRegisterArrayBuilderSite value:
                visit(value.CollectionOrCapacity);
                if (value.Item is { } appendItem) visit(appendItem);
                visit(value.Target); break;
            case CoflowRegisterConstantSite value: visit(value.Target); break;
            case CoflowRegisterFieldValueSite value: visit(value.Target); break;
            case CoflowRegisterTargetSite value: visit(value.Target); break;
            case CoflowRegisterPropagateSite value:
                visit(value.Source); visit(value.Payload); visit(value.ReturnValue); break;
            case CoflowRegisterClosureSite value:
                foreach (var capture in value.Captures) visit(capture);
                visit(value.Target); break;
            case CoflowRegisterCallSite value:
                for (var index = 0; index < value.SourceArguments.Length; index++)
                    if (value.CopyArguments[index]) visit(value.SourceArguments[index]);
                foreach (var argument in value.Arguments) visit(argument);
                visit(value.Result); break;
            case CoflowRegisterIndirectCallSite value:
                visit(value.Callable);
                foreach (var argument in value.Arguments) visit(argument);
                visit(value.Result); break;
            case CoflowNativeCallSite value:
                foreach (var argument in value.Arguments) visit(argument);
                visit(value.Result); break;
        }
    }

    private static object? MapOperation(
        object? operation,
        Func<CoflowValueRegister, CoflowValueRegister> map) => operation switch
        {
            CoflowRegisterValueTransfer value => new CoflowRegisterValueTransfer(
                map(value.Source), map(value.Target)),
            CoflowRegisterCollectionSite value => new CoflowRegisterCollectionSite(
                value.First.Select(map).ToArray(), value.Second?.Select(map).ToArray(), map(value.Target)),
            CoflowRegisterArrayIndexSite value => new CoflowRegisterArrayIndexSite(
                map(value.Collection), map(value.Index), map(value.Target)),
            CoflowRegisterCollectionReadSite value => new CoflowRegisterCollectionReadSite(
                map(value.Collection), value.Index is { } index ? map(index) : null, map(value.Target)),
            CoflowRegisterCollectionProjectionSite value => new CoflowRegisterCollectionProjectionSite(
                map(value.Source), map(value.Target)),
            CoflowRegisterCollectionBuiltinSite value => new CoflowRegisterCollectionBuiltinSite(
                value.Builtin, map(value.Receiver),
                value.Argument is { } builtinArgument ? map(builtinArgument) : null,
                map(value.Target)),
            CoflowRegisterArrayBuilderSite value => new CoflowRegisterArrayBuilderSite(
                map(value.CollectionOrCapacity), value.Item is { } item ? map(item) : null,
                map(value.Target)),
            CoflowRegisterConstantSite value => new CoflowRegisterConstantSite(value.Value, map(value.Target)),
            CoflowRegisterFieldValueSite value => new CoflowRegisterFieldValueSite(
                value.Access, map(value.Target)),
            CoflowRegisterTargetSite value => new CoflowRegisterTargetSite(map(value.Target)),
            CoflowRegisterPropagateSite value => new CoflowRegisterPropagateSite(
                map(value.Source), map(value.Payload), map(value.ReturnValue)),
            CoflowRegisterClosureSite value => new CoflowRegisterClosureSite(
                value.Template, value.Captures.Select(map).ToArray(), map(value.Target)),
            CoflowRegisterCallSite value => MapCallSite(value, map),
            CoflowRegisterIndirectCallSite value => new CoflowRegisterIndirectCallSite(
                map(value.Callable), value.Arguments.Select(map).ToArray(), map(value.Result), value.ResultType),
            CoflowNativeCallSite value => new CoflowNativeCallSite(
                value.Call, value.Arguments.Select(map).ToArray(), map(value.Result)),
            _ => operation,
        };

    private static CoflowRegisterCallSite MapCallSite(
        CoflowRegisterCallSite value,
        Func<CoflowValueRegister, CoflowValueRegister> map)
    {
        var arguments = value.Arguments.Select(map).ToArray();
        var sourceArguments = value.SourceArguments.Select((argument, index) =>
            value.CopyArguments[index] ? map(argument) : arguments[index]).ToArray();
        return new CoflowRegisterCallSite(
            value.ProgramIndex,
            value.Signature,
            sourceArguments,
            arguments,
            value.CopyArguments.ToArray(),
            map(value.Result),
            arguments.Where(argument => argument.Shape.IntegerCount != 0)
                .Select(argument => argument.IntegerBase).DefaultIfEmpty(-1).Min(),
            arguments.Where(argument => argument.Shape.FloatCount != 0)
                .Select(argument => argument.FloatBase).DefaultIfEmpty(-1).Min(),
            arguments.Where(argument => argument.Shape.ReferenceCount != 0)
                .Select(argument => argument.ReferenceBase).DefaultIfEmpty(-1).Min());
    }

    private static CoflowValueRegister Allocate(
        Type type,
        ref int integer,
        ref int floating,
        ref int reference)
    {
        var result = new CoflowValueRegister(CoflowValueShape.Of(type), integer, floating, reference);
        integer += result.Shape.IntegerCount;
        floating += result.Shape.FloatCount;
        reference += result.Shape.ReferenceCount;
        return result;
    }

    private static CoflowValueRegister[] Allocate(
        IReadOnlyList<Type> types,
        ref int integer,
        ref int floating,
        ref int reference)
    {
        var result = new CoflowValueRegister[types.Count];
        for (var index = 0; index < types.Count; index++)
        {
            var shape = CoflowValueShape.Of(types[index]);
            result[index] = new CoflowValueRegister(shape, integer, floating, reference);
            integer += shape.IntegerCount;
            floating += shape.FloatCount;
            reference += shape.ReferenceCount;
        }
        return result;
    }

    private static (
        CoflowRegisterInstruction[] Instructions,
        CfdSpan?[] InstructionSpans,
        long[] Immediates,
        CoflowRegisterOperations Operations) LowerInstructions(
        CoflowLoweringInput program,
        Type[][] states,
        CoflowValueRegister[] parameters,
        CoflowValueRegister[] locals,
        CoflowValueRegister returnValue,
        CoflowValueRegister[] temporaries,
        int outgoingIntegerBase,
        int outgoingFloatBase,
        int outgoingReferenceBase)
    {
        var result = new CoflowLoweredInstruction[program.Instructions.Length];
        for (var pc = 0; pc < result.Length; pc++)
        {
            var source = program.Instructions[pc];
            var stack = states[pc];
            var depth = stack.Length;

            CoflowValueRegister Temporary(Type type, int index)
            {
                var storage = temporaries[index];
                return new CoflowValueRegister(
                    CoflowValueShape.Of(type),
                    storage.IntegerBase,
                    storage.FloatBase,
                    storage.ReferenceBase);
            }
            CoflowValueRegister Stack(int index) => Temporary(stack[index], index);
            CoflowValueRegister Top(int offset = 1) => Stack(depth - offset);
            CoflowValueRegister Output(Type type, int consumed) => Temporary(type, depth - consumed);
            object Operation() => program.Operations[source.Operand]
                ?? throw Invalid(program, $"instruction {pc} has no operation descriptor");
            CoflowValueRegister[] Arguments(int count)
            {
                var start = depth - count;
                var arguments = new CoflowValueRegister[count];
                for (var index = 0; index < count; index++)
                    arguments[index] = Stack(start + index);
                return arguments;
            }
            CoflowLoweredInstruction LowerCollectionBuiltin(CoflowBuiltin builtin, Type resultType)
            {
                var consumed = builtin.HasCollectionArgument ? 2 : 1;
                return new CoflowLoweredInstruction(CoflowRegisterOpCode.CollectionBuiltin,
                    Operation: new CoflowRegisterCollectionBuiltinSite(
                        builtin,
                        builtin.HasCollectionArgument ? Top(2) : Top(),
                        builtin.HasCollectionArgument ? Top() : null,
                        Output(resultType, consumed)));
            }

            var valueType = source.Code switch
            {
                CoflowOpCode.Constant => program.EncodedConstants[source.Operand]?.Shape.Type ?? typeof(object),
                CoflowOpCode.Argument => program.ParameterTypes[source.Operand],
                CoflowOpCode.Local => locals[source.Operand].Shape.Type,
                CoflowOpCode.LoadField => ((CoflowFieldAccess)Operation()).RuntimeType,
                CoflowOpCode.Native => ((CoflowNativeCall)Operation()).ResultType,
                CoflowOpCode.Call => ((CoflowCallSite)Operation()).Signature.ResultType,
                CoflowOpCode.TailCall or CoflowOpCode.TailCallIndirect => program.ReturnType,
                _ => source.ValueType ?? typeof(object),
            };
            result[pc] = source.Code switch
            {
                CoflowOpCode.Constant => Constant(
                    program.EncodedConstants[source.Operand]
                        ?? throw Invalid(program, $"instruction {pc} has no encoded constant"),
                    Output(valueType, 0)),
                CoflowOpCode.Argument => Move(parameters[source.Operand], Output(valueType, 0)),
                CoflowOpCode.Local => Move(locals[source.Operand], Output(valueType, 0)),
                CoflowOpCode.StoreLocal => Move(Top(), locals[source.Operand]),
                CoflowOpCode.LoadField => Field(
                    (CoflowFieldAccess)Operation(), Top(), Output(valueType, 1)),
                CoflowOpCode.Native => Native(
                    (CoflowNativeCall)Operation(),
                    Arguments(((CoflowNativeCall)Operation()).ArgumentCount),
                    Output(valueType, ((CoflowNativeCall)Operation()).ArgumentCount)),
                CoflowOpCode.MakeArray => Collection(
                    CoflowRegisterOpCode.MakeArray, Arguments(source.Operand), null,
                    Output(valueType, source.Operand)),
                CoflowOpCode.MakeDictionary => Dictionary(
                    Arguments(checked(source.Operand * 2)),
                    Output(valueType, checked(source.Operand * 2))),
                CoflowOpCode.ArrayIndex or CoflowOpCode.DictionaryIndex => new(
                    source.Code == CoflowOpCode.ArrayIndex
                        ? CoflowRegisterOpCode.ArrayIndex : CoflowRegisterOpCode.DictionaryIndex,
                    Operation: new CoflowRegisterArrayIndexSite(
                        Top(2), Top(), Output(valueType, 2))),
                CoflowOpCode.CollectionCount => CollectionRead(
                    CoflowRegisterOpCode.CollectionCount, Top(), null, Output(valueType, 1)),
                CoflowOpCode.ArrayItem => CollectionRead(
                    CoflowRegisterOpCode.ArrayItem, Top(2), Top(), Output(valueType, 2)),
                CoflowOpCode.DictionaryKey => CollectionRead(
                    CoflowRegisterOpCode.DictionaryKey, Top(2), Top(), Output(valueType, 2)),
                CoflowOpCode.DictionaryValue => CollectionRead(
                    CoflowRegisterOpCode.DictionaryValue, Top(2), Top(), Output(valueType, 2)),
                CoflowOpCode.DictionaryKeys or CoflowOpCode.DictionaryValues => new(
                    source.Code == CoflowOpCode.DictionaryKeys
                        ? CoflowRegisterOpCode.DictionaryKeys : CoflowRegisterOpCode.DictionaryValues,
                    Operation: new CoflowRegisterCollectionProjectionSite(
                        Top(), Output(valueType, 1))),
                CoflowOpCode.CollectionBuiltin => LowerCollectionBuiltin(
                    (CoflowBuiltin)Operation(), valueType),
                CoflowOpCode.BeginArrayBuilder => ArrayBuilder(
                    CoflowRegisterOpCode.BeginArrayBuilder, Top(), null, Output(valueType, 1)),
                CoflowOpCode.AppendArrayBuilder => ArrayBuilder(
                    CoflowRegisterOpCode.AppendArrayBuilder, Top(2), Top(), Output(valueType, 2)),
                CoflowOpCode.MakeOptionNone => new(
                    CoflowRegisterOpCode.MakeOptionNone,
                    Operation: new CoflowRegisterTargetSite(Output(valueType, 0))),
                CoflowOpCode.MakeOptionSome => Transfer(
                    CoflowRegisterOpCode.MakeOptionSome, Top(), Output(valueType, 1)),
                CoflowOpCode.MakeResultOk => Transfer(
                    CoflowRegisterOpCode.MakeResultOk, Top(), Output(valueType, 1)),
                CoflowOpCode.MakeResultErr => Transfer(
                    CoflowRegisterOpCode.MakeResultErr, Top(), Output(valueType, 1)),
                CoflowOpCode.ReadValueTag => new(
                    CoflowRegisterOpCode.ReadValueTag,
                    Output(typeof(bool), 1).IntegerBase,
                    Top().IntegerBase),
                CoflowOpCode.ReadFirstPayload => Transfer(
                    CoflowRegisterOpCode.ReadFirstPayload, Top().First, Output(valueType, 1)),
                CoflowOpCode.ReadSecondPayload => Transfer(
                    CoflowRegisterOpCode.ReadSecondPayload, Top().Second, Output(valueType, 1)),
                CoflowOpCode.Propagate => new(
                    CoflowRegisterOpCode.Propagate,
                    Operation: new CoflowRegisterPropagateSite(
                        Top(), Output(valueType, 1), returnValue)),
                CoflowOpCode.MakeClosure => Closure(
                    (CoflowClosureTemplate)Operation(),
                    Arguments(((CoflowClosureTemplate)Operation()).CaptureCount),
                    Output(valueType, ((CoflowClosureTemplate)Operation()).CaptureCount)),
                CoflowOpCode.Pop => new(CoflowRegisterOpCode.Nop),
                CoflowOpCode.Reinterpret => Transfer(
                    CoflowRegisterOpCode.MoveValue, Top(), Output(valueType, 1)),
                CoflowOpCode.ConvertIntToFloat => new(
                    CoflowRegisterOpCode.ConvertIntToFloat,
                    Output(typeof(double), 1).FloatBase,
                    Top().IntegerBase),
                CoflowOpCode.ConvertFloatToInt => new(
                    CoflowRegisterOpCode.ConvertFloatToInt,
                    Output(typeof(long), 1).IntegerBase,
                    Top().FloatBase),
                CoflowOpCode.IsType => new(
                    Top().Shape.Kind == CoflowValueShapeKind.Record
                        ? CoflowRegisterOpCode.IsArenaType : CoflowRegisterOpCode.IsType,
                    Output(typeof(bool), 1).IntegerBase,
                    Top().Shape.Kind == CoflowValueShapeKind.Record
                        ? Top().IntegerBase : Top().ReferenceBase,
                    Operation: (Type)Operation()),
                CoflowOpCode.NegateInt or CoflowOpCode.Not or CoflowOpCode.BitNot => IntegerUnary(
                    source.Code, Top().IntegerBase, Output(valueType, 1).IntegerBase),
                CoflowOpCode.NegateFloat => new(
                    CoflowRegisterOpCode.NegateFloat,
                    Output(valueType, 1).FloatBase,
                    Top().FloatBase),
                CoflowOpCode.AddInt or CoflowOpCode.SubtractInt or CoflowOpCode.MultiplyInt or
                CoflowOpCode.DivideInt or CoflowOpCode.IntegerDivide or CoflowOpCode.Remainder or
                CoflowOpCode.PowerInt or CoflowOpCode.ShiftLeft or CoflowOpCode.ShiftRight or
                CoflowOpCode.BitAnd or CoflowOpCode.BitXor or CoflowOpCode.BitOr => IntegerBinary(
                    source.Code,
                    Output(valueType, 2).IntegerBase,
                    Top(2).IntegerBase,
                    Top().IntegerBase),
                CoflowOpCode.AddFloat or CoflowOpCode.SubtractFloat or CoflowOpCode.MultiplyFloat or
                CoflowOpCode.DivideFloat or CoflowOpCode.PowerFloat => FloatBinary(
                    source.Code,
                    Output(valueType, 2).FloatBase,
                    Top(2).FloatBase,
                    Top().FloatBase),
                CoflowOpCode.AddString => new(
                    CoflowRegisterOpCode.AddString,
                    Output(typeof(string), 2).ReferenceBase,
                    Top(2).ReferenceBase,
                    Top().ReferenceBase),
                CoflowOpCode.LessInt or CoflowOpCode.LessOrEqualInt or CoflowOpCode.GreaterInt or
                CoflowOpCode.GreaterOrEqualInt or CoflowOpCode.EqualInteger => IntegerComparison(
                    source.Code,
                    Output(typeof(bool), 2).IntegerBase,
                    Top(2).IntegerBase,
                    Top().IntegerBase),
                CoflowOpCode.LessFloat or CoflowOpCode.LessOrEqualFloat or CoflowOpCode.GreaterFloat or
                CoflowOpCode.GreaterOrEqualFloat or CoflowOpCode.EqualFloat => FloatComparison(
                    source.Code,
                    Output(typeof(bool), 2).IntegerBase,
                    Top(2).FloatBase,
                    Top().FloatBase),
                CoflowOpCode.LessString or CoflowOpCode.LessOrEqualString or CoflowOpCode.GreaterString or
                CoflowOpCode.GreaterOrEqualString => StringComparison(
                    source.Code,
                    Output(typeof(bool), 2).IntegerBase,
                    Top(2).ReferenceBase,
                    Top().ReferenceBase),
                CoflowOpCode.EqualReference => new(
                    CoflowRegisterOpCode.EqualReference,
                    Output(typeof(bool), 2).IntegerBase,
                    Top(2).ReferenceBase,
                    Top().ReferenceBase),
                CoflowOpCode.JumpIfFalseKeep or CoflowOpCode.JumpIfFalse => new(
                    CoflowRegisterOpCode.JumpIfFalse, Top().IntegerBase, source.Operand),
                CoflowOpCode.JumpIfTrueKeep => new(
                    CoflowRegisterOpCode.JumpIfTrue, Top().IntegerBase, source.Operand),
                CoflowOpCode.Jump => new(CoflowRegisterOpCode.Jump, source.Operand),
                CoflowOpCode.Call => DirectCall(
                    CoflowRegisterOpCode.Call,
                    (CoflowCallSite)Operation(),
                    ResolveProgramIndex(program, (CoflowCallSite)Operation(), pc),
                    Arguments(((CoflowCallSite)Operation()).ArgumentCount),
                    Output(((CoflowCallSite)Operation()).Signature.ResultType,
                        ((CoflowCallSite)Operation()).ArgumentCount),
                    outgoingIntegerBase,
                    outgoingFloatBase,
                    outgoingReferenceBase),
                CoflowOpCode.CallIndirect => IndirectCall(
                    CoflowRegisterOpCode.CallIndirect,
                    Stack(depth - source.Operand - 1),
                    Arguments(source.Operand),
                    Output(valueType, source.Operand + 1),
                    valueType),
                CoflowOpCode.TailCall => DirectCall(
                    CoflowRegisterOpCode.TailCall,
                    (CoflowCallSite)Operation(),
                    ResolveProgramIndex(program, (CoflowCallSite)Operation(), pc),
                    Arguments(((CoflowCallSite)Operation()).ArgumentCount),
                    returnValue,
                    outgoingIntegerBase,
                    outgoingFloatBase,
                    outgoingReferenceBase),
                CoflowOpCode.TailCallIndirect => IndirectCall(
                    CoflowRegisterOpCode.TailCallIndirect,
                    Stack(depth - source.Operand - 1),
                    Arguments(source.Operand),
                    returnValue,
                    program.ReturnType),
                CoflowOpCode.Return => new(
                    CoflowRegisterOpCode.Return,
                    Operation: new CoflowRegisterTargetSite(Top())),
                _ => throw Invalid(program, $"unknown opcode `{source.Code}`"),
            };
        }
        PlaceDirectCallArguments(result);
        OptimizeScalarMoves(result);
        return Compact(result, program.InstructionSpans);
    }

    private static void PlaceDirectCallArguments(CoflowLoweredInstruction[] instructions)
    {
        for (var callIndex = 0; callIndex < instructions.Length; callIndex++)
        {
            if (instructions[callIndex].Operation is not CoflowRegisterCallSite site) continue;
            for (var argumentIndex = 0; argumentIndex < site.SourceArguments.Length; argumentIndex++)
            {
                var source = site.SourceArguments[argumentIndex];
                var target = site.Arguments[argumentIndex];
                if (source.Shape.Kind != CoflowValueShapeKind.Scalar) continue;
                var kind = source.Shape.ScalarKind!.Value;
                var sourceIndex = kind switch
                {
                    CoflowRegisterKind.Integer => source.IntegerBase,
                    CoflowRegisterKind.Float => source.FloatBase,
                    _ => source.ReferenceBase,
                };
                for (var scan = callIndex - 1; scan >= 0; scan--)
                {
                    var instruction = instructions[scan];
                    if (IsControlBoundary(instruction.Code) || instruction.Operation is not null) break;
                    if (WrittenRegister(instruction.Code, kind, instruction.A) == sourceIndex)
                    {
                        var targetIndex = kind switch
                        {
                            CoflowRegisterKind.Integer => target.IntegerBase,
                            CoflowRegisterKind.Float => target.FloatBase,
                            _ => target.ReferenceBase,
                        };
                        instructions[scan] = instruction with { A = targetIndex };
                        site.CopyArguments[argumentIndex] = false;
                        break;
                    }
                    if (ReadsRegister(instruction, kind, sourceIndex)) break;
                }
            }
        }
    }

    private static bool ReadsRegister(
        CoflowLoweredInstruction instruction,
        CoflowRegisterKind kind,
        int register) => RewriteReads(instruction, kind, register, int.MinValue) != instruction;

    private static void OptimizeScalarMoves(CoflowLoweredInstruction[] instructions)
    {
        bool changed;
        do
        {
            changed = false;
            for (var index = 0; index < instructions.Length; index++)
            {
                var move = instructions[index];
                var kind = move.Code switch
                {
                    CoflowRegisterOpCode.MoveInteger => CoflowRegisterKind.Integer,
                    CoflowRegisterOpCode.MoveFloat => CoflowRegisterKind.Float,
                    CoflowRegisterOpCode.MoveReference => CoflowRegisterKind.Reference,
                    _ => (CoflowRegisterKind?)null,
                };
                if (kind is null) continue;
                if (move.A == move.B)
                {
                    instructions[index] = new(CoflowRegisterOpCode.Nop);
                    changed = true;
                    continue;
                }

                var rewrites = new List<(int Index, CoflowLoweredInstruction Instruction)>();
                var eliminated = false;
                for (var scan = index + 1; scan < instructions.Length; scan++)
                {
                    var instruction = instructions[scan];
                    if (instruction.Operation is not null || IsControlBoundary(instruction.Code)) break;
                    var rewritten = RewriteReads(instruction, kind.Value, move.A, move.B);
                    if (rewritten != instruction) rewrites.Add((scan, rewritten));

                    var written = WrittenRegister(instruction.Code, kind.Value, instruction.A);
                    if (written == move.B) break;
                    if (written != move.A) continue;
                    eliminated = true;
                    break;
                }
                if (!eliminated) continue;
                foreach (var rewrite in rewrites) instructions[rewrite.Index] = rewrite.Instruction;
                instructions[index] = new(CoflowRegisterOpCode.Nop);
                changed = true;
            }
        } while (changed);
    }

    private static bool IsControlBoundary(CoflowRegisterOpCode code) => code is
        CoflowRegisterOpCode.JumpIfFalse or CoflowRegisterOpCode.JumpIfTrue or
        CoflowRegisterOpCode.Jump or CoflowRegisterOpCode.Call or
        CoflowRegisterOpCode.CallIndirect or CoflowRegisterOpCode.TailCall or
        CoflowRegisterOpCode.TailCallIndirect or CoflowRegisterOpCode.Return;

    private static int WrittenRegister(
        CoflowRegisterOpCode code,
        CoflowRegisterKind kind,
        int target) => kind switch
        {
            CoflowRegisterKind.Integer when code is
                CoflowRegisterOpCode.ConstantInteger or CoflowRegisterOpCode.MoveInteger or
                CoflowRegisterOpCode.LoadHostFieldInteger or CoflowRegisterOpCode.LoadArenaFieldInteger or
                CoflowRegisterOpCode.LoadArenaFieldValue or CoflowRegisterOpCode.ReadValueTag or
                CoflowRegisterOpCode.ConvertFloatToInt or CoflowRegisterOpCode.IsType or CoflowRegisterOpCode.IsArenaType or
                CoflowRegisterOpCode.NegateInt or CoflowRegisterOpCode.Not or CoflowRegisterOpCode.BitNot or
                CoflowRegisterOpCode.AddInt or CoflowRegisterOpCode.SubtractInt or
                CoflowRegisterOpCode.MultiplyInt or CoflowRegisterOpCode.DivideInt or
                CoflowRegisterOpCode.IntegerDivide or CoflowRegisterOpCode.Remainder or
                CoflowRegisterOpCode.PowerInt or CoflowRegisterOpCode.ShiftLeft or
                CoflowRegisterOpCode.ShiftRight or CoflowRegisterOpCode.BitAnd or
                CoflowRegisterOpCode.BitXor or CoflowRegisterOpCode.BitOr or
                CoflowRegisterOpCode.LessInt or CoflowRegisterOpCode.LessOrEqualInt or
                CoflowRegisterOpCode.GreaterInt or CoflowRegisterOpCode.GreaterOrEqualInt or
                CoflowRegisterOpCode.EqualInteger or CoflowRegisterOpCode.LessFloat or
                CoflowRegisterOpCode.LessOrEqualFloat or CoflowRegisterOpCode.GreaterFloat or
                CoflowRegisterOpCode.GreaterOrEqualFloat or CoflowRegisterOpCode.EqualFloat or
                CoflowRegisterOpCode.LessString or CoflowRegisterOpCode.LessOrEqualString or
                CoflowRegisterOpCode.GreaterString or CoflowRegisterOpCode.GreaterOrEqualString or
                CoflowRegisterOpCode.EqualReference => target,
            CoflowRegisterKind.Float when code is
                CoflowRegisterOpCode.ConstantFloat or CoflowRegisterOpCode.MoveFloat or
                CoflowRegisterOpCode.LoadHostFieldFloat or CoflowRegisterOpCode.LoadArenaFieldFloat or CoflowRegisterOpCode.ConvertIntToFloat or
                CoflowRegisterOpCode.NegateFloat or CoflowRegisterOpCode.AddFloat or
                CoflowRegisterOpCode.SubtractFloat or CoflowRegisterOpCode.MultiplyFloat or
                CoflowRegisterOpCode.DivideFloat or CoflowRegisterOpCode.PowerFloat => target,
            CoflowRegisterKind.Reference when code is
                CoflowRegisterOpCode.ConstantReference or CoflowRegisterOpCode.MoveReference or
                CoflowRegisterOpCode.LoadHostFieldReference or CoflowRegisterOpCode.LoadArenaFieldReference or CoflowRegisterOpCode.AddString => target,
            _ => -1,
        };

    private static CoflowLoweredInstruction RewriteReads(
        CoflowLoweredInstruction instruction,
        CoflowRegisterKind kind,
        int target,
        int source)
    {
        var readB = kind switch
        {
            CoflowRegisterKind.Integer => instruction.Code is
                CoflowRegisterOpCode.MoveInteger or CoflowRegisterOpCode.LoadArenaFieldInteger or
                CoflowRegisterOpCode.LoadArenaFieldFloat or CoflowRegisterOpCode.LoadArenaFieldReference or
                CoflowRegisterOpCode.IsArenaType or CoflowRegisterOpCode.ReadValueTag or
                CoflowRegisterOpCode.ConvertIntToFloat or CoflowRegisterOpCode.NegateInt or
                CoflowRegisterOpCode.Not or CoflowRegisterOpCode.BitNot or
                CoflowRegisterOpCode.AddInt or CoflowRegisterOpCode.SubtractInt or
                CoflowRegisterOpCode.MultiplyInt or CoflowRegisterOpCode.DivideInt or
                CoflowRegisterOpCode.IntegerDivide or CoflowRegisterOpCode.Remainder or
                CoflowRegisterOpCode.PowerInt or CoflowRegisterOpCode.ShiftLeft or
                CoflowRegisterOpCode.ShiftRight or CoflowRegisterOpCode.BitAnd or
                CoflowRegisterOpCode.BitXor or CoflowRegisterOpCode.BitOr or
                CoflowRegisterOpCode.LessInt or CoflowRegisterOpCode.LessOrEqualInt or
                CoflowRegisterOpCode.GreaterInt or CoflowRegisterOpCode.GreaterOrEqualInt or
                CoflowRegisterOpCode.EqualInteger,
            CoflowRegisterKind.Float => instruction.Code is
                CoflowRegisterOpCode.MoveFloat or CoflowRegisterOpCode.ConvertFloatToInt or
                CoflowRegisterOpCode.NegateFloat or CoflowRegisterOpCode.AddFloat or
                CoflowRegisterOpCode.SubtractFloat or CoflowRegisterOpCode.MultiplyFloat or
                CoflowRegisterOpCode.DivideFloat or CoflowRegisterOpCode.PowerFloat or
                CoflowRegisterOpCode.LessFloat or CoflowRegisterOpCode.LessOrEqualFloat or
                CoflowRegisterOpCode.GreaterFloat or CoflowRegisterOpCode.GreaterOrEqualFloat or
                CoflowRegisterOpCode.EqualFloat,
            _ => instruction.Code is
                CoflowRegisterOpCode.MoveReference or CoflowRegisterOpCode.LoadHostFieldInteger or
                CoflowRegisterOpCode.LoadHostFieldFloat or CoflowRegisterOpCode.LoadHostFieldReference or
                CoflowRegisterOpCode.IsType or CoflowRegisterOpCode.AddString or
                CoflowRegisterOpCode.LessString or CoflowRegisterOpCode.LessOrEqualString or
                CoflowRegisterOpCode.GreaterString or CoflowRegisterOpCode.GreaterOrEqualString or
                CoflowRegisterOpCode.EqualReference,
        };
        var readC = kind switch
        {
            CoflowRegisterKind.Integer => instruction.Code is
                CoflowRegisterOpCode.AddInt or CoflowRegisterOpCode.SubtractInt or
                CoflowRegisterOpCode.MultiplyInt or CoflowRegisterOpCode.DivideInt or
                CoflowRegisterOpCode.IntegerDivide or CoflowRegisterOpCode.Remainder or
                CoflowRegisterOpCode.PowerInt or CoflowRegisterOpCode.ShiftLeft or
                CoflowRegisterOpCode.ShiftRight or CoflowRegisterOpCode.BitAnd or
                CoflowRegisterOpCode.BitXor or CoflowRegisterOpCode.BitOr or
                CoflowRegisterOpCode.LessInt or CoflowRegisterOpCode.LessOrEqualInt or
                CoflowRegisterOpCode.GreaterInt or CoflowRegisterOpCode.GreaterOrEqualInt or
                CoflowRegisterOpCode.EqualInteger,
            CoflowRegisterKind.Float => instruction.Code is
                CoflowRegisterOpCode.AddFloat or CoflowRegisterOpCode.SubtractFloat or
                CoflowRegisterOpCode.MultiplyFloat or CoflowRegisterOpCode.DivideFloat or
                CoflowRegisterOpCode.PowerFloat or CoflowRegisterOpCode.LessFloat or
                CoflowRegisterOpCode.LessOrEqualFloat or CoflowRegisterOpCode.GreaterFloat or
                CoflowRegisterOpCode.GreaterOrEqualFloat or CoflowRegisterOpCode.EqualFloat,
            _ => instruction.Code is
                CoflowRegisterOpCode.AddString or CoflowRegisterOpCode.LessString or
                CoflowRegisterOpCode.LessOrEqualString or CoflowRegisterOpCode.GreaterString or
                CoflowRegisterOpCode.GreaterOrEqualString or CoflowRegisterOpCode.EqualReference,
        };
        return instruction with
        {
            B = readB && instruction.B == target ? source : instruction.B,
            C = readC && instruction.C == target ? source : instruction.C,
        };
    }

    private static (
        CoflowRegisterInstruction[] Instructions,
        CfdSpan?[] InstructionSpans,
        long[] Immediates,
        CoflowRegisterOperations Operations) Compact(
        CoflowLoweredInstruction[] source,
        CfdSpan?[] sourceSpans)
    {
        var targets = new int[source.Length + 1];
        var count = 0;
        for (var index = 0; index < source.Length; index++)
        {
            targets[index] = count;
            if (source[index].Code != CoflowRegisterOpCode.Nop) count++;
        }
        targets[source.Length] = count;

        var instructions = new CoflowRegisterInstruction[count];
        var instructionSpans = new CfdSpan?[count];
        var immediates = new List<long>();
        var operations = new CoflowRegisterOperations.Builder();
        var target = 0;
        for (var index = 0; index < source.Length; index++)
        {
            var instruction = source[index];
            if (instruction.Code == CoflowRegisterOpCode.Nop) continue;
            instruction = instruction.Code switch
            {
                CoflowRegisterOpCode.Jump => instruction with { A = targets[instruction.A] },
                CoflowRegisterOpCode.JumpIfFalse or CoflowRegisterOpCode.JumpIfTrue =>
                    instruction with { B = targets[instruction.B] },
                _ => instruction,
            };
            var immediate = 0;
            if (instruction.Code is CoflowRegisterOpCode.ConstantInteger or CoflowRegisterOpCode.ConstantFloat)
            {
                immediate = immediates.Count;
                immediates.Add(instruction.Immediate);
            }
            var hasOperation = HasOperation(instruction.Code);
            var operation = 0;
            if (hasOperation)
            {
                operation = operations.Add(instruction.Code, instruction.Operation);
            }
            instructions[target] = new CoflowRegisterInstruction(
                instruction.Code,
                instruction.A,
                instruction.Code is CoflowRegisterOpCode.ConstantInteger or CoflowRegisterOpCode.ConstantFloat
                    ? immediate : instruction.B,
                hasOperation ? operation : instruction.C);
            instructionSpans[target] = sourceSpans[index];
            target++;
        }
        return (instructions, instructionSpans, immediates.ToArray(), operations.Build());
    }

    private static bool HasOperation(CoflowRegisterOpCode code) => code is
        CoflowRegisterOpCode.ConstantReference or CoflowRegisterOpCode.ConstantValue or
        CoflowRegisterOpCode.MoveValue or CoflowRegisterOpCode.LoadHostFieldInteger or
        CoflowRegisterOpCode.LoadHostFieldFloat or CoflowRegisterOpCode.LoadHostFieldReference or
        CoflowRegisterOpCode.LoadHostFieldValue or CoflowRegisterOpCode.LoadArenaFieldInteger or
        CoflowRegisterOpCode.LoadArenaFieldFloat or CoflowRegisterOpCode.LoadArenaFieldReference or
        CoflowRegisterOpCode.LoadArenaFieldValue or
        CoflowRegisterOpCode.Native or CoflowRegisterOpCode.MakeArray or
        CoflowRegisterOpCode.MakeDictionary or CoflowRegisterOpCode.ArrayIndex or
        CoflowRegisterOpCode.DictionaryIndex or
        CoflowRegisterOpCode.CollectionCount or CoflowRegisterOpCode.ArrayItem or
        CoflowRegisterOpCode.DictionaryKey or CoflowRegisterOpCode.DictionaryValue or
        CoflowRegisterOpCode.DictionaryKeys or CoflowRegisterOpCode.DictionaryValues or
        CoflowRegisterOpCode.CollectionBuiltin or
        CoflowRegisterOpCode.BeginArrayBuilder or CoflowRegisterOpCode.AppendArrayBuilder or
        CoflowRegisterOpCode.MakeOptionNone or
        CoflowRegisterOpCode.MakeOptionSome or CoflowRegisterOpCode.MakeResultOk or
        CoflowRegisterOpCode.MakeResultErr or CoflowRegisterOpCode.ReadFirstPayload or
        CoflowRegisterOpCode.ReadSecondPayload or CoflowRegisterOpCode.Propagate or
        CoflowRegisterOpCode.MakeClosure or CoflowRegisterOpCode.IsType or CoflowRegisterOpCode.IsArenaType or
        CoflowRegisterOpCode.Call or CoflowRegisterOpCode.CallIndirect or
        CoflowRegisterOpCode.TailCall or CoflowRegisterOpCode.TailCallIndirect or
        CoflowRegisterOpCode.Return;

    private static CoflowLoweredInstruction Constant(
        CoflowEncodedValue value,
        CoflowValueRegister target) => value.Shape.Kind switch
        {
            CoflowValueShapeKind.Unit => new(CoflowRegisterOpCode.Nop),
            CoflowValueShapeKind.Scalar when value.Shape.ScalarKind == CoflowRegisterKind.Integer =>
                new(CoflowRegisterOpCode.ConstantInteger, target.IntegerBase, Immediate: value.Integers[0]),
            CoflowValueShapeKind.Scalar when value.Shape.ScalarKind == CoflowRegisterKind.Float =>
                new(CoflowRegisterOpCode.ConstantFloat, target.FloatBase,
                    Immediate: BitConverter.DoubleToInt64Bits(value.Floats[0])),
            CoflowValueShapeKind.Scalar =>
                new(CoflowRegisterOpCode.ConstantReference, target.ReferenceBase, Operation: value.References[0]),
            _ => new(CoflowRegisterOpCode.ConstantValue,
                Operation: new CoflowRegisterConstantSite(value, target)),
        };

    private static CoflowLoweredInstruction Move(
        CoflowValueRegister source,
        CoflowValueRegister target)
    {
        if (source.Shape.Kind == CoflowValueShapeKind.Unit)
            return new(CoflowRegisterOpCode.Nop);
        if (source.Shape.Kind is not (CoflowValueShapeKind.Scalar or CoflowValueShapeKind.Collection))
            return Transfer(CoflowRegisterOpCode.MoveValue, source, target);
        return source.Shape.ScalarKind switch
        {
            CoflowRegisterKind.Integer => new(
                CoflowRegisterOpCode.MoveInteger, target.IntegerBase, source.IntegerBase),
            CoflowRegisterKind.Float => new(
                CoflowRegisterOpCode.MoveFloat, target.FloatBase, source.FloatBase),
            _ => new(CoflowRegisterOpCode.MoveReference, target.ReferenceBase, source.ReferenceBase),
        };
    }

    private static CoflowLoweredInstruction Transfer(
        CoflowRegisterOpCode code,
        CoflowValueRegister source,
        CoflowValueRegister target) => new(
            code,
            Operation: new CoflowRegisterValueTransfer(source, target));

    private static CoflowLoweredInstruction Field(
        CoflowFieldAccess access,
        CoflowValueRegister receiver,
        CoflowValueRegister target)
    {
        if (access.ReceiverIsStruct)
        {
            if (receiver.Shape.Kind != CoflowValueShapeKind.Struct)
                throw new InvalidOperationException($"field `{access.Name}` requires a struct receiver layout");
            if (access.IsFunction)
                return Native(access.Call, new[] { receiver }, target);
            var source = new CoflowValueRegister(
                target.Shape,
                receiver.IntegerBase + access.IntegerOffset,
                receiver.FloatBase + access.FloatOffset,
                receiver.ReferenceBase + access.ReferenceOffset);
            return Move(source, target);
        }
        if (!access.IsHost)
        {
            if (receiver.Shape.Kind != CoflowValueShapeKind.Record)
                throw new InvalidOperationException($"field `{access.Name}` requires a schema ValueId receiver");
            if (access.IsFunction)
                return Native(access.Call, new[] { receiver }, target);
            if (target.Shape.Kind is CoflowValueShapeKind.Scalar or CoflowValueShapeKind.Collection or
                CoflowValueShapeKind.Record)
            {
                return target.Shape.ScalarKind switch
                {
                    CoflowRegisterKind.Integer => new(CoflowRegisterOpCode.LoadArenaFieldInteger,
                        target.IntegerBase, receiver.IntegerBase, Operation: access),
                    CoflowRegisterKind.Float => new(CoflowRegisterOpCode.LoadArenaFieldFloat,
                        target.FloatBase, receiver.IntegerBase, Operation: access),
                    _ => new(CoflowRegisterOpCode.LoadArenaFieldReference,
                        target.ReferenceBase, receiver.IntegerBase, Operation: access),
                };
            }
            return new(CoflowRegisterOpCode.LoadArenaFieldValue, receiver.IntegerBase,
                Operation: new CoflowRegisterFieldValueSite(access, target));
        }
        if (access.ReadInteger is not null)
            return new(CoflowRegisterOpCode.LoadHostFieldInteger,
                target.IntegerBase, receiver.ReferenceBase, Operation: access);
        if (access.ReadFloat is not null)
            return new(CoflowRegisterOpCode.LoadHostFieldFloat,
                target.FloatBase, receiver.ReferenceBase, Operation: access);
        if (access.ReadReference is not null)
            return new(CoflowRegisterOpCode.LoadHostFieldReference,
                target.ReferenceBase, receiver.ReferenceBase, Operation: access);
        if (access.ReadValue is not null)
        {
            return new(CoflowRegisterOpCode.LoadHostFieldValue,
                receiver.ReferenceBase, Operation: new CoflowRegisterFieldValueSite(access, target));
        }
        return Native(access.Call, new[] { receiver }, target);
    }

    private static CoflowLoweredInstruction Native(
        CoflowNativeCall call,
        CoflowValueRegister[] arguments,
        CoflowValueRegister result) => new(
            CoflowRegisterOpCode.Native,
            Operation: new CoflowNativeCallSite(call, arguments, result));

    private static CoflowLoweredInstruction Collection(
        CoflowRegisterOpCode code,
        CoflowValueRegister[] first,
        CoflowValueRegister[]? second,
        CoflowValueRegister target) => new(code,
            Operation: new CoflowRegisterCollectionSite(first, second, target));

    private static CoflowLoweredInstruction Dictionary(
        CoflowValueRegister[] entries,
        CoflowValueRegister target)
    {
        var keys = new CoflowValueRegister[entries.Length / 2];
        var values = new CoflowValueRegister[keys.Length];
        for (var index = 0; index < keys.Length; index++)
        {
            keys[index] = entries[index * 2];
            values[index] = entries[index * 2 + 1];
        }
        return Collection(CoflowRegisterOpCode.MakeDictionary, keys, values, target);
    }

    private static CoflowLoweredInstruction CollectionRead(
        CoflowRegisterOpCode code,
        CoflowValueRegister collection,
        CoflowValueRegister? index,
        CoflowValueRegister target) => new(code,
            Operation: new CoflowRegisterCollectionReadSite(collection, index, target));

    private static CoflowLoweredInstruction ArrayBuilder(
        CoflowRegisterOpCode code,
        CoflowValueRegister collectionOrCapacity,
        CoflowValueRegister? item,
        CoflowValueRegister target) => new(code,
            Operation: new CoflowRegisterArrayBuilderSite(collectionOrCapacity, item, target));

    private static CoflowLoweredInstruction Closure(
        CoflowClosureTemplate template,
        CoflowValueRegister[] captures,
        CoflowValueRegister target) => new(
            CoflowRegisterOpCode.MakeClosure,
            Operation: new CoflowRegisterClosureSite(template, captures, target));

    private static CoflowLoweredInstruction DirectCall(
        CoflowRegisterOpCode code,
        CoflowCallSite call,
        int programIndex,
        CoflowValueRegister[] arguments,
        CoflowValueRegister result,
        int integerWindowBase,
        int floatWindowBase,
        int referenceWindowBase)
    {
        var window = AllocateAt(
            call.VmParameterTypes,
            integerWindowBase,
            floatWindowBase,
            referenceWindowBase);
        return new(code, Operation: new CoflowRegisterCallSite(
            programIndex,
            call.Signature,
            arguments,
            window,
            Enumerable.Repeat(true, arguments.Length).ToArray(),
            result,
            integerWindowBase,
            floatWindowBase,
            referenceWindowBase));
    }

    private static int ResolveProgramIndex(
        CoflowLoweringInput program,
        CoflowCallSite call,
        int pc)
    {
        if (!program.FunctionIndexes.TryGetValue(call.Identity, out var programIndex))
            throw new CoflowProgramLinkException($"unknown function `{call.Identity}`");
        return programIndex;
    }

    private static CoflowValueRegister[] AllocateAt(
        IReadOnlyList<Type> types,
        int integer,
        int floating,
        int reference)
    {
        var result = new CoflowValueRegister[types.Count];
        for (var index = 0; index < types.Count; index++)
            result[index] = Allocate(types[index], ref integer, ref floating, ref reference);
        return result;
    }

    private static CoflowLoweredInstruction IndirectCall(
        CoflowRegisterOpCode code,
        CoflowValueRegister callable,
        CoflowValueRegister[] arguments,
        CoflowValueRegister result,
        Type resultType) => new(
            code,
            Operation: new CoflowRegisterIndirectCallSite(
                callable, arguments, result, resultType));

    private static CoflowLoweredInstruction IntegerUnary(
        CoflowOpCode code,
        int source,
        int target) => new(code switch
        {
            CoflowOpCode.NegateInt => CoflowRegisterOpCode.NegateInt,
            CoflowOpCode.Not => CoflowRegisterOpCode.Not,
            _ => CoflowRegisterOpCode.BitNot,
        }, target, source);

    private static CoflowLoweredInstruction IntegerBinary(
        CoflowOpCode code,
        int target,
        int left,
        int right) => new(code switch
        {
            CoflowOpCode.AddInt => CoflowRegisterOpCode.AddInt,
            CoflowOpCode.SubtractInt => CoflowRegisterOpCode.SubtractInt,
            CoflowOpCode.MultiplyInt => CoflowRegisterOpCode.MultiplyInt,
            CoflowOpCode.DivideInt => CoflowRegisterOpCode.DivideInt,
            CoflowOpCode.IntegerDivide => CoflowRegisterOpCode.IntegerDivide,
            CoflowOpCode.Remainder => CoflowRegisterOpCode.Remainder,
            CoflowOpCode.PowerInt => CoflowRegisterOpCode.PowerInt,
            CoflowOpCode.ShiftLeft => CoflowRegisterOpCode.ShiftLeft,
            CoflowOpCode.ShiftRight => CoflowRegisterOpCode.ShiftRight,
            CoflowOpCode.BitAnd => CoflowRegisterOpCode.BitAnd,
            CoflowOpCode.BitXor => CoflowRegisterOpCode.BitXor,
            _ => CoflowRegisterOpCode.BitOr,
        }, target, left, right);

    private static CoflowLoweredInstruction FloatBinary(
        CoflowOpCode code,
        int target,
        int left,
        int right) => new(code switch
        {
            CoflowOpCode.AddFloat => CoflowRegisterOpCode.AddFloat,
            CoflowOpCode.SubtractFloat => CoflowRegisterOpCode.SubtractFloat,
            CoflowOpCode.MultiplyFloat => CoflowRegisterOpCode.MultiplyFloat,
            CoflowOpCode.DivideFloat => CoflowRegisterOpCode.DivideFloat,
            _ => CoflowRegisterOpCode.PowerFloat,
        }, target, left, right);

    private static CoflowLoweredInstruction IntegerComparison(
        CoflowOpCode code,
        int target,
        int left,
        int right) => new(code switch
        {
            CoflowOpCode.LessInt => CoflowRegisterOpCode.LessInt,
            CoflowOpCode.LessOrEqualInt => CoflowRegisterOpCode.LessOrEqualInt,
            CoflowOpCode.GreaterInt => CoflowRegisterOpCode.GreaterInt,
            CoflowOpCode.GreaterOrEqualInt => CoflowRegisterOpCode.GreaterOrEqualInt,
            _ => CoflowRegisterOpCode.EqualInteger,
        }, target, left, right);

    private static CoflowLoweredInstruction FloatComparison(
        CoflowOpCode code,
        int target,
        int left,
        int right) => new(code switch
        {
            CoflowOpCode.LessFloat => CoflowRegisterOpCode.LessFloat,
            CoflowOpCode.LessOrEqualFloat => CoflowRegisterOpCode.LessOrEqualFloat,
            CoflowOpCode.GreaterFloat => CoflowRegisterOpCode.GreaterFloat,
            CoflowOpCode.GreaterOrEqualFloat => CoflowRegisterOpCode.GreaterOrEqualFloat,
            _ => CoflowRegisterOpCode.EqualFloat,
        }, target, left, right);

    private static CoflowLoweredInstruction StringComparison(
        CoflowOpCode code,
        int target,
        int left,
        int right) => new(code switch
        {
            CoflowOpCode.LessString => CoflowRegisterOpCode.LessString,
            CoflowOpCode.LessOrEqualString => CoflowRegisterOpCode.LessOrEqualString,
            CoflowOpCode.GreaterString => CoflowRegisterOpCode.GreaterString,
            _ => CoflowRegisterOpCode.GreaterOrEqualString,
        }, target, left, right);

    private static IEnumerable<(int Pc, Type[] Stack)> Transfer(
        CoflowLoweringInput program,
        int pc,
        Type[] input,
        Type?[] locals)
    {
        var instruction = program.Instructions[pc];
        var stack = input.ToList();
        Type Pop()
        {
            if (stack.Count == 0) throw Invalid(program, $"stack underflow at instruction {pc}");
            var value = stack[^1];
            stack.RemoveAt(stack.Count - 1);
            return value;
        }
        int Index(int value, int count, string kind)
        {
            if ((uint)value >= (uint)count)
                throw Invalid(program, $"instruction {pc} has an invalid {kind} index {value}");
            return value;
        }
        T Operation<T>() where T : notnull
        {
            var index = Index(instruction.Operand, program.Operations.Length, "operation");
            return program.Operations[index] is T value
                ? value
                : throw Invalid(program, $"instruction {pc} has an invalid {typeof(T).Name} descriptor");
        }
        void RequireKind(Type actual, CoflowRegisterKind expected)
        {
            var shape = CoflowValueShape.Of(actual);
            if (shape.Kind != CoflowValueShapeKind.Scalar || shape.ScalarKind != expected)
                throw Invalid(program, $"instruction {pc} ({instruction.Code}) reads `{actual}` as {expected}");
        }
        void PopMany(int count) { for (var index = 0; index < count; index++) Pop(); }
        void PopArguments(IReadOnlyList<Type> expected)
        {
            for (var index = expected.Count - 1; index >= 0; index--)
            {
                var actual = Pop();
                if (actual != expected[index] && !expected[index].IsAssignableFrom(actual))
                    throw Invalid(program, $"instruction {pc} argument {index} expects `{expected[index]}`, found `{actual}`");
            }
        }
        var resultType = instruction.ValueType ?? typeof(object);
        switch (instruction.Code)
        {
            case CoflowOpCode.Constant:
                stack.Add(instruction.ValueType ?? program.EncodedConstants[
                    Index(instruction.Operand, program.EncodedConstants.Length, "constant")]?.Shape.Type ?? typeof(object));
                break;
            case CoflowOpCode.Argument:
                stack.Add(program.ParameterTypes[
                Index(instruction.Operand, program.ParameterTypes.Length, "argument")]); break;
            case CoflowOpCode.Local:
                stack.Add(locals[Index(instruction.Operand, locals.Length, "local")] ??
                    throw Invalid(program, $"local {instruction.Operand} is read before assignment"));
                break;
            case CoflowOpCode.StoreLocal:
                {
                    var type = Pop();
                    var local = Index(instruction.Operand, locals.Length, "local");
                    if (locals[local] is { } existing && existing != type)
                        throw Invalid(program, $"local {instruction.Operand} changes type from `{existing}` to `{type}`");
                    locals[local] = type;
                    break;
                }
            case CoflowOpCode.LoadField:
                {
                    var receiver = CoflowValueShape.Of(Pop());
                    if (receiver.Kind != CoflowValueShapeKind.Struct &&
                        receiver.Kind != CoflowValueShapeKind.Record &&
                        (receiver.Kind != CoflowValueShapeKind.Scalar ||
                         receiver.ScalarKind != CoflowRegisterKind.Reference))
                        throw Invalid(program,
                            $"instruction {pc} ({instruction.Code}) requires a reference or struct receiver");
                    var access = Operation<CoflowFieldAccess>();
                    if (access.ReceiverIsStruct)
                    {
                        if (receiver.Kind != CoflowValueShapeKind.Struct)
                            throw Invalid(program,
                                $"instruction {pc} ({instruction.Code}) requires a struct receiver");
                    }
                    else if (access.IsHost
                        ? receiver.Kind != CoflowValueShapeKind.Scalar ||
                          receiver.ScalarKind != CoflowRegisterKind.Reference
                        : receiver.Kind != CoflowValueShapeKind.Record)
                    {
                        throw Invalid(program,
                            $"instruction {pc} ({instruction.Code}) has an invalid receiver layout");
                    }
                    stack.Add(access.RuntimeType);
                    break;
                }
            case CoflowOpCode.MakeOptionSome:
            case CoflowOpCode.MakeResultOk:
            case CoflowOpCode.MakeResultErr:
                {
                    var source = Pop();
                    var target = CoflowValueShape.Of(resultType);
                    var payload = instruction.Code == CoflowOpCode.MakeResultErr ? target.Second : target.First;
                    if (target.Kind != (instruction.Code == CoflowOpCode.MakeOptionSome
                            ? CoflowValueShapeKind.Option : CoflowValueShapeKind.Result) || payload?.Type != source)
                        throw Invalid(program, $"instruction {pc} cannot construct `{resultType}` from `{source}`");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.ReadFirstPayload:
            case CoflowOpCode.ReadSecondPayload:
                {
                    var source = CoflowValueShape.Of(Pop());
                    var payload = instruction.Code == CoflowOpCode.ReadFirstPayload ? source.First : source.Second;
                    if (payload?.Type != resultType)
                        throw Invalid(program, $"instruction {pc} payload type does not match `{resultType}`");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.Propagate:
                {
                    var source = CoflowValueShape.Of(Pop());
                    var returned = CoflowValueShape.Of(program.ReturnType);
                    if (source.Kind is not (CoflowValueShapeKind.Option or CoflowValueShapeKind.Result) ||
                        source.First?.Type != resultType || source.Kind != returned.Kind ||
                        source.Kind == CoflowValueShapeKind.Result && source.Second?.Type != returned.Second?.Type)
                        throw Invalid(program, $"instruction {pc} has incompatible propagation layouts");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.MakeOptionNone:
                if (CoflowValueShape.Of(resultType).Kind != CoflowValueShapeKind.Option)
                    throw Invalid(program, $"instruction {pc} creates None with non-Option type `{resultType}`");
                stack.Add(resultType); break;
            case CoflowOpCode.ReadValueTag:
                {
                    var shape = CoflowValueShape.Of(Pop());
                    if (shape.Kind is not (CoflowValueShapeKind.Option or CoflowValueShapeKind.Result))
                        throw Invalid(program, $"instruction {pc} reads a tag from `{shape.Type}`");
                    stack.Add(typeof(bool));
                    break;
                }
            case CoflowOpCode.Reinterpret:
                {
                    var source = CoflowValueShape.Of(Pop());
                    var target = CoflowValueShape.Of(resultType);
                    // CollectionId 虽然占用 integer lane，但它不是可参与数值重解释的整数。
                    if ((source.Kind == CoflowValueShapeKind.Collection ||
                         target.Kind == CoflowValueShapeKind.Collection) &&
                        (source.Kind != CoflowValueShapeKind.Collection ||
                         target.Kind != CoflowValueShapeKind.Collection ||
                         source.Type != target.Type))
                        throw Invalid(program, $"instruction {pc} reinterprets a collection handle as `{resultType}`");
                    if (source.IntegerCount != target.IntegerCount || source.FloatCount != target.FloatCount ||
                        source.ReferenceCount != target.ReferenceCount)
                        throw Invalid(program, $"instruction {pc} reinterprets incompatible layouts");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.ConvertIntToFloat:
                RequireKind(Pop(), CoflowRegisterKind.Integer); stack.Add(typeof(double)); break;
            case CoflowOpCode.ConvertFloatToInt:
                RequireKind(Pop(), CoflowRegisterKind.Float); stack.Add(typeof(long)); break;
            case CoflowOpCode.IsType:
                _ = Operation<Type>();
                var tested = CoflowValueShape.Of(Pop());
                if (tested.Kind != CoflowValueShapeKind.Record &&
                    tested.ScalarKind != CoflowRegisterKind.Reference)
                    throw Invalid(program, $"instruction {pc} requires a schema record or reference value");
                stack.Add(typeof(bool)); break;
            case CoflowOpCode.MakeArray:
                {
                    var shape = CoflowValueShape.Of(resultType);
                    if (shape.Kind != CoflowValueShapeKind.Collection ||
                        resultType.GetGenericTypeDefinition() != typeof(IReadOnlyList<>))
                        throw Invalid(program, $"instruction {pc} creates an array with non-array type `{resultType}`");
                    var element = resultType.GetGenericArguments()[0];
                    for (var index = 0; index < instruction.Operand; index++)
                        if (Pop() != element)
                            throw Invalid(program, $"instruction {pc} array element does not match `{element}`");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.MakeDictionary:
                {
                    var shape = CoflowValueShape.Of(resultType);
                    if (shape.Kind != CoflowValueShapeKind.Collection ||
                        resultType.GetGenericTypeDefinition() != typeof(IReadOnlyDictionary<,>))
                        throw Invalid(program,
                            $"instruction {pc} creates a dictionary with non-dictionary type `{resultType}`");
                    var arguments = resultType.GetGenericArguments();
                    for (var index = 0; index < instruction.Operand; index++)
                    {
                        if (Pop() != arguments[1] || Pop() != arguments[0])
                            throw Invalid(program, $"instruction {pc} dictionary entry has an invalid type");
                    }
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.ArrayIndex:
            case CoflowOpCode.DictionaryIndex:
                {
                    var key = Pop();
                    var collection = Pop();
                    if (!collection.IsGenericType)
                        throw Invalid(program, $"instruction {pc} indexes a non-collection value `{collection}`");
                    var definition = collection.GetGenericTypeDefinition();
                    var arguments = collection.GetGenericArguments();
                    Type expected;
                    if (instruction.Code == CoflowOpCode.ArrayIndex)
                    {
                        RequireKind(key, CoflowRegisterKind.Integer);
                        if (definition != typeof(IReadOnlyList<>))
                            throw Invalid(program, $"instruction {pc} indexes a non-array value `{collection}`");
                        expected = typeof(Option<>).MakeGenericType(arguments[0]);
                    }
                    else
                    {
                        if (definition != typeof(IReadOnlyDictionary<,>) || key != arguments[0])
                            throw Invalid(program, $"instruction {pc} uses an invalid dictionary key `{key}`");
                        expected = typeof(Option<>).MakeGenericType(arguments[1]);
                    }
                    if (resultType != expected)
                        throw Invalid(program, $"instruction {pc} index result must be `{expected}`");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.CollectionCount:
                {
                    var collection = CoflowValueShape.Of(Pop());
                    if (collection.Kind != CoflowValueShapeKind.Collection || resultType != typeof(long))
                        throw Invalid(program, $"instruction {pc} requires a collection and returns int");
                    stack.Add(typeof(long));
                    break;
                }
            case CoflowOpCode.ArrayItem:
            case CoflowOpCode.DictionaryKey:
            case CoflowOpCode.DictionaryValue:
                {
                    RequireKind(Pop(), CoflowRegisterKind.Integer);
                    var collection = Pop();
                    if (!collection.IsGenericType)
                        throw Invalid(program, $"instruction {pc} reads a non-collection value `{collection}`");
                    var definition = collection.GetGenericTypeDefinition();
                    var arguments = collection.GetGenericArguments();
                    var expected = instruction.Code switch
                    {
                        CoflowOpCode.ArrayItem when definition == typeof(IReadOnlyList<>) => arguments[0],
                        CoflowOpCode.DictionaryKey when definition == typeof(IReadOnlyDictionary<,>) => arguments[0],
                        CoflowOpCode.DictionaryValue when definition == typeof(IReadOnlyDictionary<,>) => arguments[1],
                        _ => throw Invalid(program,
                            $"instruction {pc} uses `{instruction.Code}` with `{collection}`"),
                    };
                    if (resultType != expected)
                        throw Invalid(program, $"instruction {pc} collection item result must be `{expected}`");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.DictionaryKeys:
            case CoflowOpCode.DictionaryValues:
                {
                    var collection = Pop();
                    if (!collection.IsGenericType ||
                        collection.GetGenericTypeDefinition() != typeof(IReadOnlyDictionary<,>))
                        throw Invalid(program, $"instruction {pc} projects a non-dictionary `{collection}`");
                    var arguments = collection.GetGenericArguments();
                    var expectedElement = instruction.Code == CoflowOpCode.DictionaryKeys
                        ? arguments[0] : arguments[1];
                    if (!resultType.IsGenericType ||
                        resultType.GetGenericTypeDefinition() != typeof(IReadOnlyList<>) ||
                        resultType.GetGenericArguments()[0] != expectedElement)
                        throw Invalid(program, $"instruction {pc} dictionary projection result is incompatible");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.CollectionBuiltin:
                {
                    var builtin = Operation<CoflowBuiltin>();
                    if (builtin.Kind == CoflowBuiltinKind.Native || builtin.ResultType != resultType)
                        throw Invalid(program, $"instruction {pc} has an invalid collection builtin descriptor");
                    var argument = builtin.HasCollectionArgument ? Pop() : null;
                    var receiver = Pop();
                    if (!receiver.IsGenericType)
                        throw Invalid(program, $"instruction {pc} uses a non-collection builtin receiver");
                    var definition = receiver.GetGenericTypeDefinition();
                    var arguments = receiver.GetGenericArguments();
                    var element = definition == typeof(IReadOnlyList<>) ? arguments[0] : null;
                    Type? expectedArgument = builtin.Kind switch
                    {
                        CoflowBuiltinKind.CollectionContains when definition == typeof(IReadOnlyList<>) => arguments[0],
                        CoflowBuiltinKind.DictionaryContainsKey when definition == typeof(IReadOnlyDictionary<,>) => arguments[0],
                        CoflowBuiltinKind.DictionaryContainsValue when definition == typeof(IReadOnlyDictionary<,>) => arguments[1],
                        CoflowBuiltinKind.CollectionIntersects or CoflowBuiltinKind.CollectionDisjoint or
                            CoflowBuiltinKind.CollectionSubset or CoflowBuiltinKind.CollectionSuperset
                            when definition == typeof(IReadOnlyList<>) => receiver,
                        _ when !builtin.HasCollectionArgument && definition == typeof(IReadOnlyList<>) => null,
                        _ => throw Invalid(program, $"instruction {pc} collection builtin is incompatible with `{receiver}`"),
                    };
                    if (argument != expectedArgument)
                        throw Invalid(program, $"instruction {pc} collection builtin argument is incompatible");
                    var expectedResult = builtin.Kind switch
                    {
                        CoflowBuiltinKind.CollectionMin or CoflowBuiltinKind.CollectionMax
                            when element == typeof(long) || element == typeof(double) ||
                                element == typeof(string) || element?.IsEnum == true => element,
                        CoflowBuiltinKind.CollectionSumInteger when element == typeof(long) => typeof(long),
                        CoflowBuiltinKind.CollectionSumFloat when element == typeof(double) => typeof(double),
                        CoflowBuiltinKind.CollectionContains or CoflowBuiltinKind.DictionaryContainsKey or
                            CoflowBuiltinKind.DictionaryContainsValue or CoflowBuiltinKind.CollectionUnique or
                            CoflowBuiltinKind.CollectionSorted or CoflowBuiltinKind.CollectionStrictlySorted or
                            CoflowBuiltinKind.CollectionIntersects or CoflowBuiltinKind.CollectionDisjoint or
                            CoflowBuiltinKind.CollectionSubset or CoflowBuiltinKind.CollectionSuperset => typeof(bool),
                        _ => throw Invalid(program, $"instruction {pc} has an invalid collection builtin operation"),
                    };
                    if (resultType != expectedResult)
                        throw Invalid(program, $"instruction {pc} collection builtin result is incompatible");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.BeginArrayBuilder:
                {
                    RequireKind(Pop(), CoflowRegisterKind.Integer);
                    if (!resultType.IsGenericType ||
                        resultType.GetGenericTypeDefinition() != typeof(IReadOnlyList<>))
                        throw Invalid(program, $"instruction {pc} creates a builder for non-array `{resultType}`");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.AppendArrayBuilder:
                {
                    var item = Pop();
                    var collection = Pop();
                    if (!collection.IsGenericType ||
                        collection.GetGenericTypeDefinition() != typeof(IReadOnlyList<>) ||
                        collection.GetGenericArguments()[0] != item || resultType != typeof(Unit))
                        throw Invalid(program, $"instruction {pc} appends an incompatible array item");
                    stack.Add(typeof(Unit));
                    break;
                }
            case CoflowOpCode.Native:
                {
                    var call = Operation<CoflowNativeCall>();
                    if (call.ResultType != resultType)
                        throw Invalid(program, $"instruction {pc} native result type does not match `{resultType}`");
                    for (var index = call.ArgumentCount - 1; index >= 0; index--)
                    {
                        var actual = Pop();
                        if (actual != call.ParameterTypes[index] &&
                            !(call.ParameterTypes[index].IsAssignableFrom(actual) &&
                                CoflowValueShape.Scalar(actual) == CoflowRegisterKind.Reference))
                            throw Invalid(program, $"instruction {pc} native argument {index} expects `{call.ParameterTypes[index]}`, found `{actual}`");
                    }
                    stack.Add(resultType); break;
                }
            case CoflowOpCode.MakeClosure:
                {
                    var closure = Operation<CoflowClosureTemplate>();
                    if (closure.CaptureCount < 0 || closure.CaptureCount > closure.Program.ParameterCount ||
                        closure.CaptureCount > stack.Count)
                        throw Invalid(program, $"instruction {pc} has an invalid closure capture count");
                    var captures = stack.Skip(stack.Count - closure.CaptureCount).ToArray();
                    PopMany(closure.CaptureCount);
                    var expectedCaptures = closure.Program.ParameterTypes
                        .Skip(closure.Program.ParameterCount - closure.CaptureCount).ToArray();
                    if (!captures.SequenceEqual(expectedCaptures))
                        throw Invalid(program, $"instruction {pc} closure capture signature does not match target");
                    stack.Add(instruction.ValueType ?? typeof(Delegate)); break;
                }
            case CoflowOpCode.Pop: Pop(); break;
            case CoflowOpCode.NegateInt:
            case CoflowOpCode.Not:
            case CoflowOpCode.BitNot: RequireKind(Pop(), CoflowRegisterKind.Integer); stack.Add(resultType); break;
            case CoflowOpCode.NegateFloat: RequireKind(Pop(), CoflowRegisterKind.Float); stack.Add(resultType); break;
            case CoflowOpCode.AddInt:
            case CoflowOpCode.SubtractInt:
            case CoflowOpCode.MultiplyInt:
            case CoflowOpCode.DivideInt:
            case CoflowOpCode.IntegerDivide:
            case CoflowOpCode.Remainder:
            case CoflowOpCode.PowerInt:
            case CoflowOpCode.ShiftLeft:
            case CoflowOpCode.ShiftRight:
            case CoflowOpCode.BitAnd:
            case CoflowOpCode.BitXor:
            case CoflowOpCode.BitOr:
                RequireKind(Pop(), CoflowRegisterKind.Integer); RequireKind(Pop(), CoflowRegisterKind.Integer);
                stack.Add(resultType); break;
            case CoflowOpCode.AddFloat:
            case CoflowOpCode.SubtractFloat:
            case CoflowOpCode.MultiplyFloat:
            case CoflowOpCode.DivideFloat:
            case CoflowOpCode.PowerFloat:
                RequireKind(Pop(), CoflowRegisterKind.Float); RequireKind(Pop(), CoflowRegisterKind.Float);
                stack.Add(resultType); break;
            case CoflowOpCode.AddString:
                RequireKind(Pop(), CoflowRegisterKind.Reference); RequireKind(Pop(), CoflowRegisterKind.Reference);
                stack.Add(typeof(string)); break;
            case CoflowOpCode.LessInt:
            case CoflowOpCode.LessOrEqualInt:
            case CoflowOpCode.GreaterInt:
            case CoflowOpCode.GreaterOrEqualInt:
                RequireKind(Pop(), CoflowRegisterKind.Integer); RequireKind(Pop(), CoflowRegisterKind.Integer);
                stack.Add(typeof(bool)); break;
            case CoflowOpCode.LessFloat:
            case CoflowOpCode.LessOrEqualFloat:
            case CoflowOpCode.GreaterFloat:
            case CoflowOpCode.GreaterOrEqualFloat:
                RequireKind(Pop(), CoflowRegisterKind.Float); RequireKind(Pop(), CoflowRegisterKind.Float);
                stack.Add(typeof(bool)); break;
            case CoflowOpCode.LessString:
            case CoflowOpCode.LessOrEqualString:
            case CoflowOpCode.GreaterString:
            case CoflowOpCode.GreaterOrEqualString:
                RequireKind(Pop(), CoflowRegisterKind.Reference); RequireKind(Pop(), CoflowRegisterKind.Reference);
                stack.Add(typeof(bool)); break;
            case CoflowOpCode.EqualInteger:
                RequireKind(Pop(), CoflowRegisterKind.Integer); RequireKind(Pop(), CoflowRegisterKind.Integer);
                stack.Add(typeof(bool)); break;
            case CoflowOpCode.EqualFloat:
                RequireKind(Pop(), CoflowRegisterKind.Float); RequireKind(Pop(), CoflowRegisterKind.Float);
                stack.Add(typeof(bool)); break;
            case CoflowOpCode.EqualReference:
                RequireKind(Pop(), CoflowRegisterKind.Reference); RequireKind(Pop(), CoflowRegisterKind.Reference);
                stack.Add(typeof(bool)); break;
            case CoflowOpCode.JumpIfFalseKeep:
            case CoflowOpCode.JumpIfTrueKeep:
                if (stack.Count == 0)
                    throw Invalid(program, $"stack underflow at instruction {pc}");
                RequireKind(stack[^1], CoflowRegisterKind.Integer);
                yield return (instruction.Operand, stack.ToArray());
                stack.RemoveAt(stack.Count - 1);
                break;
            case CoflowOpCode.JumpIfFalse:
                RequireKind(Pop(), CoflowRegisterKind.Integer);
                yield return (instruction.Operand, stack.ToArray());
                break;
            case CoflowOpCode.Jump:
                yield return (instruction.Operand, stack.ToArray()); yield break;
            case CoflowOpCode.Call:
                {
                    var call = Operation<CoflowCallSite>();
                    if (call.ArgumentCount != call.VmParameterTypes.Length)
                        throw Invalid(program, $"instruction {pc} call-site arity {call.ArgumentCount} does not match target {call.Identity} arity {call.VmParameterTypes.Length}");
                    PopArguments(call.VmParameterTypes);
                    stack.Add(call.Signature.ResultType); break;
                }
            case CoflowOpCode.CallIndirect:
                {
                    if (instruction.Operand < 0 || instruction.Operand >= stack.Count)
                        throw Invalid(program, $"instruction {pc} has an invalid indirect-call arity");
                    var arguments = stack.Skip(stack.Count - instruction.Operand).ToArray();
                    PopMany(instruction.Operand);
                    var callable = Pop();
                    ValidateCallable(callable, arguments, resultType, program, pc);
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.TailCall:
                {
                    var call = Operation<CoflowCallSite>();
                    if (call.ArgumentCount != call.VmParameterTypes.Length)
                        throw Invalid(program, $"instruction {pc} tail-call arity does not match target");
                    PopArguments(call.VmParameterTypes);
                    if (call.Signature.ResultType != program.ReturnType)
                        throw Invalid(program, $"instruction {pc} tail-call result does not match function return");
                    if (stack.Count != 0)
                        throw Invalid(program, $"instruction {pc} tail-call leaves values on the stack");
                    yield break;
                }
            case CoflowOpCode.TailCallIndirect:
                {
                    if (instruction.Operand < 0 || instruction.Operand >= stack.Count)
                        throw Invalid(program, $"instruction {pc} has an invalid indirect tail-call arity");
                    var arguments = stack.Skip(stack.Count - instruction.Operand).ToArray();
                    PopMany(instruction.Operand);
                    var callable = Pop();
                    ValidateCallable(callable, arguments, program.ReturnType, program, pc);
                    if (stack.Count != 0)
                        throw Invalid(program, $"instruction {pc} indirect tail-call leaves values on the stack");
                    yield break;
                }
            case CoflowOpCode.Return:
                {
                    var actual = Pop();
                    if (actual != program.ReturnType)
                        throw Invalid(program, $"return type `{actual}` does not match `{program.ReturnType}`");
                    if (stack.Count != 0)
                        throw Invalid(program, $"instruction {pc} return leaves values on the stack");
                    yield break;
                }
            default: throw Invalid(program, $"unknown opcode `{instruction.Code}`");
        }
        yield return (pc + 1, stack.ToArray());
    }

    private static bool Merge(CoflowLoweringInput program, Type[][] states, int pc, Type[] incoming)
    {
        if (pc < 0 || pc >= states.Length) throw Invalid(program, "jump target is outside the program");
        if (states[pc] is null) { states[pc] = incoming; return true; }
        if (!states[pc].SequenceEqual(incoming))
            throw Invalid(program,
                $"incompatible stack layout at instruction {pc}: " +
                $"[{string.Join(", ", states[pc].Select(type => type.Name))}] vs " +
                $"[{string.Join(", ", incoming.Select(type => type.Name))}]");
        return false;
    }

    private static void ValidateCallable(
        Type callable,
        IReadOnlyList<Type> arguments,
        Type result,
        CoflowLoweringInput program,
        int pc)
    {
        if (!CoflowFunctionHandle.IsFunctionType(callable))
            throw Invalid(program, $"instruction {pc} indirect target `{callable}` has incompatible result");
        var signature = callable.GetGenericArguments();
        if (signature[^1] != result)
            throw Invalid(program, $"instruction {pc} indirect target `{callable}` has incompatible result");
        var parameters = signature[..^1];
        if (parameters.Length != arguments.Count)
            throw Invalid(program, $"instruction {pc} indirect target arity does not match");
        for (var index = 0; index < parameters.Length; index++)
            if (parameters[index] != arguments[index] &&
                !parameters[index].IsAssignableFrom(arguments[index]))
                throw Invalid(program, $"instruction {pc} indirect argument {index} has incompatible type");
    }
    private static InvalidOperationException Invalid(CoflowLoweringInput program, string message) =>
        new($"invalid Coflow program `{program.Identity}`: {message}");
}
