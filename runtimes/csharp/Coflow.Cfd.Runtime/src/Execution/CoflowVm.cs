namespace CoflowRuntime;

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
    int Operand2 = 0,
    int Operand3 = 0,
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
    private readonly int _pc;
    private readonly int _depth;
    private readonly int _argumentCount;
    private readonly Type _resultType;
    private readonly int _resultDepth;
    private readonly CoflowNativeCallSite? _site;

    internal CoflowNativeFrame(
        CoflowVm.CoflowExecutionContext context,
        CoflowNativeCallSite site)
    {
        _context = context;
        _site = site;
        _pc = 0;
        _depth = 0;
        _argumentCount = site.Arguments.Length;
        _resultType = site.Call.ResultType;
        _resultDepth = 0;
    }

    internal CoflowNativeFrame(
        CoflowVm.CoflowExecutionContext context,
        int pc,
        int depth,
        int argumentCount,
        Type resultType)
    {
        _context = context;
        _pc = pc;
        _depth = depth;
        _argumentCount = argumentCount;
        _resultType = resultType;
        _resultDepth = depth - argumentCount;
        _site = null;
    }

    internal CoflowNativeFrame(
        CoflowVm.CoflowExecutionContext context,
        int pc,
        int argumentStart,
        int resultDepth,
        int argumentCount,
        Type resultType)
    {
        _context = context;
        _pc = pc;
        _depth = argumentStart + argumentCount;
        _argumentCount = argumentCount;
        _resultType = resultType;
        _resultDepth = resultDepth;
        _site = null;
    }

    public T Read<T>(int index) => _site is { } site
        ? CoflowBoundaryCodec<T>.ReadRelative(_context, site.Arguments[index])
        : CoflowBoundaryCodec<T>.Read(
            _context, _context.Stack(_pc, _depth - _argumentCount + index));

    public void Write<T>(T value)
    {
        if (typeof(T) != _resultType)
            throw new InvalidOperationException($"native result `{typeof(T)}` does not match `{_resultType}`");
        if (_site is { } site)
            CoflowBoundaryCodec<T>.WriteRelative(_context, site.Result, value);
        else _context.Write(_context.Temporary(_resultType, _resultDepth), value);
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
internal sealed record CoflowRange(long Start, long End, bool Inclusive)
{
    internal long Count => End <= Start
        ? Inclusive && End == Start ? 1 : 0
        : checked(End - Start + (Inclusive ? 1 : 0));
}
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
        Instructions = instructions.ToArray();
        InstructionSpans = instructionSpans.ToArray();
        if (Instructions.Length == 0)
            throw new InvalidOperationException($"Coflow program `{identity}` has no instructions.");
        if (InstructionSpans.Length != Instructions.Length)
            throw new InvalidOperationException($"Coflow program `{identity}` has an invalid source map.");
        if (localCount < 0)
            throw new InvalidOperationException($"Coflow program `{identity}` has a negative local count.");
        var sourceConstants = constants.ToArray();
        Operations = sourceConstants.ToArray();
        EncodedConstants = new CoflowEncodedValue?[sourceConstants.Length];
        foreach (var instruction in Instructions)
            if (instruction.Code == CoflowOpCode.Constant)
            {
                if ((uint)instruction.Operand >= (uint)sourceConstants.Length)
                    throw new InvalidOperationException($"Coflow program `{identity}` has an invalid constant index.");
                EncodedConstants[instruction.Operand] ??= CoflowEncodedValue.Encode(
                    instruction.ValueType ?? sourceConstants[instruction.Operand]?.GetType() ?? typeof(object),
                    sourceConstants[instruction.Operand]);
                Operations[instruction.Operand] = null;
            }
        ParameterTypes = parameterTypes.ToArray();
        ReturnType = returnType;
        LocalCount = localCount;
        RegisterProgram = CoflowRegisterLowering.Lower(this);
    }

    internal CoflowFunctionIdentity Identity { get; }
    internal string SourcePath { get; }
    internal CfdSpan? SourceSpan { get; }
    internal CoflowInstruction[] Instructions { get; }
    internal CfdSpan?[] InstructionSpans { get; }
    internal object?[] Operations { get; }
    internal CoflowEncodedValue?[] EncodedConstants { get; }
    internal Type[] ParameterTypes { get; }
    internal Type ReturnType { get; }
    internal int ParameterCount => ParameterTypes.Length;
    internal int LocalCount { get; }
    internal CoflowRegisterProgram RegisterProgram { get; }
}

public sealed class CoflowFaultException : Exception
{
    internal CoflowFaultException(
        CoflowFunctionIdentity function,
        string sourcePath,
        CfdSpan? sourceSpan,
        IReadOnlyList<CoflowFunctionIdentity> callStack,
        string message,
        Exception? inner = null,
        bool preserveSourceLocation = false) : base(message, inner)
    {
        Function = function;
        SourcePath = sourcePath;
        SourceSpan = sourceSpan;
        CallStack = callStack;
        PreserveSourceLocation = preserveSourceLocation;
    }
    public CoflowFunctionIdentity Function { get; }
    public string SourcePath { get; }
    public CfdSpan? SourceSpan { get; }
    public IReadOnlyList<CoflowFunctionIdentity> CallStack { get; }
    internal bool PreserveSourceLocation { get; }
    internal CoflowFaultException WithCallers(
        IEnumerable<CoflowFunctionIdentity> callers,
        string? callerSourcePath = null,
        CfdSpan? callerSourceSpan = null) => new(
            Function,
            callerSourcePath ?? SourcePath,
            callerSourceSpan ?? SourceSpan,
            CallStack.Concat(callers).Distinct().Take(32).ToArray(),
            Message,
            InnerException,
            PreserveSourceLocation);
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
                var depth = instruction.StackDepth;
                switch (instruction.Code)
                {
                    case CoflowOpCode.Constant:
                    {
                        var encoded = instruction.Constant
                            ?? throw new InvalidOperationException("constant has no encoded layout");
                        context.WriteEncodedRelative(encoded,
                            registers.Temporary(encoded.Shape, depth));
                        break;
                    }
                    case CoflowOpCode.Argument:
                    {
                        var source = registers.Parameters[instruction.Operand];
                        context.CopyRelative(source, registers.Temporary(source.Shape, depth));
                        break;
                    }
                    case CoflowOpCode.Local:
                    {
                        var source = registers.Locals[instruction.Operand];
                        context.CopyRelative(source, registers.Temporary(source.Shape, depth));
                        break;
                    }
                    case CoflowOpCode.StoreLocal:
                        context.CopyRelative(registers.Stack(pc, depth - 1),
                            registers.Locals[instruction.Operand]);
                        break;
                    case CoflowOpCode.LoadField:
                    {
                        var access = (CoflowFieldAccess)instruction.Operation!;
                        var receiver = context.ReadReferenceRelative(
                            registers.Stack(pc, depth - 1).ReferenceBase)
                            ?? throw new InvalidOperationException($"field `{access.Name}` receiver is null");
                        var target = registers.Temporaries[depth - 1];
                        if (access.ReadInteger is { } readInteger)
                            context.WriteIntegerRelative(target.IntegerBase, readInteger(receiver));
                        else if (access.ReadFloat is { } readFloat)
                            context.WriteFloatRelative(target.FloatBase, readFloat(receiver));
                        else if (access.ReadReference is { } readReference)
                            context.WriteReferenceRelative(target.ReferenceBase, readReference(receiver));
                        else
                            access.Call.Invoke(new CoflowNativeFrame(
                                context, pc, depth, 1, access.RuntimeType));
                        break;
                    }
                    case CoflowOpCode.Native:
                    {
                        var site = registers.NativeCallSites[pc]
                            ?? throw new InvalidOperationException("native instruction has no lowered call site");
                        site.Call.Invoke(new CoflowNativeFrame(context, site));
                        break;
                    }
                    case CoflowOpCode.MakeOptionSome:
                    case CoflowOpCode.MakeResultOk:
                    case CoflowOpCode.MakeResultErr:
                    {
                        var source = context.Stack(pc, depth - 1);
                        var target = context.Temporary(instruction.ValueType!, depth - 1);
                        context.CopyPhysical(source,
                            instruction.Code == CoflowOpCode.MakeResultErr ? target.Second : target.First);
                        context.WriteInteger(target.Tag,
                            instruction.Code == CoflowOpCode.MakeResultErr ? 0 : 1);
                        break;
                    }
                    case CoflowOpCode.MakeOptionNone:
                    {
                        var target = context.Temporary(instruction.ValueType!, depth);
                        context.WriteInteger(target.Tag, 0);
                        break;
                    }
                    case CoflowOpCode.ReadValueTag:
                        context.WriteInteger(context.Temporary(typeof(bool), depth - 1).Scalar,
                            context.ReadInteger(context.Stack(pc, depth - 1).Tag));
                        break;
                    case CoflowOpCode.ReadFirstPayload:
                        context.CopyPhysical(context.Stack(pc, depth - 1).First,
                            context.Temporary(instruction.ValueType!, depth - 1));
                        break;
                    case CoflowOpCode.ReadSecondPayload:
                        context.CopyPhysical(context.Stack(pc, depth - 1).Second,
                            context.Temporary(instruction.ValueType!, depth - 1));
                        break;
                    case CoflowOpCode.Propagate:
                    {
                        var source = context.Stack(pc, depth - 1);
                        if (context.ReadInteger(source.Tag) == 0)
                        {
                            var returnValue = context.Temporary(context.Program.ReturnType, depth - 1);
                            context.WriteInteger(returnValue.Tag, 0);
                            if (source.Shape.Kind == CoflowValueShapeKind.Result)
                                context.CopyPhysical(source.Second, returnValue.Second);
                            if (context.ReturnRegister<TResult>(returnValue, out var returned)) return returned;
                        }
                        else context.CopyPhysical(source.First,
                            context.Temporary(instruction.ValueType!, depth - 1));
                        break;
                    }
                    case CoflowOpCode.MakeClosure:
                        context.MakeClosure(pc, depth,
                            (CoflowClosureTemplate)instruction.Operation!,
                            instruction.ValueType!);
                        break;
                    case CoflowOpCode.Pop: break;
                    case CoflowOpCode.Reinterpret:
                        context.CopyPhysical(context.Stack(pc, depth - 1),
                            context.Temporary(instruction.ValueType!, depth - 1));
                        break;
                    case CoflowOpCode.ConvertIntToFloat:
                        context.WriteFloatRelative(registers.Temporaries[depth - 1].FloatBase,
                            context.ReadIntegerRelative(registers.Stack(pc, depth - 1).IntegerBase));
                        break;
                    case CoflowOpCode.ConvertFloatToInt:
                        context.WriteIntegerRelative(registers.Temporaries[depth - 1].IntegerBase,
                            checked((long)context.ReadFloatRelative(registers.Stack(pc, depth - 1).FloatBase)));
                        break;
                    case CoflowOpCode.IsType:
                        context.WriteInteger(context.Temporary(typeof(bool), depth - 1).Scalar,
                            instruction.Operation is Type type &&
                            type.IsInstanceOfType(context.ReadReference(context.Stack(pc, depth - 1).Scalar)) ? 1 : 0);
                        break;
                    case CoflowOpCode.NegateInt:
                    case CoflowOpCode.Not:
                    case CoflowOpCode.BitNot: UnaryInteger(context, pc, depth, instruction.Code); break;
                    case CoflowOpCode.NegateFloat: UnaryFloat(context, pc, depth); break;
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
                    case CoflowOpCode.BitOr: BinaryInteger(context, pc, depth, instruction.Code); break;
                    case CoflowOpCode.AddFloat:
                    case CoflowOpCode.SubtractFloat:
                    case CoflowOpCode.MultiplyFloat:
                    case CoflowOpCode.DivideFloat:
                    case CoflowOpCode.PowerFloat: BinaryFloat(context, pc, depth, instruction.Code); break;
                    case CoflowOpCode.AddString: AddString(context, pc, depth); break;
                    case CoflowOpCode.LessInt:
                    case CoflowOpCode.LessOrEqualInt:
                    case CoflowOpCode.GreaterInt:
                    case CoflowOpCode.GreaterOrEqualInt:
                    case CoflowOpCode.EqualInteger: CompareInteger(context, pc, depth, instruction.Code); break;
                    case CoflowOpCode.LessFloat:
                    case CoflowOpCode.LessOrEqualFloat:
                    case CoflowOpCode.GreaterFloat:
                    case CoflowOpCode.GreaterOrEqualFloat:
                    case CoflowOpCode.EqualFloat: CompareFloat(context, pc, depth, instruction.Code); break;
                    case CoflowOpCode.LessString:
                    case CoflowOpCode.LessOrEqualString:
                    case CoflowOpCode.GreaterString:
                    case CoflowOpCode.GreaterOrEqualString: CompareString(context, pc, depth, instruction.Code); break;
                    case CoflowOpCode.EqualReference:
                        context.WriteIntegerRelative(registers.Temporaries[depth - 2].IntegerBase,
                            Equals(context.ReadReferenceRelative(registers.Stack(pc, depth - 2).ReferenceBase),
                                context.ReadReferenceRelative(registers.Stack(pc, depth - 1).ReferenceBase)) ? 1 : 0);
                        break;
                    case CoflowOpCode.JumpIfFalseKeep:
                        if (context.ReadIntegerRelative(registers.Stack(pc, depth - 1).IntegerBase) == 0)
                            context.Pc = instruction.Operand;
                        break;
                    case CoflowOpCode.JumpIfTrueKeep:
                        if (context.ReadIntegerRelative(registers.Stack(pc, depth - 1).IntegerBase) != 0)
                            context.Pc = instruction.Operand;
                        break;
                    case CoflowOpCode.JumpIfFalse:
                        if (context.ReadIntegerRelative(registers.Stack(pc, depth - 1).IntegerBase) == 0)
                            context.Pc = instruction.Operand;
                        break;
                    case CoflowOpCode.Jump: context.Pc = instruction.Operand; break;
                    case CoflowOpCode.Call:
                    {
                        var call = (CoflowCallSite)instruction.Operation!;
                        if (!context.Call(call.Entry, pc, depth, call.ArgumentCount, tail: false))
                            call.Entry.InvokeBoundFromVm(new CoflowNativeFrame(
                                context, pc, depth, call.ArgumentCount, call.Entry.Signature.ResultType));
                        break;
                    }
                    case CoflowOpCode.CallIndirect:
                        if (context.CallIndirect<TResult>(pc, depth, instruction.Operand, tail: false,
                                out var indirectResult)) return indirectResult;
                        break;
                    case CoflowOpCode.TailCall:
                    {
                        var call = (CoflowCallSite)instruction.Operation!;
                        if (!context.Call(call.Entry, pc, depth, call.ArgumentCount, tail: true))
                        {
                            call.Entry.InvokeBoundFromVm(new CoflowNativeFrame(
                                context, pc, depth, call.ArgumentCount, call.Entry.Signature.ResultType));
                            if (context.ReturnRegister<TResult>(
                                    context.Temporary(call.Entry.Signature.ResultType, depth - call.ArgumentCount),
                                    out var returned)) return returned;
                        }
                        break;
                    }
                    case CoflowOpCode.TailCallIndirect:
                        if (context.CallIndirect<TResult>(pc, depth, instruction.Operand, tail: true,
                                out var indirectTailResult)) return indirectTailResult;
                        break;
                    case CoflowOpCode.Return:
                    {
                        if (context.ReturnRegister<TResult>(context.Stack(pc, depth - 1), out var returned)) return returned;
                        break;
                    }
                    default: throw new InvalidOperationException($"Unknown Coflow opcode `{instruction.Code}`.");
                }
            }
        }
        catch (CoflowFaultException) { throw; }
        catch (Exception error)
        {
            var span = context.Pc > 0 && context.Pc <= context.Program.InstructionSpans.Length
                ? context.Program.InstructionSpans[context.Pc - 1] : null;
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

    private static void UnaryInteger(CoflowExecutionContext context, int pc, int depth, CoflowOpCode code)
    {
        var register = context.Program.RegisterProgram.Stack(pc, depth - 1).IntegerBase;
        var value = context.ReadIntegerRelative(register);
        context.WriteIntegerRelative(register, code switch
        {
            CoflowOpCode.NegateInt => checked(-value),
            CoflowOpCode.Not => value == 0 ? 1 : 0,
            CoflowOpCode.BitNot => ~value,
            _ => throw new InvalidOperationException($"invalid unary integer opcode `{code}`"),
        });
    }
    private static void UnaryFloat(CoflowExecutionContext context, int pc, int depth)
    {
        var register = context.Program.RegisterProgram.Stack(pc, depth - 1).FloatBase;
        context.WriteFloatRelative(register, -context.ReadFloatRelative(register));
    }
    private static void BinaryInteger(CoflowExecutionContext context, int pc, int depth, CoflowOpCode code)
    {
        var registers = context.Program.RegisterProgram;
        var left = registers.Stack(pc, depth - 2).IntegerBase;
        var leftValue = context.ReadIntegerRelative(left);
        var rightValue = context.ReadIntegerRelative(registers.Stack(pc, depth - 1).IntegerBase);
        context.WriteIntegerRelative(left, code switch
        {
            CoflowOpCode.AddInt => checked(leftValue + rightValue),
            CoflowOpCode.SubtractInt => checked(leftValue - rightValue),
            CoflowOpCode.MultiplyInt => checked(leftValue * rightValue),
            CoflowOpCode.DivideInt or CoflowOpCode.IntegerDivide => checked(leftValue / rightValue),
            CoflowOpCode.Remainder => checked(leftValue % rightValue),
            CoflowOpCode.PowerInt => PowerInteger(leftValue, rightValue),
            CoflowOpCode.ShiftLeft => checked(leftValue << checked((int)rightValue)),
            CoflowOpCode.ShiftRight => leftValue >> checked((int)rightValue),
            CoflowOpCode.BitAnd => leftValue & rightValue,
            CoflowOpCode.BitXor => leftValue ^ rightValue,
            CoflowOpCode.BitOr => leftValue | rightValue,
            _ => throw new InvalidOperationException($"invalid binary integer opcode `{code}`"),
        });
    }
    private static void BinaryFloat(CoflowExecutionContext context, int pc, int depth, CoflowOpCode code)
    {
        var registers = context.Program.RegisterProgram;
        var left = registers.Stack(pc, depth - 2).FloatBase;
        var leftValue = context.ReadFloatRelative(left);
        var rightValue = context.ReadFloatRelative(registers.Stack(pc, depth - 1).FloatBase);
        context.WriteFloatRelative(left, code switch
        {
            CoflowOpCode.AddFloat => leftValue + rightValue,
            CoflowOpCode.SubtractFloat => leftValue - rightValue,
            CoflowOpCode.MultiplyFloat => leftValue * rightValue,
            CoflowOpCode.DivideFloat => leftValue / rightValue,
            CoflowOpCode.PowerFloat => Math.Pow(leftValue, rightValue),
            _ => throw new InvalidOperationException($"invalid binary float opcode `{code}`"),
        });
    }
    private static void AddString(CoflowExecutionContext context, int pc, int depth)
    {
        var registers = context.Program.RegisterProgram;
        var left = registers.Stack(pc, depth - 2).ReferenceBase;
        context.WriteReferenceRelative(left,
            (string)context.ReadReferenceRelative(left)! +
            (string)context.ReadReferenceRelative(registers.Stack(pc, depth - 1).ReferenceBase)!);
    }
    private static void CompareInteger(CoflowExecutionContext context, int pc, int depth, CoflowOpCode code)
    {
        var registers = context.Program.RegisterProgram;
        var left = registers.Stack(pc, depth - 2).IntegerBase;
        var leftValue = context.ReadIntegerRelative(left);
        var rightValue = context.ReadIntegerRelative(registers.Stack(pc, depth - 1).IntegerBase);
        var result = code switch
        {
            CoflowOpCode.LessInt => leftValue < rightValue,
            CoflowOpCode.LessOrEqualInt => leftValue <= rightValue,
            CoflowOpCode.GreaterInt => leftValue > rightValue,
            CoflowOpCode.GreaterOrEqualInt => leftValue >= rightValue,
            CoflowOpCode.EqualInteger => leftValue == rightValue,
            _ => throw new InvalidOperationException($"invalid integer comparison opcode `{code}`"),
        };
        context.WriteIntegerRelative(registers.Temporaries[depth - 2].IntegerBase, result ? 1 : 0);
    }
    private static void CompareFloat(CoflowExecutionContext context, int pc, int depth, CoflowOpCode code)
    {
        var registers = context.Program.RegisterProgram;
        var leftValue = context.ReadFloatRelative(registers.Stack(pc, depth - 2).FloatBase);
        var rightValue = context.ReadFloatRelative(registers.Stack(pc, depth - 1).FloatBase);
        var result = code switch
        {
            CoflowOpCode.LessFloat => leftValue < rightValue,
            CoflowOpCode.LessOrEqualFloat => leftValue <= rightValue,
            CoflowOpCode.GreaterFloat => leftValue > rightValue,
            CoflowOpCode.GreaterOrEqualFloat => leftValue >= rightValue,
            CoflowOpCode.EqualFloat => leftValue.Equals(rightValue),
            _ => throw new InvalidOperationException($"invalid float comparison opcode `{code}`"),
        };
        context.WriteIntegerRelative(registers.Temporaries[depth - 2].IntegerBase, result ? 1 : 0);
    }
    private static void CompareString(CoflowExecutionContext context, int pc, int depth, CoflowOpCode code)
    {
        var registers = context.Program.RegisterProgram;
        var comparison = string.CompareOrdinal(
            (string)context.ReadReferenceRelative(registers.Stack(pc, depth - 2).ReferenceBase)!,
            (string)context.ReadReferenceRelative(registers.Stack(pc, depth - 1).ReferenceBase)!);
        var result = code switch
        {
            CoflowOpCode.LessString => comparison < 0,
            CoflowOpCode.LessOrEqualString => comparison <= 0,
            CoflowOpCode.GreaterString => comparison > 0,
            CoflowOpCode.GreaterOrEqualString => comparison >= 0,
            _ => throw new InvalidOperationException($"invalid string comparison opcode `{code}`"),
        };
        context.WriteIntegerRelative(registers.Temporaries[depth - 2].IntegerBase, result ? 1 : 0);
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

        internal CoflowValueRegister Stack(int pc, int depth) => Offset(Program.RegisterProgram.Stack(pc, depth));
        internal CoflowValueRegister Temporary(Type type, int depth) =>
            Offset(Program.RegisterProgram.Temporary(CoflowValueShape.Of(type), depth));
        internal CoflowValueRegister Parameter(int index) => Offset(Program.RegisterProgram.Parameters[index]);
        internal CoflowValueRegister Local(int index) => Offset(Program.RegisterProgram.Locals[index]);
        private CoflowValueRegister Offset(CoflowValueRegister register) => register with
        {
            IntegerBase = register.IntegerBase + _integerBase,
            FloatBase = register.FloatBase + _floatBase,
            ReferenceBase = register.ReferenceBase + _referenceBase,
        };
        private CoflowRegister Offset(CoflowRegister register) => register with { Index = register.Index + register.Kind switch
        {
            CoflowRegisterKind.Integer => _integerBase,
            CoflowRegisterKind.Float => _floatBase,
            _ => _referenceBase,
        }};
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

        internal void CopyPhysical(CoflowValueRegister source, CoflowValueRegister target)
        {
            if (source.Shape.IntegerCount != target.Shape.IntegerCount ||
                source.Shape.FloatCount != target.Shape.FloatCount ||
                source.Shape.ReferenceCount != target.Shape.ReferenceCount)
                throw new InvalidOperationException("register physical layout mismatch");
            Array.Copy(_integers, source.IntegerBase, _integers, target.IntegerBase, source.Shape.IntegerCount);
            Array.Copy(_floats, source.FloatBase, _floats, target.FloatBase, source.Shape.FloatCount);
            Array.Copy(_references, source.ReferenceBase, _references, target.ReferenceBase, source.Shape.ReferenceCount);
        }

        internal void WriteEncoded(CoflowEncodedValue source, CoflowValueRegister target)
        {
            if (source.Shape.Type != target.Shape.Type)
                throw new InvalidOperationException("encoded constant type mismatch");
            Array.Copy(source.Integers, 0, _integers, target.IntegerBase, source.Integers.Length);
            Array.Copy(source.Floats, 0, _floats, target.FloatBase, source.Floats.Length);
            Array.Copy(source.References, 0, _references, target.ReferenceBase, source.References.Length);
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

        internal bool Call(CoflowFunctionEntry entry, int pc, int depth, int argumentCount, bool tail)
        {
            var target = entry.CompiledProgram;
            if (target is null) return false;
            if (tail && ReferenceEquals(target, Program))
            {
                for (var index = 0; index < argumentCount; index++)
                    CopyRelative(
                        Program.RegisterProgram.Stack(pc, depth - argumentCount + index),
                        Program.RegisterProgram.Parameters[index]);
                Pc = 0;
                return true;
            }
            var returnTarget = Temporary(target.ReturnType, depth - argumentCount);
            EnterFromStack(target, pc, depth, argumentCount, tail, returnTarget);
            return true;
        }

        private void EnterFromStack(
            CoflowProgram target,
            int callerPc,
            int callerDepth,
            int argumentCount,
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
            for (var index = 0; index < argumentCount; index++)
            {
                var source = Absolute(
                    caller.RegisterProgram.Stack(callerPc, callerDepth - argumentCount + index),
                    callerIntegerBase,
                    callerFloatBase,
                    callerReferenceBase);
                Copy(source, Parameter(index));
            }
            if (tail) CompactTailWindow(target.RegisterProgram,
                callerIntegerBase, callerFloatBase, callerReferenceBase, callerReferenceTop);
        }

        internal bool CallIndirect<TResult>(
            int pc, int depth, int argumentCount, bool tail, out TResult returned)
        {
            returned = default!;
            var callable = ReadReference(Stack(pc, depth - argumentCount - 1).Scalar);
            if (callable is CoflowFunctionEntry entry)
            {
                if (entry.CompiledProgram is { } compiled)
                {
                    EnterFromStack(compiled, pc, depth, argumentCount, tail,
                        Temporary(compiled.ReturnType, depth - argumentCount - 1));
                    return false;
                }
                var resultType = Program.RegisterProgram.Instructions[pc].ValueType!;
                entry.InvokeBoundFromVm(new CoflowNativeFrame(
                    this, pc, depth - argumentCount, depth - argumentCount - 1,
                    argumentCount, resultType));
                return tail && ReturnRegister(Temporary(resultType, depth - argumentCount - 1), out returned);
            }
            if (callable is CoflowClosure closure)
            {
                var returnTarget = Temporary(closure.Program.ReturnType, depth - argumentCount - 1);
                EnterClosureFromStack(closure, pc, depth, argumentCount, tail, returnTarget);
                return false;
            }
            if (callable is Delegate implementation)
            {
                var descriptor = CoflowFunctionDelegates.Callable(implementation);
                if (descriptor.Entry is { } adaptedEntry)
                {
                    if (adaptedEntry.CompiledProgram is { } compiled)
                    {
                        EnterFromStack(compiled, pc, depth, argumentCount, tail,
                            Temporary(compiled.ReturnType, depth - argumentCount - 1));
                        return false;
                    }
                    adaptedEntry.InvokeBoundFromVm(new CoflowNativeFrame(
                        this, pc, depth - argumentCount, depth - argumentCount - 1,
                        argumentCount, Program.RegisterProgram.Instructions[pc].ValueType!));
                }
                else if (descriptor.Closure is { } adaptedClosure)
                {
                    EnterClosureFromStack(adaptedClosure, pc, depth, argumentCount, tail,
                        Temporary(adaptedClosure.Program.ReturnType, depth - argumentCount - 1));
                    return false;
                }
                else descriptor.NativeCall!.Invoke(new CoflowNativeFrame(
                    this, pc, depth - argumentCount, depth - argumentCount - 1,
                    argumentCount, Program.RegisterProgram.Instructions[pc].ValueType!));
                if (tail && ReturnRegister(
                        Temporary(Program.RegisterProgram.Instructions[pc].ValueType!, depth - argumentCount - 1),
                        out returned)) return true;
                return false;
            }
            throw new InvalidOperationException("indirect call target is not callable");
        }

        private void EnterClosureFromStack(
            CoflowClosure closure,
            int callerPc,
            int callerDepth,
            int argumentCount,
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
            for (var index = 0; index < argumentCount; index++)
            {
                var source = Absolute(
                    caller.RegisterProgram.Stack(callerPc, callerDepth - argumentCount + index),
                    callerIntegerBase,
                    callerFloatBase,
                    callerReferenceBase);
                Copy(source, Parameter(index));
            }
            WriteCaptures(closure, argumentCount);
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
                root = CoflowBoundaryCodec<TResult>.Read(this, source);
                return true;
            }
            var frame = _frames[_frameCount - 1];
            Copy(source, frame.ReturnTarget);
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

        internal void MakeClosure(int pc, int depth, CoflowClosureTemplate template, Type delegateType)
        {
            var closure = CoflowClosure.Create(template.Program, template.Captures,
                template.IntegerCount, template.FloatCount, template.ReferenceCount);
            var start = depth - template.CaptureCount;
            for (var index = 0; index < template.CaptureCount; index++)
                Capture(Stack(pc, start + index), template.Captures[index], closure);
            WriteReference(Temporary(delegateType, start).Scalar, closure);
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
