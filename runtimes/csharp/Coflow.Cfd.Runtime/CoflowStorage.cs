namespace CoflowRuntime;

using System.Reflection;
using System.Runtime.CompilerServices;

internal readonly record struct CoflowTypeId(int Value);
internal readonly record struct CoflowFieldId(int Value);
internal readonly record struct CoflowRecordId(int Value);
internal readonly record struct CoflowLayoutId(int Value);

internal enum CoflowSlotKind : byte
{
    Empty,
    Int64,
    Float64,
    Bool,
    Enum,
    Heap,
    Record,
    Function,
}

internal readonly record struct CoflowSlot(CoflowSlotKind Kind, long Value);

internal sealed class CoflowDataHeap
{
    private readonly List<object> _values = new();

    public int Count => _values.Count;

    public int Add(object value)
    {
        _values.Add(value);
        return _values.Count - 1;
    }

    public object Read(int handle) => _values[handle];
}

internal sealed class CoflowFieldDescriptor
{
    public CoflowFieldDescriptor(
        CoflowFieldId id,
        string name,
        Type runtimeType,
        CoflowLayoutId layoutId,
        int offset,
        int width,
        bool isReference)
    {
        Id = id;
        Name = name;
        RuntimeType = runtimeType;
        LayoutId = layoutId;
        Offset = offset;
        Width = width;
        IsReference = isReference;
    }

    public CoflowFieldId Id { get; }
    public string Name { get; }
    public Type RuntimeType { get; }
    public CoflowLayoutId LayoutId { get; }
    public int Offset { get; }
    public int Width { get; }
    public bool IsReference { get; }
}

internal sealed class CoflowRecordLayout
{
    private readonly IReadOnlyDictionary<string, CoflowFieldDescriptor> _fieldsByName;

    public CoflowRecordLayout(
        CoflowLayoutId id,
        ICoflowTypeMetadata metadata,
        IReadOnlyList<CoflowFieldDescriptor> fields,
        int width)
    {
        Id = id;
        Metadata = metadata;
        Fields = fields;
        Width = width;
        _fieldsByName = fields.ToDictionary(field => field.Name, StringComparer.Ordinal);
    }

    public CoflowLayoutId Id { get; }
    public ICoflowTypeMetadata Metadata { get; }
    public IReadOnlyList<CoflowFieldDescriptor> Fields { get; }
    public int Width { get; }

    public CoflowFieldDescriptor Field(string name) => _fieldsByName.TryGetValue(name, out var field)
        ? field
        : throw new InvalidOperationException($"generation layout `{Metadata.DeclaredType}` has no field `{name}`");
}

internal sealed class CoflowRecordTableStorage
{
    public CoflowRecordTableStorage(CoflowTypeId typeId, CoflowRecordLayout layout, int count)
    {
        TypeId = typeId;
        Layout = layout;
        Rows = new CoflowSlot[checked(layout.Width * count)];
        Records = new object[count];
    }

    public CoflowTypeId TypeId { get; }
    public CoflowRecordLayout Layout { get; }
    public CoflowSlot[] Rows { get; }
    public object[] Records { get; }
}

internal sealed class CoflowFieldAccess
{
    private readonly ICoflowTypeMetadata _fallback;

    public CoflowFieldAccess(
        ICoflowTypeMetadata fallback,
        string name,
        Type runtimeType,
        int offset,
        bool isReference)
    {
        _fallback = fallback;
        Name = name;
        RuntimeType = runtimeType;
        Offset = offset;
        IsReference = isReference;
    }

    public string Name { get; }
    public Type RuntimeType { get; }
    public int Offset { get; }
    public bool IsReference { get; }

    public object? Read(object record)
    {
        if (CoflowGenerationStorage.TryLocation(record, out var storage, out var id))
            return storage.ReadField(id, this);
        return _fallback.GetField(record, Name);
    }
}

internal sealed class CoflowGenerationStorage
{
    private sealed class RecordLocation
    {
        public RecordLocation(CoflowGenerationStorage storage, CoflowRecordId id)
        {
            Storage = storage;
            Id = id;
        }

        public CoflowGenerationStorage Storage { get; }
        public CoflowRecordId Id { get; }
    }

    private sealed class ReferenceComparer : IEqualityComparer<object>
    {
        public static readonly ReferenceComparer Instance = new();
        public new bool Equals(object? left, object? right) => ReferenceEquals(left, right);
        public int GetHashCode(object value) => RuntimeHelpers.GetHashCode(value);
    }

    private static readonly ConditionalWeakTable<object, RecordLocation> Locations = new();
    private readonly IReadOnlyList<CoflowFunctionSlot> _functions;
    private readonly Dictionary<CoflowFunctionSlot, int> _functionIds;
    private readonly Dictionary<string, CoflowRecordLayout> _layouts = new(StringComparer.Ordinal);
    private readonly List<(CoflowRecordTableStorage Table, int Row)> _records = new();
    private readonly Dictionary<object, CoflowRecordId> _recordIds = new(ReferenceComparer.Instance);

    private CoflowGenerationStorage(IReadOnlyList<CoflowFunctionSlot> functions)
    {
        _functions = functions;
        _functionIds = functions.Select((slot, index) => (slot, index))
            .ToDictionary(item => item.slot, item => item.index);
    }

    public CoflowDataHeap Heap { get; } = new();
    public IReadOnlyList<CoflowRecordTableStorage> Tables { get; private set; } =
        Array.Empty<CoflowRecordTableStorage>();

    public static CoflowGenerationStorage Build(
        IReadOnlyList<ICoflowTypeMetadata> metadata,
        IReadOnlyDictionary<(string DeclaredType, string Key), object> records,
        IReadOnlyList<CoflowFunctionSlot> functions)
    {
        var storage = new CoflowGenerationStorage(functions);
        var tables = new List<CoflowRecordTableStorage>();
        var metadataByName = metadata.ToDictionary(item => item.DeclaredType, StringComparer.Ordinal);
        var grouped = records
            .Where(item => !metadataByName[item.Key.DeclaredType].IsHost)
            .GroupBy(item => item.Key.DeclaredType, StringComparer.Ordinal)
            .ToDictionary(group => group.Key, group => group.Select(item => item.Value).ToArray(), StringComparer.Ordinal);

        foreach (var type in metadata.Where(item => !item.IsHost))
        {
            var layout = CreateLayout(type, tables.Count);
            storage._layouts.Add(type.DeclaredType, layout);
            var values = grouped.TryGetValue(type.DeclaredType, out var recordsForType)
                ? recordsForType : Array.Empty<object>();
            var table = new CoflowRecordTableStorage(new CoflowTypeId(tables.Count), layout, values.Length);
            tables.Add(table);
            for (var row = 0; row < values.Length; row++)
            {
                var id = new CoflowRecordId(storage._records.Count);
                table.Records[row] = values[row];
                storage._records.Add((table, row));
                storage._recordIds.Add(values[row], id);
                Locations.Add(values[row], new RecordLocation(storage, id));
            }
        }

        storage.Tables = tables;
        storage.ValidateCompatibleBaseLayouts(metadata);
        foreach (var table in tables)
        {
            for (var row = 0; row < table.Records.Length; row++)
                storage.EncodeRow(table, row, table.Records[row]);
        }
        return storage;
    }

    public CoflowFieldAccess BindField(ICoflowTypeMetadata metadata, string fieldName)
    {
        if (!_layouts.TryGetValue(metadata.DeclaredType, out var layout))
            return new CoflowFieldAccess(metadata, fieldName,
                metadata.GetFieldType(fieldName), -1, metadata.ReferenceFieldType(fieldName) is not null);
        var field = layout.Field(fieldName);
        return new CoflowFieldAccess(metadata, fieldName, field.RuntimeType, field.Offset, field.IsReference);
    }

    internal static bool TryLocation(
        object record,
        out CoflowGenerationStorage storage,
        out CoflowRecordId id)
    {
        if (Locations.TryGetValue(record, out var location))
        {
            storage = location.Storage;
            id = location.Id;
            return true;
        }
        storage = null!;
        id = default;
        return false;
    }

    internal object? ReadField(CoflowRecordId id, CoflowFieldAccess access)
    {
        var (table, row) = _records[id.Value];
        return Decode(access.RuntimeType, access.IsReference,
            table.Rows, checked(row * table.Layout.Width + access.Offset));
    }

    private void ValidateCompatibleBaseLayouts(IReadOnlyList<ICoflowTypeMetadata> metadata)
    {
        foreach (var derived in metadata.Where(item => !item.IsHost))
        {
            var derivedLayout = _layouts[derived.DeclaredType];
            foreach (var baseName in derived.AssignableTypes.Where(name => name != derived.DeclaredType))
            {
                if (!_layouts.TryGetValue(baseName, out var baseLayout)) continue;
                foreach (var baseField in baseLayout.Fields)
                {
                    var derivedField = derivedLayout.Field(baseField.Name);
                    if (derivedField.Offset != baseField.Offset || derivedField.Width != baseField.Width ||
                        derivedField.RuntimeType != baseField.RuntimeType ||
                        derivedField.IsReference != baseField.IsReference)
                        throw new InvalidOperationException(
                            $"generated layout `{derived.DeclaredType}` is not compatible with base layout `{baseName}`");
                }
            }
        }
    }

    private static CoflowRecordLayout CreateLayout(ICoflowTypeMetadata metadata, int layoutIndex)
    {
        var fields = new List<CoflowFieldDescriptor>();
        var offset = 0;
        foreach (var name in metadata.FieldNames)
        {
            var type = metadata.GetFieldType(name);
            var isReference = metadata.ReferenceFieldType(name) is not null;
            var width = ValueWidth(type, isReference);
            fields.Add(new CoflowFieldDescriptor(new CoflowFieldId(fields.Count), name, type,
                new CoflowLayoutId(layoutIndex), offset, width, isReference));
            offset = checked(offset + width);
        }
        return new CoflowRecordLayout(new CoflowLayoutId(layoutIndex), metadata, fields, offset);
    }

    private static int ValueWidth(Type type, bool isReference)
    {
        if (isReference || type == typeof(long) || type == typeof(double) || type == typeof(bool) ||
            type.IsEnum || type == typeof(CoflowFunctionSlot)) return 1;
        if (!type.IsGenericType) return 1;
        var definition = type.GetGenericTypeDefinition();
        var arguments = type.GetGenericArguments();
        if (definition == typeof(Option<>)) return checked(1 + ValueWidth(arguments[0], false));
        if (definition == typeof(Result<,>)) return checked(1 + Math.Max(
            ValueWidth(arguments[0], false), ValueWidth(arguments[1], false)));
        return 1;
    }

    private void EncodeRow(CoflowRecordTableStorage table, int row, object record)
    {
        var rowBase = checked(row * table.Layout.Width);
        foreach (var field in table.Layout.Fields)
            Encode(field.RuntimeType, field.IsReference,
                table.Layout.Metadata.GetField(record, field.Name), table.Rows, rowBase + field.Offset);
    }

    private void Encode(Type type, bool isReference, object? value, CoflowSlot[] slots, int offset)
    {
        if (value is null) throw new InvalidOperationException("Coflow generation values cannot be null.");
        if (_recordIds.TryGetValue(value, out var canonicalRecord))
        {
            slots[offset] = new CoflowSlot(CoflowSlotKind.Record, canonicalRecord.Value);
            return;
        }
        if (isReference)
        {
            throw new InvalidOperationException("A generated record reference points outside its generation.");
        }
        if (type == typeof(long)) { slots[offset] = new CoflowSlot(CoflowSlotKind.Int64, (long)value); return; }
        if (type == typeof(double)) { slots[offset] = new CoflowSlot(CoflowSlotKind.Float64, BitConverter.DoubleToInt64Bits((double)value)); return; }
        if (type == typeof(bool)) { slots[offset] = new CoflowSlot(CoflowSlotKind.Bool, (bool)value ? 1 : 0); return; }
        if (type.IsEnum) { slots[offset] = new CoflowSlot(CoflowSlotKind.Enum, Convert.ToInt64(value)); return; }
        if (type == typeof(CoflowFunctionSlot))
        {
            slots[offset] = new CoflowSlot(CoflowSlotKind.Function, _functionIds[(CoflowFunctionSlot)value]);
            return;
        }
        if (type.IsGenericType)
        {
            var definition = type.GetGenericTypeDefinition();
            var arguments = type.GetGenericArguments();
            if (definition == typeof(Option<>))
            {
                var hasValue = (bool)type.GetProperty(nameof(Option<int>.HasValue))!.GetValue(value)!;
                slots[offset] = new CoflowSlot(CoflowSlotKind.Bool, hasValue ? 1 : 0);
                if (hasValue)
                    Encode(arguments[0], false, type.GetProperty(nameof(Option<int>.Value))!.GetValue(value), slots, offset + 1);
                return;
            }
            if (definition == typeof(Result<,>))
            {
                var isOk = (bool)type.GetProperty(nameof(Result<int, int>.IsOk))!.GetValue(value)!;
                slots[offset] = new CoflowSlot(CoflowSlotKind.Bool, isOk ? 1 : 0);
                Encode(arguments[isOk ? 0 : 1], false,
                    type.GetProperty(isOk ? nameof(Result<int, int>.Value) : nameof(Result<int, int>.Error))!.GetValue(value),
                    slots, offset + 1);
                return;
            }
        }
        slots[offset] = new CoflowSlot(CoflowSlotKind.Heap, Heap.Add(value));
    }

    private object? Decode(Type type, bool isReference, CoflowSlot[] slots, int offset)
    {
        var slot = slots[offset];
        if (slot.Kind == CoflowSlotKind.Record)
            return _records[checked((int)slot.Value)].Table.Records[_records[checked((int)slot.Value)].Row];
        if (isReference)
            throw new InvalidOperationException("A record field window does not contain a RecordId.");
        if (type == typeof(long)) return slot.Value;
        if (type == typeof(double)) return BitConverter.Int64BitsToDouble(slot.Value);
        if (type == typeof(bool)) return slot.Value != 0;
        if (type.IsEnum) return Enum.ToObject(type, slot.Value);
        if (type == typeof(CoflowFunctionSlot)) return _functions[checked((int)slot.Value)];
        if (type.IsGenericType)
        {
            var definition = type.GetGenericTypeDefinition();
            var arguments = type.GetGenericArguments();
            if (definition == typeof(Option<>))
            {
                if (slot.Value == 0) return type.GetProperty(nameof(Option<int>.None), BindingFlags.Public | BindingFlags.Static)!.GetValue(null);
                return type.GetMethod(nameof(Option<int>.Some), BindingFlags.Public | BindingFlags.Static)!
                    .Invoke(null, new[] { Decode(arguments[0], false, slots, offset + 1) });
            }
            if (definition == typeof(Result<,>))
            {
                var isOk = slot.Value != 0;
                return type.GetMethod(isOk ? nameof(Result<int, int>.Ok) : nameof(Result<int, int>.Err),
                        BindingFlags.Public | BindingFlags.Static)!
                    .Invoke(null, new[] { Decode(arguments[isOk ? 0 : 1], false, slots, offset + 1) });
            }
        }
        return Heap.Read(checked((int)slot.Value));
    }
}
