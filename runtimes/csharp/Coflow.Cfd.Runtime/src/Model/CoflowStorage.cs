namespace CoflowRuntime;

[System.ComponentModel.EditorBrowsable(System.ComponentModel.EditorBrowsableState.Never)]
public sealed class CoflowFieldBinding
{
    private CoflowFieldBinding(
        string name,
        Type runtimeType,
        Delegate reader,
        Func<object, object> read,
        CoflowNativeCall call,
        Func<object, long>? readInteger,
        Func<object, double>? readFloat,
        Func<object, object?>? readReference)
    {
        Name = name;
        RuntimeType = runtimeType;
        Reader = reader;
        _read = read;
        Call = call;
        ReadInteger = readInteger;
        ReadFloat = readFloat;
        ReadReference = readReference;
    }

    public string Name { get; }
    public Type RuntimeType { get; }
    public Delegate Reader { get; }
    private readonly Func<object, object> _read;
    internal CoflowNativeCall Call { get; }
    internal Func<object, long>? ReadInteger { get; }
    internal Func<object, double>? ReadFloat { get; }
    internal Func<object, object?>? ReadReference { get; }

    public object Read(object record) => _read(record);

    public static CoflowFieldBinding Create<TRecord, TValue>(
        string name,
        Func<TRecord, TValue> reader)
    {
        if (name is null) throw new ArgumentNullException(nameof(name));
        if (reader is null) throw new ArgumentNullException(nameof(reader));
        var shape = CoflowValueShape.Of(typeof(TValue));
        Func<object, long>? readInteger = null;
        Func<object, double>? readFloat = null;
        Func<object, object?>? readReference = null;
        if (shape.Kind == CoflowValueShapeKind.Scalar)
        {
            switch (shape.ScalarKind)
            {
                case CoflowRegisterKind.Integer when typeof(TValue) == typeof(long):
                {
                    var typed = (Func<TRecord, long>)(object)reader;
                    readInteger = value => typed((TRecord)value!);
                    break;
                }
                case CoflowRegisterKind.Integer when typeof(TValue) == typeof(bool):
                {
                    var typed = (Func<TRecord, bool>)(object)reader;
                    readInteger = value => typed((TRecord)value!) ? 1L : 0L;
                    break;
                }
                case CoflowRegisterKind.Integer:
                    readInteger = value => Convert.ToInt64(reader((TRecord)value!));
                    break;
                case CoflowRegisterKind.Float:
                {
                    var typed = (Func<TRecord, double>)(object)reader;
                    readFloat = value => typed((TRecord)value!);
                    break;
                }
                default:
                    readReference = value => reader((TRecord)value!);
                    break;
            }
        }
        return new CoflowFieldBinding(
            name,
            typeof(TValue),
            reader,
            value => reader((TRecord)value!)!,
            CoflowNativeCall.Create(reader),
            readInteger,
            readFloat,
            readReference);
    }

    public static CoflowFieldBinding CreateEnum<TRecord, TEnum>(
        string name,
        Func<TRecord, TEnum> reader,
        Func<TEnum, long> toInt64)
        where TEnum : struct, Enum
    {
        if (name is null) throw new ArgumentNullException(nameof(name));
        if (reader is null) throw new ArgumentNullException(nameof(reader));
        if (toInt64 is null) throw new ArgumentNullException(nameof(toInt64));
        return new CoflowFieldBinding(
            name,
            typeof(TEnum),
            reader,
            value => reader((TRecord)value!)!,
            CoflowNativeCall.Create(reader),
            value => toInt64(reader((TRecord)value!)),
            null,
            null);
    }
}

/// <summary>Load-time bound access to an ordinary generated object.</summary>
internal sealed class CoflowFieldAccess
{
    private CoflowFieldAccess(
        string name,
        Type runtimeType,
        bool isHost,
        CoflowNativeCall call,
        Func<object, long>? readInteger,
        Func<object, double>? readFloat,
        Func<object, object?>? readReference)
    {
        Name = name;
        RuntimeType = runtimeType;
        IsHost = isHost;
        Call = call;
        ReadInteger = readInteger;
        ReadFloat = readFloat;
        ReadReference = readReference;
    }

    public string Name { get; }
    public Type RuntimeType { get; }
    public bool IsHost { get; }
    internal CoflowNativeCall Call { get; }
    internal Func<object, long>? ReadInteger { get; }
    internal Func<object, double>? ReadFloat { get; }
    internal Func<object, object?>? ReadReference { get; }

    internal static CoflowFieldAccess Bind(ICoflowTypeMetadata metadata, string fieldName)
    {
        if (metadata is null) throw new ArgumentNullException(nameof(metadata));
        if (metadata is CoflowGeneratedTypeMetadata generated)
        {
            var binding = generated.GetFieldBinding(fieldName);
            return new CoflowFieldAccess(
                binding.Name,
                binding.RuntimeType,
                metadata is ICoflowHostMetadata,
                binding.Call,
                binding.ReadInteger,
                binding.ReadFloat,
                binding.ReadReference);
        }
        var runtimeType = metadata.GetFieldType(fieldName);
        var reader = metadata.GetFieldReader(fieldName);
        var shape = CoflowValueShape.Of(runtimeType);
        Func<object, long>? readInteger = null;
        Func<object, double>? readFloat = null;
        Func<object, object?>? readReference = null;
        if (shape.Kind == CoflowValueShapeKind.Scalar)
        {
            var value = System.Linq.Expressions.Expression.Parameter(typeof(object), "value");
            var invoke = reader.GetType().GetMethod("Invoke")!;
            var read = System.Linq.Expressions.Expression.Invoke(
                System.Linq.Expressions.Expression.Constant(reader),
                System.Linq.Expressions.Expression.Convert(value, invoke.GetParameters()[0].ParameterType));
            switch (shape.ScalarKind)
            {
                case CoflowRegisterKind.Integer:
                    System.Linq.Expressions.Expression integer = runtimeType == typeof(bool)
                        ? System.Linq.Expressions.Expression.Condition(read,
                            System.Linq.Expressions.Expression.Constant(1L),
                            System.Linq.Expressions.Expression.Constant(0L))
                        : System.Linq.Expressions.Expression.Convert(read, typeof(long));
                    readInteger = CoflowExpressionCompiler.Compile(System.Linq.Expressions.Expression
                        .Lambda<Func<object, long>>(integer, value));
                    break;
                case CoflowRegisterKind.Float:
                    readFloat = CoflowExpressionCompiler.Compile(System.Linq.Expressions.Expression
                        .Lambda<Func<object, double>>(read, value));
                    break;
                default:
                    readReference = CoflowExpressionCompiler.Compile(System.Linq.Expressions.Expression
                        .Lambda<Func<object, object?>>(
                            System.Linq.Expressions.Expression.Convert(read, typeof(object)), value));
                    break;
            }
        }
        return new CoflowFieldAccess(
            fieldName,
            runtimeType,
            metadata is ICoflowHostMetadata,
            new CoflowNativeCall(reader),
            readInteger,
            readFloat,
            readReference);
    }
}
