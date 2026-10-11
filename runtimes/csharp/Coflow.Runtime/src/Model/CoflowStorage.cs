using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

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
        Func<object, object?>? readReference,
        CoflowFieldValueReader? readValue,
        bool isFunction,
        bool receiverIsStruct,
        int integerOffset,
        int floatOffset,
        int referenceOffset)
    {
        Name = name;
        RuntimeType = runtimeType;
        Reader = reader;
        _read = read;
        Call = call;
        ReadInteger = readInteger;
        ReadFloat = readFloat;
        ReadReference = readReference;
        ReadValue = readValue;
        IsFunction = isFunction;
        ReceiverIsStruct = receiverIsStruct;
        IntegerOffset = integerOffset;
        FloatOffset = floatOffset;
        ReferenceOffset = referenceOffset;
    }

    public string Name { get; }
    public Type RuntimeType { get; }
    public Delegate Reader { get; }
    private readonly Func<object, object> _read;
    internal CoflowNativeCall Call { get; }
    internal Func<object, long>? ReadInteger { get; }
    internal Func<object, double>? ReadFloat { get; }
    internal Func<object, object?>? ReadReference { get; }
    internal CoflowFieldValueReader? ReadValue { get; }
    internal bool IsFunction { get; }
    internal bool ReceiverIsStruct { get; }
    internal int IntegerOffset { get; }
    internal int FloatOffset { get; }
    internal int ReferenceOffset { get; }

    public object Read(object record) => _read(record);

    public static CoflowFieldBinding Function<TRecord, TDelegate>(
        string name,
        CoflowTypeId typeId,
        CoflowFieldId fieldId,
        Func<TRecord, CoflowValueId> id,
        bool receiverIsStruct)
    {
        if (name is null) throw new ArgumentNullException(nameof(name));
        if (id is null) throw new ArgumentNullException(nameof(id));
        Func<object, object> read = value =>
        {
            var handle = CoflowFunctionHandle.Resolve(id((TRecord)value), typeId, fieldId);
            return CoflowFunctionHandle.Create<TDelegate>(handle.FunctionId, handle.EnvironmentId)!;
        };
        var call = new CoflowNativeCall(
            new[] { typeof(TRecord) },
            typeof(TDelegate),
            frame =>
            {
                var receiver = frame.Read<TRecord>(0);
                if (receiver is null) throw new CoflowBoundaryException("Function receiver cannot be null.");
                var handle = CoflowFunctionHandle.Resolve(id(receiver), typeId, fieldId);
                frame.WriteFunction(handle.FunctionId, handle.EnvironmentId);
            });
        return new CoflowFieldBinding(
            name,
            typeof(TDelegate),
            id,
            read,
            call,
            null,
            null,
            read,
            null,
            true,
            receiverIsStruct, 0, 0, 0);
    }

    public static CoflowFieldBinding Create<TRecord, TValue>(
        string name,
        Func<TRecord, TValue> reader,
        bool receiverIsStruct = false,
        int integerOffset = 0,
        int floatOffset = 0,
        int referenceOffset = 0)
    {
        if (name is null) throw new ArgumentNullException(nameof(name));
        if (reader is null) throw new ArgumentNullException(nameof(reader));
        var valueType = typeof(TValue);
        CoflowRegisterKind? scalarKind =
            valueType == typeof(long) || valueType == typeof(bool) || valueType.IsEnum
                ? CoflowRegisterKind.Integer
                : valueType == typeof(double)
                    ? CoflowRegisterKind.Float
                    : valueType == typeof(string)
                        ? CoflowRegisterKind.Reference
                        : null;
        Func<object, long>? readInteger = null;
        Func<object, double>? readFloat = null;
        Func<object, object?>? readReference = null;
        if (scalarKind is not null)
        {
            switch (scalarKind)
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
            readReference,
            scalarKind is not null ? null :
                (context, target, value) => CoflowBoundaryCodec<TValue>.Write(
                    context, target, reader((TRecord)value!)),
            false,
            receiverIsStruct, integerOffset, floatOffset, referenceOffset);
    }

    public static CoflowFieldBinding CreateEnum<TRecord, TEnum>(
        string name,
        Func<TRecord, TEnum> reader,
        Func<TEnum, long> toInt64,
        bool receiverIsStruct = false,
        int integerOffset = 0,
        int floatOffset = 0,
        int referenceOffset = 0)
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
            null,
            null,
            false,
            receiverIsStruct, integerOffset, floatOffset, referenceOffset);
    }
}

/// <summary>Load-time bound access to an ordinary schema object.</summary>
internal sealed class CoflowFieldAccess
{
    private CoflowFieldAccess(
        string name,
        Type runtimeType,
        bool isHost,
        CoflowNativeCall call,
        Func<object, long>? readInteger,
        Func<object, double>? readFloat,
        Func<object, object?>? readReference,
        CoflowFieldValueReader? readValue,
        bool isFunction,
        bool receiverIsStruct,
        int integerOffset,
        int floatOffset,
        int referenceOffset)
    {
        Name = name;
        RuntimeType = runtimeType;
        IsHost = isHost;
        Call = call;
        ReadInteger = readInteger;
        ReadFloat = readFloat;
        ReadReference = readReference;
        ReadValue = readValue;
        IsFunction = isFunction;
        ReceiverIsStruct = receiverIsStruct;
        IntegerOffset = integerOffset;
        FloatOffset = floatOffset;
        ReferenceOffset = referenceOffset;
    }

    public string Name { get; }
    public Type RuntimeType { get; }
    public bool IsHost { get; }
    internal CoflowNativeCall Call { get; }
    internal Func<object, long>? ReadInteger { get; }
    internal Func<object, double>? ReadFloat { get; }
    internal Func<object, object?>? ReadReference { get; }
    internal CoflowFieldValueReader? ReadValue { get; }
    internal bool IsFunction { get; }
    internal bool ReceiverIsStruct { get; }
    internal int IntegerOffset { get; }
    internal int FloatOffset { get; }
    internal int ReferenceOffset { get; }

    internal static CoflowFieldAccess Bind(ICoflowTypeMetadata metadata, CoflowFieldBinding binding)
    {
        if (metadata is null) throw new ArgumentNullException(nameof(metadata));
        if (binding is null) throw new ArgumentNullException(nameof(binding));
        var isHost = metadata is ICoflowHostMetadata;
        return new CoflowFieldAccess(
            binding.Name,
            binding.RuntimeType,
            isHost,
            binding.Call,
            isHost ? binding.ReadInteger : null,
            isHost ? binding.ReadFloat : null,
            isHost ? binding.ReadReference : null,
            isHost ? binding.ReadValue : null,
            binding.IsFunction,
            binding.ReceiverIsStruct,
            binding.IntegerOffset,
            binding.FloatOffset,
            binding.ReferenceOffset);
    }
}

internal delegate void CoflowFieldValueReader(
    CoflowExecutionSession context,
    CoflowValueRegister target,
    object receiver);
}
