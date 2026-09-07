namespace Coflow.Runtime.CompilerServices;

/// <summary>发布快照使用的不可变列式记录存储。</summary>
internal sealed class CoflowRecordArena
{
    private readonly long[] _integers;
    private readonly double[] _floats;
    private readonly object?[] _references;
    private readonly Row[] _rows;
    private readonly CoflowCollectionArena _collections;

    private CoflowRecordArena(
        long[] integers,
        double[] floats,
        object?[] references,
        Row[] rows,
        CoflowCollectionArena collections)
    {
        _integers = integers;
        _floats = floats;
        _references = references;
        _rows = rows;
        _collections = collections;
    }

    internal static CoflowRecordArena Build(
        IReadOnlyList<CoflowLoadedValue> values,
        IReadOnlyDictionary<CoflowTypeId, ICoflowTypeMetadata> metadata,
        uint snapshotId)
    {
        var collections = new CoflowCollectionArena();
        collections.Reset(snapshotId);
        CoflowEncodedValue Encode(Type type, object? value) =>
            CoflowCollectionEncoding.Encode(type, value, collections);
        var encoded = values.Select(value =>
        {
            if (!CoflowSchemaRuntimeContext.TryGetTypeCodec(value.ApiValue.GetType(), out var codec))
            {
                // Host 由 native adapter 访问，不进入 VM 数据 Arena，但仍保留对齐的 ValueEntry。
                if (metadata[value.TypeId] is ICoflowHostMetadata)
                    return new CoflowEncodedValue(CoflowValueShape.Of(typeof(Unit)),
                        Array.Empty<long>(), Array.Empty<double>(), Array.Empty<object?>());
                throw new InvalidOperationException($"No schema Arena codec exists for `{value.ApiValue.GetType()}`.");
            }
            return codec.EncodeArena(value.ApiValue, Encode);
        }).ToArray();
        var integers = new long[encoded.Sum(value => value.Integers.Length)];
        var floats = new double[encoded.Sum(value => value.Floats.Length)];
        var references = new object?[encoded.Sum(value => value.References.Length)];
        var rows = new Row[encoded.Length];
        var integerBase = 0;
        var floatBase = 0;
        var referenceBase = 0;
        for (var index = 0; index < encoded.Length; index++)
        {
            var value = encoded[index];
            rows[index] = new Row(integerBase, floatBase, referenceBase);
            Array.Copy(value.Integers, 0, integers, integerBase, value.Integers.Length);
            Array.Copy(value.Floats, 0, floats, floatBase, value.Floats.Length);
            Array.Copy(value.References, 0, references, referenceBase, value.References.Length);
            integerBase += value.Integers.Length;
            floatBase += value.Floats.Length;
            referenceBase += value.References.Length;
        }
        return new CoflowRecordArena(integers, floats, references, rows, collections);
    }

    internal int CollectionCount => _collections.Count;
    internal CoflowCollectionArena Collections => _collections;
    internal bool ContainsCollection(CoflowCollectionId id) => _collections.Contains(id);
    internal CoflowCollectionKind CollectionKind(CoflowCollectionId id) => _collections.Kind(id);
    internal int CollectionItemCount(CoflowCollectionId id) => _collections.ItemCount(id);
    internal CoflowEncodedValue ReadArrayItem(CoflowCollectionId id, int index) =>
        _collections.ReadArrayItem(id, index);
    internal void CopyArrayItem(CoflowCollectionId id, int index,
        CoflowExecutionSession context, CoflowValueRegister target) =>
        _collections.CopyArrayItem(id, index, context, target);
    internal CoflowEncodedValue ReadDictionaryKey(CoflowCollectionId id, int index) =>
        _collections.ReadDictionaryKey(id, index);
    internal CoflowEncodedValue ReadDictionaryValue(CoflowCollectionId id, int index) =>
        _collections.ReadDictionaryValue(id, index);
    internal void CopyDictionaryKey(CoflowCollectionId id, int index,
        CoflowExecutionSession context, CoflowValueRegister target) =>
        _collections.CopyDictionaryKey(id, index, context, target);
    internal void CopyDictionaryValue(CoflowCollectionId id, int index,
        CoflowExecutionSession context, CoflowValueRegister target) =>
        _collections.CopyDictionaryValue(id, index, context, target);
    internal int FindDictionaryKey(CoflowCollectionId id,
        CoflowExecutionSession context, CoflowValueRegister key) =>
        _collections.FindDictionaryKey(id, context, key);

    internal long ReadInteger(int row, int offset) => _integers[_rows[row].IntegerBase + offset];
    internal double ReadFloat(int row, int offset) => _floats[_rows[row].FloatBase + offset];
    internal object? ReadReference(int row, int offset) => _references[_rows[row].ReferenceBase + offset];

    internal void CopyField(int row, CoflowFieldAccess access,
        CoflowExecutionSession context, CoflowValueRegister target)
    {
        var entry = _rows[row];
        for (var index = 0; index < target.Shape.IntegerCount; index++)
            context.Registers.WriteInteger(new CoflowRegister(CoflowRegisterKind.Integer, target.IntegerBase + index),
                _integers[entry.IntegerBase + access.IntegerOffset + index]);
        for (var index = 0; index < target.Shape.FloatCount; index++)
            context.Registers.WriteFloat(new CoflowRegister(CoflowRegisterKind.Float, target.FloatBase + index),
                _floats[entry.FloatBase + access.FloatOffset + index]);
        for (var index = 0; index < target.Shape.ReferenceCount; index++)
            context.Registers.WriteReference(new CoflowRegister(CoflowRegisterKind.Reference, target.ReferenceBase + index),
                _references[entry.ReferenceBase + access.ReferenceOffset + index]);
    }

    private readonly record struct Row(int IntegerBase, int FloatBase, int ReferenceBase);
}
