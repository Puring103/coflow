namespace CoflowRuntime.Generated;

using System.Buffers;

internal enum CoflowOpCode : byte
{
    Constant, Argument, Local, StoreLocal, LoadField, Native,
    MakeOptionNone, MakeOptionSome, MakeResultOk, MakeResultErr, ReadValueTag, ReadFirstPayload, ReadSecondPayload, Propagate,
    MakeClosure, Pop,
    Reinterpret, ConvertIntToFloat, ConvertFloatToInt, IsType,
    NegateInt, NegateFloat, Not, BitNot,
    AddInt, AddFloat, AddString, SubtractInt, SubtractFloat, MultiplyInt, MultiplyFloat,
    DivideInt, DivideFloat, IntegerDivide, Remainder, PowerInt, PowerFloat,
    ShiftLeft, ShiftRight, BitAnd, BitXor, BitOr,
    LessInt, LessFloat, LessString, LessOrEqualInt, LessOrEqualFloat, LessOrEqualString,
    GreaterInt, GreaterFloat, GreaterString,
    GreaterOrEqualInt, GreaterOrEqualFloat, GreaterOrEqualString,
    EqualInteger, EqualFloat, EqualReference,
    JumpIfFalseKeep, JumpIfTrueKeep, JumpIfFalse, Jump,
    Call, CallIndirect, TailCall, TailCallIndirect, Return,
}

internal static class CoflowBoundaryCodec<T>
{
    internal static readonly Action<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T> Write =
        CoflowBoundaryCodec.BuildWrite<T>();
    internal static readonly Func<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T> Read =
        CoflowBoundaryCodec.BuildRead<T>();
    internal static readonly Action<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T> WriteRelative =
        CoflowBoundaryCodec.BuildWrite<T>(relative: true);
    internal static readonly Func<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T> ReadRelative =
        CoflowBoundaryCodec.BuildRead<T>(relative: true);
}

internal static class CoflowBoundaryCodec
{
    internal static Action<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T> BuildWrite<T>(
        bool relative = false)
    {
        var context = System.Linq.Expressions.Expression.Parameter(typeof(CoflowVm.CoflowExecutionContext), "context");
        var register = System.Linq.Expressions.Expression.Parameter(typeof(CoflowValueRegister), "register");
        var value = System.Linq.Expressions.Expression.Parameter(typeof(T), "value");
        return CoflowExpressionCompiler.Compile(
            System.Linq.Expressions.Expression.Lambda<Action<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T>>(
                WriteExpression(typeof(T), context, register, value, relative), context, register, value));
    }

    internal static Func<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T> BuildRead<T>(
        bool relative = false)
    {
        var context = System.Linq.Expressions.Expression.Parameter(typeof(CoflowVm.CoflowExecutionContext), "context");
        var register = System.Linq.Expressions.Expression.Parameter(typeof(CoflowValueRegister), "register");
        return CoflowExpressionCompiler.Compile(
            System.Linq.Expressions.Expression.Lambda<Func<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T>>(
                ReadExpression(typeof(T), context, register, relative), context, register));
    }

    private static System.Linq.Expressions.Expression WriteExpression(
        Type type,
        System.Linq.Expressions.Expression context,
        System.Linq.Expressions.Expression register,
        System.Linq.Expressions.Expression value,
        bool relative)
    {
        var shape = CoflowValueShape.Of(type);
        if (shape.Kind == CoflowValueShapeKind.Unit)
            return System.Linq.Expressions.Expression.Empty();
        if (shape.Kind == CoflowValueShapeKind.Scalar)
        {
            var scalar = Register(register, shape.ScalarKind!.Value, relative);
            return shape.ScalarKind switch
            {
                CoflowRegisterKind.Integer => System.Linq.Expressions.Expression.Call(
                    context,
                    relative ? nameof(CoflowVm.CoflowExecutionContext.WriteIntegerRelative) :
                        nameof(CoflowVm.CoflowExecutionContext.WriteInteger),
                    Type.EmptyTypes,
                    scalar,
                    type == typeof(bool)
                        ? System.Linq.Expressions.Expression.Condition(value,
                            System.Linq.Expressions.Expression.Constant(1L),
                            System.Linq.Expressions.Expression.Constant(0L))
                        : System.Linq.Expressions.Expression.Convert(value, typeof(long))),
                CoflowRegisterKind.Float => System.Linq.Expressions.Expression.Call(
                    context, relative ? nameof(CoflowVm.CoflowExecutionContext.WriteFloatRelative) :
                        nameof(CoflowVm.CoflowExecutionContext.WriteFloat), Type.EmptyTypes,
                    scalar, value),
                _ => System.Linq.Expressions.Expression.Call(
                    context, relative ? nameof(CoflowVm.CoflowExecutionContext.WriteReferenceRelative) :
                        nameof(CoflowVm.CoflowExecutionContext.WriteReference), Type.EmptyTypes,
                    scalar, System.Linq.Expressions.Expression.Convert(value, typeof(object))),
            };
        }

        var tag = Register(register, CoflowRegisterKind.Integer, relative, tag: true);
        var first = System.Linq.Expressions.Expression.Property(register, nameof(CoflowValueRegister.First));
        if (shape.Kind == CoflowValueShapeKind.Option)
        {
            var hasValue = System.Linq.Expressions.Expression.Property(value, nameof(Option<int>.HasValue));
            return System.Linq.Expressions.Expression.Block(
                System.Linq.Expressions.Expression.Call(context,
                    relative ? nameof(CoflowVm.CoflowExecutionContext.WriteIntegerRelative) :
                        nameof(CoflowVm.CoflowExecutionContext.WriteInteger), Type.EmptyTypes,
                    tag, System.Linq.Expressions.Expression.Condition(hasValue,
                        System.Linq.Expressions.Expression.Constant(1L),
                        System.Linq.Expressions.Expression.Constant(0L))),
                System.Linq.Expressions.Expression.IfThen(hasValue,
                    WriteExpression(shape.First!.Type, context, first,
                        System.Linq.Expressions.Expression.Property(value, nameof(Option<int>.Value)), relative)));
        }

        var isOk = System.Linq.Expressions.Expression.Property(value, nameof(Result<int, int>.IsOk));
        var second = System.Linq.Expressions.Expression.Property(register, nameof(CoflowValueRegister.Second));
        return System.Linq.Expressions.Expression.Block(
            System.Linq.Expressions.Expression.Call(context,
                relative ? nameof(CoflowVm.CoflowExecutionContext.WriteIntegerRelative) :
                    nameof(CoflowVm.CoflowExecutionContext.WriteInteger), Type.EmptyTypes,
                tag, System.Linq.Expressions.Expression.Condition(isOk,
                    System.Linq.Expressions.Expression.Constant(1L),
                    System.Linq.Expressions.Expression.Constant(0L))),
            System.Linq.Expressions.Expression.IfThenElse(isOk,
                WriteExpression(shape.First!.Type, context, first,
                    System.Linq.Expressions.Expression.Property(value, nameof(Result<int, int>.Value)), relative),
                WriteExpression(shape.Second!.Type, context, second,
                    System.Linq.Expressions.Expression.Property(value, nameof(Result<int, int>.Error)), relative)));
    }

    private static System.Linq.Expressions.Expression ReadExpression(
        Type type,
        System.Linq.Expressions.Expression context,
        System.Linq.Expressions.Expression register,
        bool relative)
    {
        var shape = CoflowValueShape.Of(type);
        if (shape.Kind == CoflowValueShapeKind.Unit)
            return System.Linq.Expressions.Expression.Property(null, typeof(Unit), nameof(Unit.Value));
        if (shape.Kind == CoflowValueShapeKind.Scalar)
        {
            var scalar = Register(register, shape.ScalarKind!.Value, relative);
            var read = shape.ScalarKind switch
            {
                CoflowRegisterKind.Integer => System.Linq.Expressions.Expression.Call(context,
                    relative ? nameof(CoflowVm.CoflowExecutionContext.ReadIntegerRelative) :
                        nameof(CoflowVm.CoflowExecutionContext.ReadInteger), Type.EmptyTypes, scalar),
                CoflowRegisterKind.Float => System.Linq.Expressions.Expression.Call(context,
                    relative ? nameof(CoflowVm.CoflowExecutionContext.ReadFloatRelative) :
                        nameof(CoflowVm.CoflowExecutionContext.ReadFloat), Type.EmptyTypes, scalar),
                _ => System.Linq.Expressions.Expression.Call(context,
                    relative ? nameof(CoflowVm.CoflowExecutionContext.ReadReferenceRelative) :
                        nameof(CoflowVm.CoflowExecutionContext.ReadReference), Type.EmptyTypes, scalar),
            };
            if (type == typeof(bool))
                return System.Linq.Expressions.Expression.NotEqual(read, System.Linq.Expressions.Expression.Constant(0L));
            if (typeof(Delegate).IsAssignableFrom(type))
                return System.Linq.Expressions.Expression.Call(
                    typeof(CoflowFunctionDelegates), nameof(CoflowFunctionDelegates.Adapt),
                    new[] { type }, read);
            return System.Linq.Expressions.Expression.Convert(read, type);
        }

        var tag = Register(register, CoflowRegisterKind.Integer, relative, tag: true);
        var active = System.Linq.Expressions.Expression.NotEqual(
            System.Linq.Expressions.Expression.Call(context,
                relative ? nameof(CoflowVm.CoflowExecutionContext.ReadIntegerRelative) :
                    nameof(CoflowVm.CoflowExecutionContext.ReadInteger), Type.EmptyTypes, tag),
            System.Linq.Expressions.Expression.Constant(0L));
        var first = System.Linq.Expressions.Expression.Property(register, nameof(CoflowValueRegister.First));
        if (shape.Kind == CoflowValueShapeKind.Option)
        {
            var some = type.GetMethod(nameof(Option<int>.Some),
                System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.Static)!;
            var none = System.Linq.Expressions.Expression.Property(null, type, nameof(Option<int>.None));
            return System.Linq.Expressions.Expression.Condition(active,
                System.Linq.Expressions.Expression.Call(some,
                    ReadExpression(shape.First!.Type, context, first, relative)), none);
        }
        var second = System.Linq.Expressions.Expression.Property(register, nameof(CoflowValueRegister.Second));
        return System.Linq.Expressions.Expression.Condition(active,
            System.Linq.Expressions.Expression.Call(type.GetMethod(nameof(Result<int, int>.Ok))!,
                ReadExpression(shape.First!.Type, context, first, relative)),
            System.Linq.Expressions.Expression.Call(type.GetMethod(nameof(Result<int, int>.Err))!,
                ReadExpression(shape.Second!.Type, context, second, relative)));
    }

    private static System.Linq.Expressions.Expression Register(
        System.Linq.Expressions.Expression register,
        CoflowRegisterKind kind,
        bool relative,
        bool tag = false)
    {
        if (!relative)
            return System.Linq.Expressions.Expression.Property(register,
                tag ? nameof(CoflowValueRegister.Tag) : nameof(CoflowValueRegister.Scalar));
        return System.Linq.Expressions.Expression.Property(register, kind switch
        {
            CoflowRegisterKind.Integer => nameof(CoflowValueRegister.IntegerBase),
            CoflowRegisterKind.Float => nameof(CoflowValueRegister.FloatBase),
            _ => nameof(CoflowValueRegister.ReferenceBase),
        });
    }
}

internal readonly record struct CoflowInstruction(
    CoflowOpCode Code,
    int Operand = 0,
    Type? ValueType = null);

internal readonly record struct CoflowCallSite(CoflowFunctionEntry Entry, int ArgumentCount);
internal delegate void CoflowNativeInvoker(CoflowNativeFrame frame);

internal sealed class CoflowNativeCallSite(
    CoflowNativeCall call,
    CoflowValueRegister[] arguments,
    CoflowValueRegister result)
{
    internal CoflowNativeCall Call { get; } = call;
    internal CoflowValueRegister[] Arguments { get; } = arguments;
    internal CoflowValueRegister Result { get; } = result;
}

internal sealed class CoflowNativeCall
{
    internal CoflowNativeCall(Delegate implementation)
    {
        if (implementation is null) throw new ArgumentNullException(nameof(implementation));
        var invoke = implementation.GetType().GetMethod("Invoke")!;
        ParameterTypes = invoke.GetParameters().Select(parameter => parameter.ParameterType).ToArray();
        ResultType = invoke.ReturnType == typeof(void) ? typeof(Unit) : invoke.ReturnType;
        Invoke = Build(implementation, invoke);
    }

    internal CoflowNativeCall(Type[] parameterTypes, Type resultType, CoflowNativeInvoker invoke)
    {
        ParameterTypes = parameterTypes ?? throw new ArgumentNullException(nameof(parameterTypes));
        ResultType = resultType ?? throw new ArgumentNullException(nameof(resultType));
        Invoke = invoke ?? throw new ArgumentNullException(nameof(invoke));
    }

    internal static CoflowNativeCall Create<TRecord, TValue>(Func<TRecord, TValue> implementation)
    {
        if (implementation is null) throw new ArgumentNullException(nameof(implementation));
        return new CoflowNativeCall(
            new[] { typeof(TRecord) },
            typeof(TValue),
            frame => frame.Write(implementation(frame.Read<TRecord>(0))));
    }

    internal int ArgumentCount => ParameterTypes.Length;
    internal Type[] ParameterTypes { get; }
    internal Type ResultType { get; }
    internal CoflowNativeInvoker Invoke { get; }

    private static CoflowNativeInvoker Build(Delegate implementation, System.Reflection.MethodInfo invoke)
    {
        var frame = System.Linq.Expressions.Expression.Parameter(typeof(CoflowNativeFrame), "frame");
        var arguments = invoke.GetParameters().Select((parameter, index) =>
            System.Linq.Expressions.Expression.Call(frame,
                typeof(CoflowNativeFrame).GetMethod(nameof(CoflowNativeFrame.Read))!
                    .MakeGenericMethod(parameter.ParameterType),
                System.Linq.Expressions.Expression.Constant(index))).ToArray();
        var call = System.Linq.Expressions.Expression.Invoke(
            System.Linq.Expressions.Expression.Constant(implementation), arguments);
        System.Linq.Expressions.Expression body = invoke.ReturnType == typeof(void)
            ? System.Linq.Expressions.Expression.Block(call,
                System.Linq.Expressions.Expression.Call(frame,
                    typeof(CoflowNativeFrame).GetMethod(nameof(CoflowNativeFrame.Write))!
                        .MakeGenericMethod(typeof(Unit)),
                    System.Linq.Expressions.Expression.Property(null, typeof(Unit), nameof(Unit.Value))))
            : System.Linq.Expressions.Expression.Call(frame,
                typeof(CoflowNativeFrame).GetMethod(nameof(CoflowNativeFrame.Write))!
                    .MakeGenericMethod(invoke.ReturnType), call);
        return CoflowExpressionCompiler.Compile(
            System.Linq.Expressions.Expression.Lambda<CoflowNativeInvoker>(body, frame));
    }
}

internal readonly struct CoflowNativeFrame
{
    private readonly CoflowVm.CoflowExecutionContext _context;
    private readonly CoflowValueRegister[] _arguments;
    private readonly CoflowValueRegister _result;
    private readonly Type _resultType;

    internal CoflowNativeFrame(
        CoflowVm.CoflowExecutionContext context,
        CoflowNativeCallSite site)
        : this(context, site.Arguments, site.Result, site.Call.ResultType)
    {
    }

    internal CoflowNativeFrame(
        CoflowVm.CoflowExecutionContext context,
        CoflowValueRegister[] arguments,
        CoflowValueRegister result,
        Type resultType)
    {
        _context = context;
        _arguments = arguments;
        _result = result;
        _resultType = resultType;
    }

    public T Read<T>(int index) =>
        CoflowBoundaryCodec<T>.ReadRelative(_context, _arguments[index]);

    public void Write<T>(T value)
    {
        if (typeof(T) != _resultType)
            throw new InvalidOperationException($"native result `{typeof(T)}` does not match `{_resultType}`");
        CoflowBoundaryCodec<T>.WriteRelative(_context, _result, value);
    }
}

internal static class CoflowNativeCallFactory
{
    private static readonly System.Reflection.MethodInfo ArrayMethod = Method(nameof(ArrayCore));
    private static readonly System.Reflection.MethodInfo DictionaryMethod = Method(nameof(DictionaryCore));

    internal static CoflowNativeCall Array(Type elementType, int count) =>
        (CoflowNativeCall)ArrayMethod.MakeGenericMethod(elementType).Invoke(null, new object[] { count })!;

    internal static CoflowNativeCall Dictionary(Type keyType, Type valueType, int count) =>
        (CoflowNativeCall)DictionaryMethod.MakeGenericMethod(keyType, valueType)
            .Invoke(null, new object[] { count })!;

    private static CoflowNativeCall ArrayCore<T>(int count) => new(
        Enumerable.Repeat(typeof(T), count).ToArray(), typeof(IReadOnlyList<T>), frame =>
    {
        var values = new T[count];
        for (var index = 0; index < values.Length; index++) values[index] = frame.Read<T>(index);
        frame.Write<IReadOnlyList<T>>(System.Array.AsReadOnly(values));
    });

    private static CoflowNativeCall DictionaryCore<TKey, TValue>(int count) where TKey : notnull =>
        new(Enumerable.Range(0, count * 2)
                .Select(index => index % 2 == 0 ? typeof(TKey) : typeof(TValue)).ToArray(),
            typeof(IReadOnlyDictionary<TKey, TValue>), frame =>
        {
            var values = new Dictionary<TKey, TValue>();
            for (var index = 0; index < count; index++)
                values.Add(frame.Read<TKey>(index * 2), frame.Read<TValue>(index * 2 + 1));
            frame.Write<IReadOnlyDictionary<TKey, TValue>>(
                new System.Collections.ObjectModel.ReadOnlyDictionary<TKey, TValue>(values));
        });

    private static System.Reflection.MethodInfo Method(string name) =>
        typeof(CoflowNativeCallFactory).GetMethod(name,
            System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic)!;
}
internal readonly record struct CoflowLoopAccess(
    Delegate Prepare,
    Type PreparedType,
    Delegate Count,
    Delegate First,
    Delegate? Second);
internal sealed class CoflowClosureTemplate
{
    internal CoflowClosureTemplate(CoflowProgram program, IReadOnlyList<Type> captureTypes)
    {
        Program = program;
        Captures = new CoflowCaptureLayout[captureTypes.Count];
        for (var index = 0; index < captureTypes.Count; index++)
        {
            var shape = CoflowValueShape.Of(captureTypes[index]);
            Captures[index] = new CoflowCaptureLayout(
                shape, IntegerCount, FloatCount, ReferenceCount);
            IntegerCount += shape.IntegerCount;
            FloatCount += shape.FloatCount;
            ReferenceCount += shape.ReferenceCount;
        }
    }

    internal CoflowProgram Program { get; }
    internal CoflowCaptureLayout[] Captures { get; }
    internal int CaptureCount => Captures.Length;
    internal int IntegerCount { get; private set; }
    internal int FloatCount { get; private set; }
    internal int ReferenceCount { get; private set; }
}
internal readonly record struct CoflowCaptureLayout(
    CoflowValueShape Shape,
    int IntegerBase,
    int FloatBase,
    int ReferenceBase);
internal sealed record CoflowEncodedValue(
    CoflowValueShape Shape,
    long[] Integers,
    double[] Floats,
    object?[] References)
{
    internal static CoflowEncodedValue Encode(Type type, object? value)
    {
        var shape = CoflowValueShape.Of(type);
        var integers = new long[shape.IntegerCount];
        var floats = new double[shape.FloatCount];
        var references = new object?[shape.ReferenceCount];
        Encode(shape, value, 0, 0, 0, integers, floats, references);
        return new(shape, integers, floats, references);
    }

    private static void Encode(
        CoflowValueShape shape, object? value, int integerBase, int floatBase, int referenceBase,
        long[] integers, double[] floats, object?[] references)
    {
        if (shape.Kind == CoflowValueShapeKind.Unit) return;
        if (shape.Kind == CoflowValueShapeKind.Scalar)
        {
            switch (shape.ScalarKind)
            {
                case CoflowRegisterKind.Integer:
                    integers[integerBase] = shape.Type == typeof(bool)
                        ? (bool)value! ? 1 : 0 : Convert.ToInt64(value);
                    break;
                case CoflowRegisterKind.Float: floats[floatBase] = (double)value!; break;
                default: references[referenceBase] = value; break;
            }
            return;
        }
        var active = (bool)shape.Type.GetProperty(shape.Kind == CoflowValueShapeKind.Option
            ? nameof(Option<int>.HasValue) : nameof(Result<int, int>.IsOk))!.GetValue(value)!;
        integers[integerBase] = active ? 1 : 0;
        if (shape.Kind == CoflowValueShapeKind.Option)
        {
            if (active) Encode(shape.First!, shape.Type.GetProperty(nameof(Option<int>.Value))!.GetValue(value),
                integerBase + 1, floatBase, referenceBase, integers, floats, references);
            return;
        }
        var child = active ? shape.First! : shape.Second!;
        Encode(child, shape.Type.GetProperty(active ? nameof(Result<int, int>.Value) : nameof(Result<int, int>.Error))!
                .GetValue(value),
            integerBase + 1 + (active ? 0 : shape.First!.IntegerCount),
            floatBase + (active ? 0 : shape.First!.FloatCount),
            referenceBase + (active ? 0 : shape.First!.ReferenceCount),
            integers, floats, references);
    }
}
internal readonly record struct CoflowHigherOrderOperation(
    string Name,
    Type ElementType,
    Type OutputElementType,
    Type ResultType,
    Delegate Count,
    Delegate Item,
    Delegate? CreateBuilder,
    Delegate? Add,
    Delegate? Finish);

internal abstract class CoflowClosure
{
    private CoflowClosure(CoflowProgram program, CoflowCaptureLayout[] captures)
    {
        Program = program;
        Captures = captures;
    }

    internal CoflowProgram Program { get; }
    internal IReadOnlyList<CoflowCaptureLayout> Captures { get; }
    internal abstract long Integer(int index);
    internal abstract double Float(int index);
    internal abstract object? Reference(int index);
    internal abstract void SetInteger(int index, long value);
    internal abstract void SetFloat(int index, double value);
    internal abstract void SetReference(int index, object? value);

    internal static CoflowClosure Create(
        CoflowProgram program,
        CoflowCaptureLayout[] captures,
        int integerCount,
        int floatCount,
        int referenceCount) => integerCount == 0 && floatCount == 0 && referenceCount == 0
            ? new Empty(program, captures)
            : new WithCaptures(program, captures, integerCount, floatCount, referenceCount);

    private sealed class Empty(CoflowProgram program, CoflowCaptureLayout[] captures)
        : CoflowClosure(program, captures)
    {
        internal override long Integer(int index) => throw NoCapture();
        internal override double Float(int index) => throw NoCapture();
        internal override object? Reference(int index) => throw NoCapture();
        internal override void SetInteger(int index, long value) => throw NoCapture();
        internal override void SetFloat(int index, double value) => throw NoCapture();
        internal override void SetReference(int index, object? value) => throw NoCapture();
        private static InvalidOperationException NoCapture() =>
            new("the closure has no captured values");
    }

    private sealed class WithCaptures : CoflowClosure
    {
        private long _integer0;
        private double _float0;
        private object? _reference0;
        private readonly long[]? _integerCaptures;
        private readonly double[]? _floatCaptures;
        private readonly object?[]? _referenceCaptures;

        internal WithCaptures(
            CoflowProgram program,
            CoflowCaptureLayout[] captures,
            int integerCount,
            int floatCount,
            int referenceCount) : base(program, captures)
        {
            _integerCaptures = integerCount > 1 ? new long[integerCount] : null;
            _floatCaptures = floatCount > 1 ? new double[floatCount] : null;
            _referenceCaptures = referenceCount > 1 ? new object?[referenceCount] : null;
        }

        internal override long Integer(int index) => _integerCaptures is { } values
            ? values[index] : _integer0;
        internal override double Float(int index) => _floatCaptures is { } values
            ? values[index] : _float0;
        internal override object? Reference(int index) => _referenceCaptures is { } values
            ? values[index] : _reference0;
        internal override void SetInteger(int index, long value)
        {
            if (_integerCaptures is { } values) values[index] = value;
            else _integer0 = value;
        }
        internal override void SetFloat(int index, double value)
        {
            if (_floatCaptures is { } values) values[index] = value;
            else _float0 = value;
        }
        internal override void SetReference(int index, object? value)
        {
            if (_referenceCaptures is { } values) values[index] = value;
            else _reference0 = value;
        }
    }

}

internal sealed class CoflowProgram
{
    internal CoflowProgram(
        CoflowFunctionIdentity identity,
        string sourcePath,
        CfdSpan? sourceSpan,
        IReadOnlyList<CoflowInstruction> instructions,
        IReadOnlyList<CfdSpan?> instructionSpans,
        IReadOnlyList<object?> constants,
        IReadOnlyList<Type> parameterTypes,
        Type returnType,
        int localCount)
    {
        Identity = identity;
        SourcePath = sourcePath;
        SourceSpan = sourceSpan;
        var loweredInstructions = instructions.ToArray();
        var loweredInstructionSpans = instructionSpans.ToArray();
        if (loweredInstructions.Length == 0)
            throw new InvalidOperationException($"Coflow program `{identity}` has no instructions.");
        if (loweredInstructionSpans.Length != loweredInstructions.Length)
            throw new InvalidOperationException($"Coflow program `{identity}` has an invalid source map.");
        if (localCount < 0)
            throw new InvalidOperationException($"Coflow program `{identity}` has a negative local count.");
        var sourceConstants = constants.ToArray();
        var operations = sourceConstants.ToArray();
        var encodedConstants = new CoflowEncodedValue?[sourceConstants.Length];
        foreach (var instruction in loweredInstructions)
            if (instruction.Code == CoflowOpCode.Constant)
            {
                if ((uint)instruction.Operand >= (uint)sourceConstants.Length)
                    throw new InvalidOperationException($"Coflow program `{identity}` has an invalid constant index.");
                encodedConstants[instruction.Operand] ??= CoflowEncodedValue.Encode(
                    instruction.ValueType ?? sourceConstants[instruction.Operand]?.GetType() ?? typeof(object),
                    sourceConstants[instruction.Operand]);
                operations[instruction.Operand] = null;
            }
        ParameterTypes = parameterTypes.ToArray();
        ReturnType = returnType;
        RegisterProgram = CoflowRegisterLowering.Lower(new CoflowLoweringInput(
            identity,
            loweredInstructions,
            loweredInstructionSpans,
            operations,
            encodedConstants,
            ParameterTypes,
            ReturnType,
            localCount));
    }

    internal CoflowFunctionIdentity Identity { get; }
    internal string SourcePath { get; }
    internal CfdSpan? SourceSpan { get; }
    internal Type[] ParameterTypes { get; }
    internal Type ReturnType { get; }
    internal int ParameterCount => ParameterTypes.Length;
    internal CoflowRegisterProgram RegisterProgram { get; }
}

internal static class CoflowVm
{
    [ThreadStatic] private static CoflowExecutionContext? _pooledContexts;

    internal static TResult Execute<TResult>(CoflowProgram program) => ExecuteCore<Arguments0, TResult>(program, new());
    internal static TResult Execute<T1, TResult>(CoflowProgram program, T1 arg1) => ExecuteCore<Arguments1<T1>, TResult>(program, new(arg1));
    internal static TResult Execute<T1, T2, TResult>(CoflowProgram program, T1 arg1, T2 arg2) => ExecuteCore<Arguments2<T1, T2>, TResult>(program, new(arg1, arg2));
    internal static TResult Execute<T1, T2, T3, TResult>(CoflowProgram program, T1 arg1, T2 arg2, T3 arg3) => ExecuteCore<Arguments3<T1, T2, T3>, TResult>(program, new(arg1, arg2, arg3));
    internal static TResult Execute<T1, T2, T3, T4, TResult>(CoflowProgram program, T1 arg1, T2 arg2, T3 arg3, T4 arg4) => ExecuteCore<Arguments4<T1, T2, T3, T4>, TResult>(program, new(arg1, arg2, arg3, arg4));
    internal static TResult Execute<T1, T2, T3, T4, T5, TResult>(CoflowProgram program, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5) => ExecuteCore<Arguments5<T1, T2, T3, T4, T5>, TResult>(program, new(arg1, arg2, arg3, arg4, arg5));
    internal static TResult Execute<T1, T2, T3, T4, T5, T6, TResult>(CoflowProgram program, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6) => ExecuteCore<Arguments6<T1, T2, T3, T4, T5, T6>, TResult>(program, new(arg1, arg2, arg3, arg4, arg5, arg6));
    internal static TResult Execute<T1, T2, T3, T4, T5, T6, T7, TResult>(CoflowProgram program, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7) => ExecuteCore<Arguments7<T1, T2, T3, T4, T5, T6, T7>, TResult>(program, new(arg1, arg2, arg3, arg4, arg5, arg6, arg7));
    internal static TResult Execute<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(CoflowProgram program, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7, T8 arg8) => ExecuteCore<Arguments8<T1, T2, T3, T4, T5, T6, T7, T8>, TResult>(program, new(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8));
    internal static TResult ExecuteClosure<TResult>(CoflowClosure closure) =>
        ExecuteCore<ClosureArguments<Arguments0>, TResult>(closure.Program, new(closure, new Arguments0()));
    internal static TResult ExecuteClosure<T1, TResult>(CoflowClosure closure, T1 arg1) =>
        ExecuteCore<ClosureArguments<Arguments1<T1>>, TResult>(closure.Program, new(closure, new(arg1)));
    internal static TResult ExecuteClosure<T1, T2, TResult>(CoflowClosure closure, T1 arg1, T2 arg2) =>
        ExecuteCore<ClosureArguments<Arguments2<T1, T2>>, TResult>(closure.Program, new(closure, new(arg1, arg2)));
    internal static TResult ExecuteClosure<T1, T2, T3, TResult>(CoflowClosure closure, T1 arg1, T2 arg2, T3 arg3) =>
        ExecuteCore<ClosureArguments<Arguments3<T1, T2, T3>>, TResult>(closure.Program, new(closure, new(arg1, arg2, arg3)));
    internal static TResult ExecuteClosure<T1, T2, T3, T4, TResult>(CoflowClosure closure, T1 arg1, T2 arg2, T3 arg3, T4 arg4) =>
        ExecuteCore<ClosureArguments<Arguments4<T1, T2, T3, T4>>, TResult>(closure.Program, new(closure, new(arg1, arg2, arg3, arg4)));
    internal static TResult ExecuteClosure<T1, T2, T3, T4, T5, TResult>(CoflowClosure closure, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5) =>
        ExecuteCore<ClosureArguments<Arguments5<T1, T2, T3, T4, T5>>, TResult>(closure.Program, new(closure, new(arg1, arg2, arg3, arg4, arg5)));
    internal static TResult ExecuteClosure<T1, T2, T3, T4, T5, T6, TResult>(CoflowClosure closure, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6) =>
        ExecuteCore<ClosureArguments<Arguments6<T1, T2, T3, T4, T5, T6>>, TResult>(closure.Program, new(closure, new(arg1, arg2, arg3, arg4, arg5, arg6)));
    internal static TResult ExecuteClosure<T1, T2, T3, T4, T5, T6, T7, TResult>(CoflowClosure closure, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7) =>
        ExecuteCore<ClosureArguments<Arguments7<T1, T2, T3, T4, T5, T6, T7>>, TResult>(closure.Program, new(closure, new(arg1, arg2, arg3, arg4, arg5, arg6, arg7)));
    internal static TResult ExecuteClosure<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(CoflowClosure closure, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7, T8 arg8) =>
        ExecuteCore<ClosureArguments<Arguments8<T1, T2, T3, T4, T5, T6, T7, T8>>, TResult>(closure.Program, new(closure, new(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8)));

    private static TResult ExecuteCore<TArguments, TResult>(CoflowProgram program, TArguments arguments)
        where TArguments : struct, ICoflowArguments
    {
        if (arguments.Count != program.ParameterCount)
            throw Fault(program, $"function expected {program.ParameterCount} arguments but received {arguments.Count}");
        var context = RentContext();
        try
        {
            context.Start(program, arguments);
            while (true)
            {
                var pc = context.Pc;
                var registers = context.Program.RegisterProgram;
                if ((uint)pc >= (uint)registers.Instructions.Length)
                    throw new InvalidOperationException("Coflow function ended without Return.");
                var instruction = registers.Instructions[pc];
                context.Pc = pc + 1;
                switch (instruction.Code)
                {
                    case CoflowRegisterOpCode.Nop: break;
                    case CoflowRegisterOpCode.ConstantInteger:
                        context.WriteIntegerRelative(instruction.A, instruction.Immediate);
                        break;
                    case CoflowRegisterOpCode.ConstantFloat:
                        context.WriteFloatRelative(instruction.A,
                            BitConverter.Int64BitsToDouble(instruction.Immediate));
                        break;
                    case CoflowRegisterOpCode.ConstantReference:
                        context.WriteReferenceRelative(instruction.A, instruction.Operation);
                        break;
                    case CoflowRegisterOpCode.ConstantValue:
                    {
                        var site = (CoflowRegisterConstantSite)instruction.Operation!;
                        context.WriteEncodedRelative(site.Value, site.Target);
                        break;
                    }
                    case CoflowRegisterOpCode.MoveInteger:
                        context.WriteIntegerRelative(instruction.A,
                            context.ReadIntegerRelative(instruction.B));
                        break;
                    case CoflowRegisterOpCode.MoveFloat:
                        context.WriteFloatRelative(instruction.A,
                            context.ReadFloatRelative(instruction.B));
                        break;
                    case CoflowRegisterOpCode.MoveReference:
                        context.WriteReferenceRelative(instruction.A,
                            context.ReadReferenceRelative(instruction.B));
                        break;
                    case CoflowRegisterOpCode.MoveValue:
                    {
                        var site = (CoflowRegisterValueTransfer)instruction.Operation!;
                        context.CopyRelative(site.Source, site.Target);
                        break;
                    }
                    case CoflowRegisterOpCode.LoadFieldInteger:
                    case CoflowRegisterOpCode.LoadFieldFloat:
                    case CoflowRegisterOpCode.LoadFieldReference:
                    {
                        var access = (CoflowFieldAccess)instruction.Operation!;
                        var receiver = context.ReadReferenceRelative(instruction.B)
                            ?? throw new InvalidOperationException($"field `{access.Name}` receiver is null");
                        if (instruction.Code == CoflowRegisterOpCode.LoadFieldInteger)
                            context.WriteIntegerRelative(instruction.A, access.ReadInteger!(receiver));
                        else if (instruction.Code == CoflowRegisterOpCode.LoadFieldFloat)
                            context.WriteFloatRelative(instruction.A, access.ReadFloat!(receiver));
                        else context.WriteReferenceRelative(instruction.A, access.ReadReference!(receiver));
                        break;
                    }
                    case CoflowRegisterOpCode.Native:
                    {
                        var site = (CoflowNativeCallSite)instruction.Operation!;
                        site.Call.Invoke(new CoflowNativeFrame(context, site));
                        break;
                    }
                    case CoflowRegisterOpCode.MakeOptionSome:
                    case CoflowRegisterOpCode.MakeResultOk:
                    case CoflowRegisterOpCode.MakeResultErr:
                    {
                        var site = (CoflowRegisterValueTransfer)instruction.Operation!;
                        context.CopyRelative(site.Source,
                            instruction.Code == CoflowRegisterOpCode.MakeResultErr
                                ? site.Target.Second : site.Target.First);
                        context.WriteIntegerRelative(site.Target.IntegerBase,
                            instruction.Code == CoflowRegisterOpCode.MakeResultErr ? 0 : 1);
                        break;
                    }
                    case CoflowRegisterOpCode.MakeOptionNone:
                    {
                        var site = (CoflowRegisterTargetSite)instruction.Operation!;
                        context.WriteIntegerRelative(site.Target.IntegerBase, 0);
                        break;
                    }
                    case CoflowRegisterOpCode.ReadValueTag:
                        context.WriteIntegerRelative(instruction.A,
                            context.ReadIntegerRelative(instruction.B));
                        break;
                    case CoflowRegisterOpCode.ReadFirstPayload:
                    case CoflowRegisterOpCode.ReadSecondPayload:
                    {
                        var site = (CoflowRegisterValueTransfer)instruction.Operation!;
                        context.CopyRelative(site.Source, site.Target);
                        break;
                    }
                    case CoflowRegisterOpCode.Propagate:
                    {
                        var site = (CoflowRegisterPropagateSite)instruction.Operation!;
                        if (context.ReadIntegerRelative(site.Source.IntegerBase) == 0)
                        {
                            context.WriteIntegerRelative(site.ReturnValue.IntegerBase, 0);
                            if (site.Source.Shape.Kind == CoflowValueShapeKind.Result)
                                context.CopyRelative(site.Source.Second, site.ReturnValue.Second);
                            if (context.ReturnRegister<TResult>(site.ReturnValue, out var returned)) return returned;
                        }
                        else context.CopyRelative(site.Source.First, site.Payload);
                        break;
                    }
                    case CoflowRegisterOpCode.MakeClosure:
                        context.MakeClosure((CoflowRegisterClosureSite)instruction.Operation!);
                        break;
                    case CoflowRegisterOpCode.ConvertIntToFloat:
                        context.WriteFloatRelative(instruction.A,
                            context.ReadIntegerRelative(instruction.B));
                        break;
                    case CoflowRegisterOpCode.ConvertFloatToInt:
                        context.WriteIntegerRelative(instruction.A,
                            checked((long)context.ReadFloatRelative(instruction.B)));
                        break;
                    case CoflowRegisterOpCode.IsType:
                        context.WriteIntegerRelative(instruction.A,
                            ((Type)instruction.Operation!).IsInstanceOfType(
                                context.ReadReferenceRelative(instruction.B)) ? 1 : 0);
                        break;
                    case CoflowRegisterOpCode.NegateInt:
                    case CoflowRegisterOpCode.Not:
                    case CoflowRegisterOpCode.BitNot:
                        UnaryInteger(context, instruction);
                        break;
                    case CoflowRegisterOpCode.NegateFloat:
                        context.WriteFloatRelative(instruction.A,
                            -context.ReadFloatRelative(instruction.B));
                        break;
                    case CoflowRegisterOpCode.AddInt:
                    case CoflowRegisterOpCode.SubtractInt:
                    case CoflowRegisterOpCode.MultiplyInt:
                    case CoflowRegisterOpCode.DivideInt:
                    case CoflowRegisterOpCode.IntegerDivide:
                    case CoflowRegisterOpCode.Remainder:
                    case CoflowRegisterOpCode.PowerInt:
                    case CoflowRegisterOpCode.ShiftLeft:
                    case CoflowRegisterOpCode.ShiftRight:
                    case CoflowRegisterOpCode.BitAnd:
                    case CoflowRegisterOpCode.BitXor:
                    case CoflowRegisterOpCode.BitOr:
                        BinaryInteger(context, instruction);
                        break;
                    case CoflowRegisterOpCode.AddFloat:
                    case CoflowRegisterOpCode.SubtractFloat:
                    case CoflowRegisterOpCode.MultiplyFloat:
                    case CoflowRegisterOpCode.DivideFloat:
                    case CoflowRegisterOpCode.PowerFloat:
                        BinaryFloat(context, instruction);
                        break;
                    case CoflowRegisterOpCode.AddString:
                        context.WriteReferenceRelative(instruction.A,
                            (string)context.ReadReferenceRelative(instruction.B)! +
                            (string)context.ReadReferenceRelative(instruction.C)!);
                        break;
                    case CoflowRegisterOpCode.LessInt:
                    case CoflowRegisterOpCode.LessOrEqualInt:
                    case CoflowRegisterOpCode.GreaterInt:
                    case CoflowRegisterOpCode.GreaterOrEqualInt:
                    case CoflowRegisterOpCode.EqualInteger:
                        CompareInteger(context, instruction);
                        break;
                    case CoflowRegisterOpCode.LessFloat:
                    case CoflowRegisterOpCode.LessOrEqualFloat:
                    case CoflowRegisterOpCode.GreaterFloat:
                    case CoflowRegisterOpCode.GreaterOrEqualFloat:
                    case CoflowRegisterOpCode.EqualFloat:
                        CompareFloat(context, instruction);
                        break;
                    case CoflowRegisterOpCode.LessString:
                    case CoflowRegisterOpCode.LessOrEqualString:
                    case CoflowRegisterOpCode.GreaterString:
                    case CoflowRegisterOpCode.GreaterOrEqualString:
                        CompareString(context, instruction);
                        break;
                    case CoflowRegisterOpCode.EqualReference:
                        context.WriteIntegerRelative(instruction.A,
                            Equals(context.ReadReferenceRelative(instruction.B),
                                context.ReadReferenceRelative(instruction.C)) ? 1 : 0);
                        break;
                    case CoflowRegisterOpCode.JumpIfFalse:
                        if (context.ReadIntegerRelative(instruction.A) == 0)
                            context.Pc = instruction.B;
                        break;
                    case CoflowRegisterOpCode.JumpIfTrue:
                        if (context.ReadIntegerRelative(instruction.A) != 0)
                            context.Pc = instruction.B;
                        break;
                    case CoflowRegisterOpCode.Jump:
                        context.Pc = instruction.A;
                        break;
                    case CoflowRegisterOpCode.Call:
                    {
                        var site = (CoflowRegisterCallSite)instruction.Operation!;
                        if (!context.Call(site, tail: false))
                            site.Entry.InvokeBoundFromVm(new CoflowNativeFrame(
                                context, site.Arguments, site.Result,
                                site.Entry.Signature.ResultType));
                        break;
                    }
                    case CoflowRegisterOpCode.CallIndirect:
                        if (context.CallIndirect<TResult>(
                                (CoflowRegisterIndirectCallSite)instruction.Operation!, tail: false,
                                out var indirectResult)) return indirectResult;
                        break;
                    case CoflowRegisterOpCode.TailCall:
                    {
                        var site = (CoflowRegisterCallSite)instruction.Operation!;
                        if (!context.Call(site, tail: true))
                        {
                            site.Entry.InvokeBoundFromVm(new CoflowNativeFrame(
                                context, site.Arguments, site.Result,
                                site.Entry.Signature.ResultType));
                            if (context.ReturnRegister<TResult>(site.Result, out var returned)) return returned;
                        }
                        break;
                    }
                    case CoflowRegisterOpCode.TailCallIndirect:
                        if (context.CallIndirect<TResult>(
                                (CoflowRegisterIndirectCallSite)instruction.Operation!, tail: true,
                                out var indirectTailResult)) return indirectTailResult;
                        break;
                    case CoflowRegisterOpCode.Return:
                    {
                        var site = (CoflowRegisterTargetSite)instruction.Operation!;
                        if (context.ReturnRegister<TResult>(site.Target, out var returned)) return returned;
                        break;
                    }
                    default: throw new InvalidOperationException($"Unknown Coflow opcode `{instruction.Code}`.");
                }
            }
        }
        catch (CoflowFaultException) { throw; }
        catch (Exception error)
        {
            var executable = context.Program.RegisterProgram;
            var span = context.Pc > 0 && context.Pc <= executable.InstructionSpans.Length
                ? executable.InstructionSpans[context.Pc - 1] : null;
            throw Fault(context.Program, error.Message, error, context.CallStack, span);
        }
        finally
        {
            context.Dispose();
        }
    }

    private static CoflowExecutionContext RentContext()
    {
        var context = _pooledContexts;
        if (context is null) context = new CoflowExecutionContext();
        else
        {
            _pooledContexts = context.NextPooled;
            context.NextPooled = null;
        }
        context.Reset();
        return context;
    }

    private static void UnaryInteger(
        CoflowExecutionContext context,
        CoflowRegisterInstruction instruction)
    {
        var value = context.ReadIntegerRelative(instruction.B);
        context.WriteIntegerRelative(instruction.A, instruction.Code switch
        {
            CoflowRegisterOpCode.NegateInt => checked(-value),
            CoflowRegisterOpCode.Not => value == 0 ? 1 : 0,
            CoflowRegisterOpCode.BitNot => ~value,
            _ => throw new InvalidOperationException($"invalid unary integer opcode `{instruction.Code}`"),
        });
    }
    private static void BinaryInteger(
        CoflowExecutionContext context,
        CoflowRegisterInstruction instruction)
    {
        var leftValue = context.ReadIntegerRelative(instruction.B);
        var rightValue = context.ReadIntegerRelative(instruction.C);
        context.WriteIntegerRelative(instruction.A, instruction.Code switch
        {
            CoflowRegisterOpCode.AddInt => checked(leftValue + rightValue),
            CoflowRegisterOpCode.SubtractInt => checked(leftValue - rightValue),
            CoflowRegisterOpCode.MultiplyInt => checked(leftValue * rightValue),
            CoflowRegisterOpCode.DivideInt or CoflowRegisterOpCode.IntegerDivide => checked(leftValue / rightValue),
            CoflowRegisterOpCode.Remainder => checked(leftValue % rightValue),
            CoflowRegisterOpCode.PowerInt => PowerInteger(leftValue, rightValue),
            CoflowRegisterOpCode.ShiftLeft => checked(leftValue << checked((int)rightValue)),
            CoflowRegisterOpCode.ShiftRight => leftValue >> checked((int)rightValue),
            CoflowRegisterOpCode.BitAnd => leftValue & rightValue,
            CoflowRegisterOpCode.BitXor => leftValue ^ rightValue,
            CoflowRegisterOpCode.BitOr => leftValue | rightValue,
            _ => throw new InvalidOperationException($"invalid binary integer opcode `{instruction.Code}`"),
        });
    }
    private static void BinaryFloat(
        CoflowExecutionContext context,
        CoflowRegisterInstruction instruction)
    {
        var leftValue = context.ReadFloatRelative(instruction.B);
        var rightValue = context.ReadFloatRelative(instruction.C);
        context.WriteFloatRelative(instruction.A, instruction.Code switch
        {
            CoflowRegisterOpCode.AddFloat => leftValue + rightValue,
            CoflowRegisterOpCode.SubtractFloat => leftValue - rightValue,
            CoflowRegisterOpCode.MultiplyFloat => leftValue * rightValue,
            CoflowRegisterOpCode.DivideFloat => leftValue / rightValue,
            CoflowRegisterOpCode.PowerFloat => Math.Pow(leftValue, rightValue),
            _ => throw new InvalidOperationException($"invalid binary float opcode `{instruction.Code}`"),
        });
    }
    private static void CompareInteger(
        CoflowExecutionContext context,
        CoflowRegisterInstruction instruction)
    {
        var leftValue = context.ReadIntegerRelative(instruction.B);
        var rightValue = context.ReadIntegerRelative(instruction.C);
        var result = instruction.Code switch
        {
            CoflowRegisterOpCode.LessInt => leftValue < rightValue,
            CoflowRegisterOpCode.LessOrEqualInt => leftValue <= rightValue,
            CoflowRegisterOpCode.GreaterInt => leftValue > rightValue,
            CoflowRegisterOpCode.GreaterOrEqualInt => leftValue >= rightValue,
            CoflowRegisterOpCode.EqualInteger => leftValue == rightValue,
            _ => throw new InvalidOperationException($"invalid integer comparison opcode `{instruction.Code}`"),
        };
        context.WriteIntegerRelative(instruction.A, result ? 1 : 0);
    }
    private static void CompareFloat(
        CoflowExecutionContext context,
        CoflowRegisterInstruction instruction)
    {
        var leftValue = context.ReadFloatRelative(instruction.B);
        var rightValue = context.ReadFloatRelative(instruction.C);
        var result = instruction.Code switch
        {
            CoflowRegisterOpCode.LessFloat => leftValue < rightValue,
            CoflowRegisterOpCode.LessOrEqualFloat => leftValue <= rightValue,
            CoflowRegisterOpCode.GreaterFloat => leftValue > rightValue,
            CoflowRegisterOpCode.GreaterOrEqualFloat => leftValue >= rightValue,
            CoflowRegisterOpCode.EqualFloat => leftValue.Equals(rightValue),
            _ => throw new InvalidOperationException($"invalid float comparison opcode `{instruction.Code}`"),
        };
        context.WriteIntegerRelative(instruction.A, result ? 1 : 0);
    }
    private static void CompareString(
        CoflowExecutionContext context,
        CoflowRegisterInstruction instruction)
    {
        var comparison = string.CompareOrdinal(
            (string)context.ReadReferenceRelative(instruction.B)!,
            (string)context.ReadReferenceRelative(instruction.C)!);
        var result = instruction.Code switch
        {
            CoflowRegisterOpCode.LessString => comparison < 0,
            CoflowRegisterOpCode.LessOrEqualString => comparison <= 0,
            CoflowRegisterOpCode.GreaterString => comparison > 0,
            CoflowRegisterOpCode.GreaterOrEqualString => comparison >= 0,
            _ => throw new InvalidOperationException($"invalid string comparison opcode `{instruction.Code}`"),
        };
        context.WriteIntegerRelative(instruction.A, result ? 1 : 0);
    }
    private static long PowerInteger(long value, long exponent)
    {
        if (exponent < 0) throw new InvalidOperationException("integer exponent must be non-negative");
        var result = 1L;
        for (var factor = value; exponent != 0; exponent >>= 1)
        {
            if ((exponent & 1) != 0) result = checked(result * factor);
            if (exponent > 1) factor = checked(factor * factor);
        }
        return result;
    }

    private static CoflowFaultException Fault(
        CoflowProgram program,
        string message,
        Exception? inner = null,
        IEnumerable<CoflowFunctionIdentity>? stack = null,
        CfdSpan? span = null) => new(
            program.Identity, program.SourcePath, span ?? program.SourceSpan,
            (stack ?? new[] { program.Identity }).Take(32).ToArray(), message, inner);

    internal interface ICoflowArguments
    {
        int Count { get; }
        void Write(CoflowExecutionContext context);
    }

    private readonly record struct Arguments0 : ICoflowArguments
    {
        public int Count => 0;
        public void Write(CoflowExecutionContext context) { }
    }
    private readonly record struct Arguments1<T1>(T1 Arg1) : ICoflowArguments
    {
        public int Count => 1;
        public void Write(CoflowExecutionContext context) => context.Write(context.Parameter(0), Arg1);
    }
    private readonly record struct Arguments2<T1, T2>(T1 Arg1, T2 Arg2) : ICoflowArguments
    {
        public int Count => 2;
        public void Write(CoflowExecutionContext context) { context.Write(context.Parameter(0), Arg1); context.Write(context.Parameter(1), Arg2); }
    }
    private readonly record struct Arguments3<T1, T2, T3>(T1 Arg1, T2 Arg2, T3 Arg3) : ICoflowArguments
    {
        public int Count => 3;
        public void Write(CoflowExecutionContext context) { context.Write(context.Parameter(0), Arg1); context.Write(context.Parameter(1), Arg2); context.Write(context.Parameter(2), Arg3); }
    }
    private readonly record struct Arguments4<T1, T2, T3, T4>(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4) : ICoflowArguments
    {
        public int Count => 4;
        public void Write(CoflowExecutionContext context) { context.Write(context.Parameter(0), Arg1); context.Write(context.Parameter(1), Arg2); context.Write(context.Parameter(2), Arg3); context.Write(context.Parameter(3), Arg4); }
    }
    private readonly record struct Arguments5<T1, T2, T3, T4, T5>(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4, T5 Arg5) : ICoflowArguments
    {
        public int Count => 5;
        public void Write(CoflowExecutionContext context) { context.Write(context.Parameter(0), Arg1); context.Write(context.Parameter(1), Arg2); context.Write(context.Parameter(2), Arg3); context.Write(context.Parameter(3), Arg4); context.Write(context.Parameter(4), Arg5); }
    }
    private readonly record struct Arguments6<T1, T2, T3, T4, T5, T6>(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4, T5 Arg5, T6 Arg6) : ICoflowArguments
    {
        public int Count => 6;
        public void Write(CoflowExecutionContext context) { context.Write(context.Parameter(0), Arg1); context.Write(context.Parameter(1), Arg2); context.Write(context.Parameter(2), Arg3); context.Write(context.Parameter(3), Arg4); context.Write(context.Parameter(4), Arg5); context.Write(context.Parameter(5), Arg6); }
    }
    private readonly record struct Arguments7<T1, T2, T3, T4, T5, T6, T7>(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4, T5 Arg5, T6 Arg6, T7 Arg7) : ICoflowArguments
    {
        public int Count => 7;
        public void Write(CoflowExecutionContext context) { context.Write(context.Parameter(0), Arg1); context.Write(context.Parameter(1), Arg2); context.Write(context.Parameter(2), Arg3); context.Write(context.Parameter(3), Arg4); context.Write(context.Parameter(4), Arg5); context.Write(context.Parameter(5), Arg6); context.Write(context.Parameter(6), Arg7); }
    }
    private readonly record struct Arguments8<T1, T2, T3, T4, T5, T6, T7, T8>(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4, T5 Arg5, T6 Arg6, T7 Arg7, T8 Arg8) : ICoflowArguments
    {
        public int Count => 8;
        public void Write(CoflowExecutionContext context) { context.Write(context.Parameter(0), Arg1); context.Write(context.Parameter(1), Arg2); context.Write(context.Parameter(2), Arg3); context.Write(context.Parameter(3), Arg4); context.Write(context.Parameter(4), Arg5); context.Write(context.Parameter(5), Arg6); context.Write(context.Parameter(6), Arg7); context.Write(context.Parameter(7), Arg8); }
    }
    private readonly record struct ClosureArguments<TArguments>(CoflowClosure Closure, TArguments Arguments)
        : ICoflowArguments where TArguments : struct, ICoflowArguments
    {
        public int Count => Arguments.Count + Closure.Captures.Count;
        public void Write(CoflowExecutionContext context)
        {
            Arguments.Write(context);
            context.WriteCaptures(Closure, Arguments.Count);
        }
    }

    private struct CoflowFrame
    {
        internal CoflowProgram Program;
        internal int ReturnPc;
        internal int IntegerBase;
        internal int FloatBase;
        internal int ReferenceBase;
        internal int IntegerTop;
        internal int FloatTop;
        internal int ReferenceTop;
        internal CoflowValueRegister ReturnTarget;
    }

    internal sealed class CoflowExecutionContext : IDisposable
    {
        private long[] _integers = ArrayPool<long>.Shared.Rent(32);
        private double[] _floats = ArrayPool<double>.Shared.Rent(16);
        private object?[] _references = RentCleared<object?>(32);
        private CoflowFrame[] _frames = RentCleared<CoflowFrame>(16);
        private int _frameCount;
        private int _frameHighWater;
        private int _integerBase;
        private int _floatBase;
        private int _referenceBase;
        private int _integerTop;
        private int _floatTop;
        private int _referenceTop;
        private int _referenceHighWater;
        internal CoflowExecutionContext? NextPooled { get; set; }

        internal void Reset()
        {
            _frameCount = 0;
            _frameHighWater = 0;
            _integerBase = 0;
            _floatBase = 0;
            _referenceBase = 0;
            _integerTop = 0;
            _floatTop = 0;
            _referenceTop = 0;
            _referenceHighWater = 0;
            Pc = 0;
            Program = null!;
        }
        internal CoflowProgram Program { get; private set; } = null!;
        internal int Pc { get; set; }
        internal IEnumerable<CoflowFunctionIdentity> CallStack =>
            _frames.Take(_frameCount).Select(value => value.Program.Identity).Append(Program.Identity).Reverse();

        internal void Start<TArguments>(CoflowProgram program, TArguments arguments)
            where TArguments : struct, ICoflowArguments
        {
            Program = program;
            Reserve(program.RegisterProgram);
            arguments.Write(this);
        }

        internal CoflowValueRegister Parameter(int index) => Offset(Program.RegisterProgram.Parameters[index]);
        private CoflowValueRegister Offset(CoflowValueRegister register) => register with
        {
            IntegerBase = register.IntegerBase + _integerBase,
            FloatBase = register.FloatBase + _floatBase,
            ReferenceBase = register.ReferenceBase + _referenceBase,
        };
        private static CoflowValueRegister Absolute(
            CoflowValueRegister register,
            int integerBase,
            int floatBase,
            int referenceBase) => register with
            {
                IntegerBase = register.IntegerBase + integerBase,
                FloatBase = register.FloatBase + floatBase,
                ReferenceBase = register.ReferenceBase + referenceBase,
            };

        internal long ReadInteger(CoflowRegister register) => _integers[register.Index];
        internal double ReadFloat(CoflowRegister register) => _floats[register.Index];
        internal object? ReadReference(CoflowRegister register) => _references[register.Index];
        internal void WriteInteger(CoflowRegister register, long value) => _integers[register.Index] = value;
        internal void WriteFloat(CoflowRegister register, double value) => _floats[register.Index] = value;
        internal void WriteReference(CoflowRegister register, object? value) => _references[register.Index] = value;
        internal long ReadIntegerRelative(int index) => _integers[_integerBase + index];
        internal double ReadFloatRelative(int index) => _floats[_floatBase + index];
        internal object? ReadReferenceRelative(int index) => _references[_referenceBase + index];
        internal void WriteIntegerRelative(int index, long value) => _integers[_integerBase + index] = value;
        internal void WriteFloatRelative(int index, double value) => _floats[_floatBase + index] = value;
        internal void WriteReferenceRelative(int index, object? value) => _references[_referenceBase + index] = value;
        internal void Write<T>(CoflowValueRegister register, T value) =>
            CoflowBoundaryCodec<T>.Write(this, register, value);
        internal void Copy(CoflowValueRegister source, CoflowValueRegister target)
        {
            if (source.Shape.Type != target.Shape.Type)
                throw new InvalidOperationException("register value type mismatch");
            switch (source.Shape.Kind)
            {
                case CoflowValueShapeKind.Unit: return;
                case CoflowValueShapeKind.Scalar:
                    switch (source.Shape.ScalarKind)
                    {
                        case CoflowRegisterKind.Integer: WriteInteger(target.Scalar, ReadInteger(source.Scalar)); break;
                        case CoflowRegisterKind.Float: WriteFloat(target.Scalar, ReadFloat(source.Scalar)); break;
                        default: WriteReference(target.Scalar, ReadReference(source.Scalar)); break;
                    }
                    return;
                case CoflowValueShapeKind.Option:
                    WriteInteger(target.Tag, ReadInteger(source.Tag));
                    Copy(source.First, target.First);
                    return;
                case CoflowValueShapeKind.Result:
                    WriteInteger(target.Tag, ReadInteger(source.Tag));
                    Copy(source.First, target.First);
                    Copy(source.Second, target.Second);
                    return;
            }
        }

        internal void CopyRelative(CoflowValueRegister source, CoflowValueRegister target)
        {
            if (source.Shape.Kind != target.Shape.Kind)
                throw new InvalidOperationException(
                    $"register value kind mismatch: `{source.Shape.Type}` to `{target.Shape.Type}`");
            switch (source.Shape.Kind)
            {
                case CoflowValueShapeKind.Unit: return;
                case CoflowValueShapeKind.Scalar:
                    switch (source.Shape.ScalarKind)
                    {
                        case CoflowRegisterKind.Integer:
                            WriteIntegerRelative(target.IntegerBase, ReadIntegerRelative(source.IntegerBase));
                            break;
                        case CoflowRegisterKind.Float:
                            WriteFloatRelative(target.FloatBase, ReadFloatRelative(source.FloatBase));
                            break;
                        default:
                            WriteReferenceRelative(target.ReferenceBase, ReadReferenceRelative(source.ReferenceBase));
                            break;
                    }
                    return;
                case CoflowValueShapeKind.Option:
                    WriteIntegerRelative(target.IntegerBase, ReadIntegerRelative(source.IntegerBase));
                    CopyRelative(source.First, target.First);
                    return;
                case CoflowValueShapeKind.Result:
                    WriteIntegerRelative(target.IntegerBase, ReadIntegerRelative(source.IntegerBase));
                    CopyRelative(source.First, target.First);
                    CopyRelative(source.Second, target.Second);
                    return;
            }
        }

        internal void WriteEncodedRelative(CoflowEncodedValue source, CoflowValueRegister target)
        {
            if (source.Integers.Length == 1 && source.Floats.Length == 0 && source.References.Length == 0)
            {
                WriteIntegerRelative(target.IntegerBase, source.Integers[0]);
                return;
            }
            if (source.Floats.Length == 1 && source.Integers.Length == 0 && source.References.Length == 0)
            {
                WriteFloatRelative(target.FloatBase, source.Floats[0]);
                return;
            }
            if (source.References.Length == 1 && source.Integers.Length == 0 && source.Floats.Length == 0)
            {
                WriteReferenceRelative(target.ReferenceBase, source.References[0]);
                return;
            }
            if (source.Integers.Length != 0)
                Array.Copy(source.Integers, 0, _integers, _integerBase + target.IntegerBase, source.Integers.Length);
            if (source.Floats.Length != 0)
                Array.Copy(source.Floats, 0, _floats, _floatBase + target.FloatBase, source.Floats.Length);
            if (source.References.Length != 0)
                Array.Copy(source.References, 0, _references,
                    _referenceBase + target.ReferenceBase, source.References.Length);
        }

        internal bool Call(CoflowRegisterCallSite site, bool tail)
        {
            var target = site.Entry.CompiledProgram;
            if (target is null) return false;
            if (tail && ReferenceEquals(target, Program))
            {
                for (var index = 0; index < site.Arguments.Length; index++)
                    CopyRelative(site.Arguments[index], Program.RegisterProgram.Parameters[index]);
                Pc = 0;
                return true;
            }
            EnterFromRegisters(target, site.Arguments, tail, Offset(site.Result));
            return true;
        }

        private void EnterFromRegisters(
            CoflowProgram target,
            CoflowValueRegister[] arguments,
            bool tail,
            CoflowValueRegister returnTarget)
        {
            var caller = Program;
            var callerIntegerBase = _integerBase;
            var callerFloatBase = _floatBase;
            var callerReferenceBase = _referenceBase;
            var callerIntegerTop = _integerTop;
            var callerFloatTop = _floatTop;
            var callerReferenceTop = _referenceTop;
            if (!tail)
            {
                PushFrame(returnTarget);
                _integerBase = _integerTop;
                _floatBase = _floatTop;
                _referenceBase = _referenceTop;
            }
            else
            {
                _integerBase = callerIntegerTop;
                _floatBase = callerFloatTop;
                _referenceBase = callerReferenceTop;
            }
            Program = target;
            Pc = 0;
            Reserve(target.RegisterProgram);
            for (var index = 0; index < arguments.Length; index++)
            {
                var source = Absolute(
                    arguments[index],
                    callerIntegerBase,
                    callerFloatBase,
                    callerReferenceBase);
                Copy(source, Parameter(index));
            }
            if (tail) CompactTailWindow(target.RegisterProgram,
                callerIntegerBase, callerFloatBase, callerReferenceBase, callerReferenceTop);
        }

        internal bool CallIndirect<TResult>(
            CoflowRegisterIndirectCallSite site,
            bool tail,
            out TResult returned)
        {
            returned = default!;
            var callable = ReadReferenceRelative(site.Callable.ReferenceBase);
            if (callable is CoflowFunctionEntry entry)
            {
                if (entry.CompiledProgram is { } compiled)
                {
                    EnterFromRegisters(compiled, site.Arguments, tail, Offset(site.Result));
                    return false;
                }
                entry.InvokeBoundFromVm(new CoflowNativeFrame(
                    this, site.Arguments, site.Result, site.ResultType));
                return tail && ReturnRegister(site.Result, out returned);
            }
            if (callable is CoflowClosure closure)
            {
                EnterClosureFromRegisters(closure, site.Arguments, tail, Offset(site.Result));
                return false;
            }
            if (callable is Delegate implementation)
            {
                var descriptor = CoflowFunctionDelegates.Callable(implementation);
                if (descriptor.Entry is { } adaptedEntry)
                {
                    if (adaptedEntry.CompiledProgram is { } compiled)
                    {
                        EnterFromRegisters(compiled, site.Arguments, tail, Offset(site.Result));
                        return false;
                    }
                    adaptedEntry.InvokeBoundFromVm(new CoflowNativeFrame(
                        this, site.Arguments, site.Result, site.ResultType));
                }
                else if (descriptor.Closure is { } adaptedClosure)
                {
                    EnterClosureFromRegisters(adaptedClosure, site.Arguments, tail, Offset(site.Result));
                    return false;
                }
                else descriptor.NativeCall!.Invoke(new CoflowNativeFrame(
                    this, site.Arguments, site.Result, site.ResultType));
                if (tail && ReturnRegister(site.Result, out returned)) return true;
                return false;
            }
            throw new InvalidOperationException("indirect call target is not callable");
        }

        private void EnterClosureFromRegisters(
            CoflowClosure closure,
            CoflowValueRegister[] arguments,
            bool tail,
            CoflowValueRegister returnTarget)
        {
            var caller = Program;
            var callerIntegerBase = _integerBase;
            var callerFloatBase = _floatBase;
            var callerReferenceBase = _referenceBase;
            var callerIntegerTop = _integerTop;
            var callerFloatTop = _floatTop;
            var callerReferenceTop = _referenceTop;
            if (!tail)
            {
                PushFrame(returnTarget);
                _integerBase = _integerTop;
                _floatBase = _floatTop;
                _referenceBase = _referenceTop;
            }
            else
            {
                _integerBase = callerIntegerTop;
                _floatBase = callerFloatTop;
                _referenceBase = callerReferenceTop;
            }
            Program = closure.Program;
            Pc = 0;
            Reserve(closure.Program.RegisterProgram);
            for (var index = 0; index < arguments.Length; index++)
            {
                var source = Absolute(
                    arguments[index],
                    callerIntegerBase,
                    callerFloatBase,
                    callerReferenceBase);
                Copy(source, Parameter(index));
            }
            WriteCaptures(closure, arguments.Length);
            if (tail) CompactTailWindow(closure.Program.RegisterProgram,
                callerIntegerBase, callerFloatBase, callerReferenceBase, callerReferenceTop);
        }

        private void CompactTailWindow(
            CoflowRegisterProgram program,
            int integerBase,
            int floatBase,
            int referenceBase,
            int previousReferenceTop)
        {
            var scratchIntegerBase = _integerBase;
            var scratchFloatBase = _floatBase;
            var scratchReferenceBase = _referenceBase;
            if (program.ParameterIntegerCount != 0)
                Array.Copy(_integers, scratchIntegerBase, _integers, integerBase, program.ParameterIntegerCount);
            if (program.ParameterFloatCount != 0)
                Array.Copy(_floats, scratchFloatBase, _floats, floatBase, program.ParameterFloatCount);
            if (previousReferenceTop > referenceBase)
                Array.Clear(_references, referenceBase, previousReferenceTop - referenceBase);
            if (program.ParameterReferenceCount != 0)
                Array.Copy(_references, scratchReferenceBase, _references, referenceBase, program.ParameterReferenceCount);
            if (program.ReferenceRegisterCount > program.ParameterReferenceCount)
                Array.Clear(_references, referenceBase + program.ParameterReferenceCount,
                    program.ReferenceRegisterCount - program.ParameterReferenceCount);
            if (program.ReferenceRegisterCount != 0 && scratchReferenceBase >= referenceBase + program.ReferenceRegisterCount)
                Array.Clear(_references, scratchReferenceBase, program.ReferenceRegisterCount);
            _integerBase = integerBase;
            _floatBase = floatBase;
            _referenceBase = referenceBase;
            _integerTop = checked(integerBase + program.IntegerRegisterCount);
            _floatTop = checked(floatBase + program.FloatRegisterCount);
            _referenceTop = checked(referenceBase + program.ReferenceRegisterCount);
        }

        internal bool ReturnRegister<TResult>(CoflowValueRegister source, out TResult root)
        {
            if (_frameCount == 0)
            {
                root = CoflowBoundaryCodec<TResult>.ReadRelative(this, source);
                return true;
            }
            var frame = _frames[_frameCount - 1];
            Copy(Offset(source), frame.ReturnTarget);
            ClearCurrentReferences();
            _frameCount--;
            Program = frame.Program;
            Pc = frame.ReturnPc;
            _integerBase = frame.IntegerBase;
            _floatBase = frame.FloatBase;
            _referenceBase = frame.ReferenceBase;
            _integerTop = frame.IntegerTop;
            _floatTop = frame.FloatTop;
            _referenceTop = frame.ReferenceTop;
            root = default!;
            return false;
        }

        internal void MakeClosure(CoflowRegisterClosureSite site)
        {
            var template = site.Template;
            var closure = CoflowClosure.Create(template.Program, template.Captures,
                template.IntegerCount, template.FloatCount, template.ReferenceCount);
            for (var index = 0; index < template.CaptureCount; index++)
                Capture(Offset(site.Captures[index]), template.Captures[index], closure);
            WriteReferenceRelative(site.Target.ReferenceBase, closure);
        }

        private void Capture(
            CoflowValueRegister source,
            CoflowCaptureLayout target,
            CoflowClosure closure)
        {
            if (source.Shape.Kind == CoflowValueShapeKind.Unit) return;
            if (source.Shape.Kind == CoflowValueShapeKind.Scalar)
            {
                switch (source.Shape.ScalarKind)
                {
                    case CoflowRegisterKind.Integer: closure.SetInteger(target.IntegerBase, ReadInteger(source.Scalar)); break;
                    case CoflowRegisterKind.Float: closure.SetFloat(target.FloatBase, ReadFloat(source.Scalar)); break;
                    default: closure.SetReference(target.ReferenceBase, ReadReference(source.Scalar)); break;
                }
                return;
            }
            closure.SetInteger(target.IntegerBase, ReadInteger(source.Tag));
            Capture(source.First, new CoflowCaptureLayout(source.Shape.First!, target.IntegerBase + 1,
                target.FloatBase, target.ReferenceBase), closure);
            if (source.Shape.Kind == CoflowValueShapeKind.Result)
                Capture(source.Second, new CoflowCaptureLayout(source.Shape.Second!,
                    target.IntegerBase + 1 + source.Shape.First!.IntegerCount,
                    target.FloatBase + source.Shape.First.FloatCount,
                    target.ReferenceBase + source.Shape.First.ReferenceCount), closure);
        }

        private void Restore(
            CoflowCaptureLayout source,
            CoflowValueRegister target,
            CoflowClosure closure)
        {
            if (target.Shape.Type != source.Shape.Type)
                throw new InvalidOperationException("closure capture type mismatch");
            if (target.Shape.Kind == CoflowValueShapeKind.Unit) return;
            if (target.Shape.Kind == CoflowValueShapeKind.Scalar)
            {
                switch (target.Shape.ScalarKind)
                {
                    case CoflowRegisterKind.Integer: WriteInteger(target.Scalar, closure.Integer(source.IntegerBase)); break;
                    case CoflowRegisterKind.Float: WriteFloat(target.Scalar, closure.Float(source.FloatBase)); break;
                    default: WriteReference(target.Scalar, closure.Reference(source.ReferenceBase)); break;
                }
                return;
            }
            WriteInteger(target.Tag, closure.Integer(source.IntegerBase));
            Restore(new CoflowCaptureLayout(target.Shape.First!, source.IntegerBase + 1,
                source.FloatBase, source.ReferenceBase), target.First, closure);
            if (target.Shape.Kind == CoflowValueShapeKind.Result)
                Restore(new CoflowCaptureLayout(target.Shape.Second!,
                    source.IntegerBase + 1 + target.Shape.First!.IntegerCount,
                    source.FloatBase + target.Shape.First.FloatCount,
                    source.ReferenceBase + target.Shape.First.ReferenceCount),
                    target.Second, closure);
        }

        internal void WriteCaptures(CoflowClosure closure, int parameterOffset)
        {
            for (var index = 0; index < closure.Captures.Count; index++)
                Restore(closure.Captures[index], Parameter(parameterOffset + index), closure);
        }

        private void PushFrame(CoflowValueRegister returnTarget)
        {
            EnsureFrames(_frameCount + 1);
            _frames[_frameCount++] = new CoflowFrame {
                Program = Program, ReturnPc = Pc,
                IntegerBase = _integerBase, FloatBase = _floatBase, ReferenceBase = _referenceBase,
                IntegerTop = _integerTop, FloatTop = _floatTop, ReferenceTop = _referenceTop,
                ReturnTarget = returnTarget,
            };
            _frameHighWater = Math.Max(_frameHighWater, _frameCount);
        }

        private void Reserve(CoflowRegisterProgram program)
        {
            _integerTop = checked(_integerBase + program.IntegerRegisterCount);
            _floatTop = checked(_floatBase + program.FloatRegisterCount);
            _referenceTop = checked(_referenceBase + program.ReferenceRegisterCount);
            Ensure(ref _integers, _integerTop);
            Ensure(ref _floats, _floatTop);
            Ensure(ref _references, _referenceTop);
            _referenceHighWater = Math.Max(_referenceHighWater, _referenceTop);
        }

        private void ClearCurrentReferences()
        {
            if (_referenceTop > _referenceBase) Array.Clear(_references, _referenceBase, _referenceTop - _referenceBase);
        }

        private static void Ensure<T>(ref T[] values, int count)
        {
            if (count <= values.Length) return;
            var replacement = ArrayPool<T>.Shared.Rent(Math.Max(count, checked(values.Length * 2)));
            var containsReferences = System.Runtime.CompilerServices.RuntimeHelpers
                .IsReferenceOrContainsReferences<T>();
            if (containsReferences) Array.Clear(replacement, 0, replacement.Length);
            Array.Copy(values, replacement, values.Length);
            if (containsReferences) Array.Clear(values, 0, values.Length);
            ArrayPool<T>.Shared.Return(values);
            values = replacement;
        }
        private static T[] RentCleared<T>(int count)
        {
            var values = ArrayPool<T>.Shared.Rent(count);
            Array.Clear(values, 0, values.Length);
            return values;
        }
        private void EnsureFrames(int count) => Ensure(ref _frames, count);

        public void Dispose()
        {
            if (_referenceHighWater != 0) Array.Clear(_references, 0, _referenceHighWater);
            if (_frameHighWater != 0) Array.Clear(_frames, 0, _frameHighWater);
            Program = null!;
            NextPooled = _pooledContexts;
            _pooledContexts = this;
        }

    }

}
