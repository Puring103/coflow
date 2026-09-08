using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

using System.ComponentModel;

[EditorBrowsable(EditorBrowsableState.Never)]
public delegate void CoflowStructWriter<T>(ref CoflowValueWriter writer, T value);

[EditorBrowsable(EditorBrowsableState.Never)]
public delegate T CoflowStructReader<T>(ref CoflowValueReader reader);

[EditorBrowsable(EditorBrowsableState.Never)]
public struct CoflowValueWriter
{
    private readonly CoflowExecutionSession? _context;
    private readonly IList<long>? _integers;
    private readonly IList<double>? _floats;
    private readonly IList<object?>? _references;
    private readonly Func<Type, object?, CoflowEncodedValue>? _encodeArenaValue;
    private readonly CoflowValueIdCollector? _collector;
    private readonly List<Type>? _layout;
    private int _integerBase;
    private int _floatBase;
    private int _referenceBase;

    internal CoflowValueWriter(CoflowExecutionSession context, CoflowValueRegister register)
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
        IList<long> integers,
        IList<double> floats,
        IList<object?> references,
        Func<Type, object?, CoflowEncodedValue>? encodeArenaValue = null,
        int integerBase = 0, int floatBase = 0, int referenceBase = 0)
    {
        _context = null;
        _integers = integers;
        _floats = floats;
        _references = references;
        _encodeArenaValue = encodeArenaValue;
        _collector = null;
        _layout = null;
        _integerBase = integerBase;
        _floatBase = floatBase;
        _referenceBase = referenceBase;
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
            CoflowEncodedValue.Encode(shape, value, _integerBase, _floatBase, _referenceBase,
                _integers!, _floats!, _references!, _encodeArenaValue);
        }
        Advance(shape);
    }

    public void WriteValueId(CoflowValueId value)
    {
        if (_layout is not null) return;
        else if (_collector is { } collector) collector.Add(value);
        else if (_context is { } context) context.Registers.WriteInteger(
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
    private readonly CoflowExecutionSession _context;
    private int _integerBase;
    private int _floatBase;
    private int _referenceBase;

    internal CoflowValueReader(CoflowExecutionSession context, CoflowValueRegister register)
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
        CoflowValueId.FromPacked(unchecked((ulong)_context.Registers.ReadInteger(
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
    internal abstract void EncodeInto(object value, IList<long> integers, IList<double> floats,
        IList<object?> references, int integerBase, int floatBase, int referenceBase,
        Func<Type, object?, CoflowEncodedValue>? encodeArenaValue);
    internal abstract void WriteObject(
        CoflowExecutionSession context,
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

    internal void Write(CoflowExecutionSession context, CoflowValueRegister register, T value)
    {
        var writer = new CoflowValueWriter(context, register);
        _writer(ref writer, value);
    }

    internal T Read(CoflowExecutionSession context, CoflowValueRegister register)
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
        EncodeInto(value, integers, floats, references, 0, 0, 0, encodeArenaValue);
        return new CoflowEncodedValue(CoflowValueShape.Of(typeof(T)), integers, floats, references);
    }

    internal override void EncodeInto(object value, IList<long> integers, IList<double> floats,
        IList<object?> references, int integerBase, int floatBase, int referenceBase,
        Func<Type, object?, CoflowEncodedValue>? encodeArenaValue)
    {
        var writer = new CoflowValueWriter(integers, floats, references, encodeArenaValue,
            integerBase, floatBase, referenceBase);
        _writer(ref writer, (T)value);
    }

    internal override void WriteObject(
        CoflowExecutionSession context,
        CoflowValueRegister register,
        object value) => Write(context, register, (T)value);

    internal override void CollectValueIds(object value, CoflowValueIdCollector collector)
    {
        var writer = new CoflowValueWriter(collector);
        _writer(ref writer, (T)value);
    }
}
}
