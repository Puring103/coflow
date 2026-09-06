namespace Coflow.Runtime.CompilerServices;

using System.ComponentModel;

[EditorBrowsable(EditorBrowsableState.Never)]
public delegate void CoflowStructWriter<T>(ref CoflowValueWriter writer, T value);

[EditorBrowsable(EditorBrowsableState.Never)]
public delegate T CoflowStructReader<T>(ref CoflowValueReader reader);

[EditorBrowsable(EditorBrowsableState.Never)]
public static class CoflowStructCodec
{
    public static void Register<T>(
        int integerCount,
        int floatCount,
        int referenceCount,
        CoflowStructWriter<T> writer,
        CoflowStructReader<T> reader) where T : struct
    {
        if (integerCount < 1) throw new ArgumentOutOfRangeException(nameof(integerCount));
        if (floatCount < 0) throw new ArgumentOutOfRangeException(nameof(floatCount));
        if (referenceCount < 0) throw new ArgumentOutOfRangeException(nameof(referenceCount));
        CoflowStructCodecs.Register(new CoflowStructDescriptor<T>(
            integerCount, floatCount, referenceCount, writer, reader));
    }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public struct CoflowValueWriter
{
    private readonly CoflowVm.CoflowExecutionContext? _context;
    private readonly long[]? _integers;
    private readonly double[]? _floats;
    private readonly object?[]? _references;
    private readonly Func<Type, object?, CoflowEncodedValue>? _encodeArenaValue;
    private readonly CoflowValueIdCollector? _collector;
    private readonly List<Type>? _layout;
    private int _integerBase;
    private int _floatBase;
    private int _referenceBase;

    internal CoflowValueWriter(CoflowVm.CoflowExecutionContext context, CoflowValueRegister register)
    {
        _context = context;
        _integers = null;
        _floats = null;
        _references = null;
        _encodeArenaValue = null;
        _collector = null;
        _layout = null;
        _integerBase = register.IntegerBase;
        _floatBase = register.FloatBase;
        _referenceBase = register.ReferenceBase;
    }

    internal CoflowValueWriter(
        long[] integers,
        double[] floats,
        object?[] references,
        Func<Type, object?, CoflowEncodedValue>? encodeArenaValue = null)
    {
        _context = null;
        _integers = integers;
        _floats = floats;
        _references = references;
        _encodeArenaValue = encodeArenaValue;
        _collector = null;
        _layout = null;
        _integerBase = 0;
        _floatBase = 0;
        _referenceBase = 0;
    }

    internal CoflowValueWriter(CoflowValueIdCollector collector)
    {
        _context = null;
        _integers = null;
        _floats = null;
        _references = null;
        _encodeArenaValue = null;
        _collector = collector;
        _layout = null;
        _integerBase = 0;
        _floatBase = 0;
        _referenceBase = 0;
    }

    internal CoflowValueWriter(List<Type> layout)
    {
        _context = null;
        _integers = null;
        _floats = null;
        _references = null;
        _encodeArenaValue = null;
        _collector = null;
        _layout = layout;
        _integerBase = 0;
        _floatBase = 0;
        _referenceBase = 0;
    }

    public void Write<T>(T value)
    {
        if (_layout is { } layout)
        {
            layout.Add(typeof(T));
            return;
        }
        var shape = CoflowValueShape.Of(typeof(T));
        if (_collector is { } collector)
        {
            CoflowEscapeValue<T>.Collect(value, collector);
        }
        else if (_context is { } context)
        {
            CoflowBoundaryCodec<T>.Write(context,
                new CoflowValueRegister(shape, _integerBase, _floatBase, _referenceBase), value);
        }
        else
        {
            var encoded = _encodeArenaValue is null
                ? CoflowEncodedValue.EncodeArenaField(typeof(T), value)
                : _encodeArenaValue(typeof(T), value);
            if (_integerBase + encoded.Integers.Length > _integers!.Length ||
                _floatBase + encoded.Floats.Length > _floats!.Length ||
                _referenceBase + encoded.References.Length > _references!.Length)
                throw new InvalidOperationException(
                    $"Schema Arena layout for `{typeof(T)}` exceeds its declared lane counts " +
                    $"at ({_integerBase}, {_floatBase}, {_referenceBase}).");
            Array.Copy(encoded.Integers, 0, _integers!, _integerBase, encoded.Integers.Length);
            Array.Copy(encoded.Floats, 0, _floats!, _floatBase, encoded.Floats.Length);
            Array.Copy(encoded.References, 0, _references!, _referenceBase, encoded.References.Length);
        }
        Advance(shape);
    }

    public void WriteValueId(CoflowValueId value)
    {
        if (_layout is not null) return;
        else if (_collector is { } collector) collector.Add(value);
        else if (_context is { } context) context.WriteInteger(
            new CoflowRegister(CoflowRegisterKind.Integer, _integerBase), unchecked((long)value.Packed));
        else _integers![_integerBase] = unchecked((long)value.Packed);
        _integerBase++;
    }

    internal int WrittenIntegerCount => _integerBase;
    internal int WrittenFloatCount => _floatBase;
    internal int WrittenReferenceCount => _referenceBase;

    private void Advance(CoflowValueShape shape)
    {
        _integerBase += shape.IntegerCount;
        _floatBase += shape.FloatCount;
        _referenceBase += shape.ReferenceCount;
    }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public struct CoflowValueReader
{
    private readonly CoflowVm.CoflowExecutionContext _context;
    private int _integerBase;
    private int _floatBase;
    private int _referenceBase;

    internal CoflowValueReader(CoflowVm.CoflowExecutionContext context, CoflowValueRegister register)
    {
        _context = context;
        _integerBase = register.IntegerBase;
        _floatBase = register.FloatBase;
        _referenceBase = register.ReferenceBase;
    }

    public T Read<T>()
    {
        var shape = CoflowValueShape.Of(typeof(T));
        var value = CoflowBoundaryCodec<T>.Read(_context,
            new CoflowValueRegister(shape, _integerBase, _floatBase, _referenceBase));
        _integerBase += shape.IntegerCount;
        _floatBase += shape.FloatCount;
        _referenceBase += shape.ReferenceCount;
        return value;
    }

    public CoflowValueId ReadValueId() =>
        CoflowValueId.FromPacked(unchecked((ulong)_context.ReadInteger(
            new CoflowRegister(CoflowRegisterKind.Integer, _integerBase++))));
}

internal abstract class CoflowStructDescriptor
{
    protected CoflowStructDescriptor(Type type, int integerCount, int floatCount, int referenceCount)
    {
        Type = type;
        IntegerCount = integerCount;
        FloatCount = floatCount;
        ReferenceCount = referenceCount;
    }

    internal Type Type { get; }
    internal int IntegerCount { get; }
    internal int FloatCount { get; }
    internal int ReferenceCount { get; }
    internal abstract IReadOnlyList<Type> FieldTypes { get; }
    internal abstract CoflowEncodedValue Encode(
        object value,
        Func<Type, object?, CoflowEncodedValue>? encodeArenaValue = null);
    internal abstract void WriteObject(
        CoflowVm.CoflowExecutionContext context,
        CoflowValueRegister register,
        object value);
    internal abstract void CollectValueIds(object value, CoflowValueIdCollector collector);
}

internal sealed class CoflowStructDescriptor<T> : CoflowStructDescriptor
{
    private readonly CoflowStructWriter<T> _writer;
    internal CoflowStructReader<T> Reader { get; }
    internal override IReadOnlyList<Type> FieldTypes { get; }

    internal CoflowStructDescriptor(int integerCount, int floatCount, int referenceCount,
        CoflowStructWriter<T> writer, CoflowStructReader<T> reader)
        : base(typeof(T), integerCount, floatCount, referenceCount)
    {
        _writer = writer ?? throw new ArgumentNullException(nameof(writer));
        Reader = reader ?? throw new ArgumentNullException(nameof(reader));
        var fieldTypes = new List<Type>();
        var layoutWriter = new CoflowValueWriter(fieldTypes);
        _writer(ref layoutWriter, default!);
        FieldTypes = fieldTypes;
    }

    internal void Write(CoflowVm.CoflowExecutionContext context, CoflowValueRegister register, T value)
    {
        var writer = new CoflowValueWriter(context, register);
        _writer(ref writer, value);
    }

    internal T Read(CoflowVm.CoflowExecutionContext context, CoflowValueRegister register)
    {
        var reader = new CoflowValueReader(context, register);
        return Reader(ref reader);
    }

    internal override CoflowEncodedValue Encode(
        object value,
        Func<Type, object?, CoflowEncodedValue>? encodeArenaValue = null)
    {
        var integers = new long[IntegerCount];
        var floats = new double[FloatCount];
        var references = new object?[ReferenceCount];
        var writer = new CoflowValueWriter(integers, floats, references, encodeArenaValue);
        _writer(ref writer, (T)value);
        if (writer.WrittenIntegerCount != IntegerCount || writer.WrittenFloatCount != FloatCount ||
            writer.WrittenReferenceCount != ReferenceCount)
            throw new InvalidOperationException($"Schema struct codec for `{typeof(T)}` wrote an invalid lane count.");
        return new CoflowEncodedValue(CoflowValueShape.Of(typeof(T)), integers, floats, references);
    }

    internal override void WriteObject(
        CoflowVm.CoflowExecutionContext context,
        CoflowValueRegister register,
        object value) => Write(context, register, (T)value);

    internal override void CollectValueIds(object value, CoflowValueIdCollector collector)
    {
        var writer = new CoflowValueWriter(collector);
        _writer(ref writer, (T)value);
    }
}

internal static class CoflowStructCodecs
{
    private static readonly Dictionary<Type, CoflowStructDescriptor> Descriptors = new();

    internal static void Register(CoflowStructDescriptor descriptor)
    {
        if (!Descriptors.TryAdd(descriptor.Type, descriptor))
            throw new InvalidOperationException($"A struct codec for `{descriptor.Type}` is already registered.");
        CoflowValueShape.RegisterStruct(descriptor.Type, descriptor.IntegerCount,
            descriptor.FloatCount, descriptor.ReferenceCount);
    }

    internal static bool TryGet(Type type, out CoflowStructDescriptor descriptor) =>
        Descriptors.TryGetValue(type, out descriptor!);

    internal static CoflowStructDescriptor<T> Get<T>() =>
        Descriptors.TryGetValue(typeof(T), out var descriptor)
            ? (CoflowStructDescriptor<T>)descriptor
            : throw new InvalidOperationException($"No schema struct codec exists for `{typeof(T)}`.");
}
