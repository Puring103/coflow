namespace Coflow.Runtime;

using System.Collections;
using global::Coflow.Runtime.CompilerServices;

public sealed class CoflowLoadException : CfdLoadException
{
    public CoflowLoadException(IReadOnlyList<CfdDiagnostic> diagnostics) : base(diagnostics) { }
}

public abstract class CoflowTable : IEnumerable
{
    public abstract Type RecordType { get; }
    public abstract Type KeyType { get; }
    public abstract int Count { get; }
    internal abstract IEnumerable<object> UntypedRecords { get; }
    public abstract IEnumerator GetEnumerator();
}

public sealed class CoflowStringTable<T> : CoflowTable, IReadOnlyList<T> where T : class
{
    private readonly IReadOnlyList<T>[] _segments;
    private readonly int[] _segmentStarts;
    private readonly int _count;
    private readonly Dictionary<string, T> _index;

    internal CoflowStringTable(IReadOnlyList<T> records, Func<T, string> key)
        : this(new[] { records }, key) { }

    internal CoflowStringTable(IReadOnlyList<T>[] segments, Func<T, string> key)
    {
        _segments = segments.Where(segment => segment.Count != 0).ToArray();
        (_segmentStarts, _count) = CoflowSegments.Index(_segments);
        _index = new Dictionary<string, T>(StringComparer.Ordinal);
        foreach (var record in _segments.SelectMany(value => value))
        {
            var recordKey = key(record);
            if (!_index.TryAdd(recordKey, record)) Duplicate(recordKey);
        }
    }

    public override Type RecordType => typeof(T);
    public override Type KeyType => typeof(string);
    public override int Count => _count;
    public T this[int index] => CoflowSegments.Item(_segments, _segmentStarts, _count, index);
    public Option<T> Get(string key) => _index.TryGetValue(key, out var value) ? Option<T>.Some(value) : Option<T>.None;
    internal override IEnumerable<object> UntypedRecords => this.Cast<object>();
    IEnumerator<T> IEnumerable<T>.GetEnumerator() => _segments.SelectMany(value => value).GetEnumerator();
    public override IEnumerator GetEnumerator() => _segments.SelectMany(value => value).GetEnumerator();
    private static void Duplicate(string key) => throw new CoflowLoadException(new[] {
        new CfdDiagnostic("CFD-SYNTAX-DUPLICATE-RECORD", $"record key `{key}` is declared more than once for `{typeof(T)}`", string.Empty) });
}

public sealed class CoflowEnumTable<T, TKey> : CoflowTable, IReadOnlyList<T>
    where T : class where TKey : struct, Enum
{
    private readonly IReadOnlyList<T>[] _segments;
    private readonly int[] _segmentStarts;
    private readonly int _count;
    private readonly Dictionary<TKey, T> _index;

    internal CoflowEnumTable(IReadOnlyList<T> records, Func<T, TKey> key) : this(new[] { records }, key) { }
    internal CoflowEnumTable(IReadOnlyList<T>[] segments, Func<T, TKey> key)
    {
        _segments = segments.Where(segment => segment.Count != 0).ToArray();
        (_segmentStarts, _count) = CoflowSegments.Index(_segments);
        _index = new Dictionary<TKey, T>();
        foreach (var record in _segments.SelectMany(value => value))
        {
            var recordKey = key(record);
            if (!_index.TryAdd(recordKey, record)) Duplicate(recordKey);
        }
    }

    public override Type RecordType => typeof(T);
    public override Type KeyType => typeof(TKey);
    public override int Count => _count;
    public T this[int index] => CoflowSegments.Item(_segments, _segmentStarts, _count, index);
    public Option<T> Get(TKey key) => _index.TryGetValue(key, out var value) ? Option<T>.Some(value) : Option<T>.None;
    internal override IEnumerable<object> UntypedRecords => this.Cast<object>();
    IEnumerator<T> IEnumerable<T>.GetEnumerator() => _segments.SelectMany(value => value).GetEnumerator();
    public override IEnumerator GetEnumerator() => _segments.SelectMany(value => value).GetEnumerator();
    private static void Duplicate(TKey key) => throw new CoflowLoadException(new[] {
        new CfdDiagnostic("CFD-SYNTAX-DUPLICATE-RECORD", $"record key `{key}` is declared more than once for `{typeof(T)}`", string.Empty) });
}

internal static class CoflowSegments
{
    internal static (int[] Starts, int Count) Index<T>(IReadOnlyList<T>[] segments)
    {
        var starts = new int[segments.Length];
        var count = 0;
        for (var index = 0; index < segments.Length; index++)
        {
            starts[index] = count;
            count = checked(count + segments[index].Count);
        }
        return (starts, count);
    }

    internal static T Item<T>(
        IReadOnlyList<T>[] segments,
        int[] starts,
        int count,
        int index)
    {
        if ((uint)index >= (uint)count) throw new ArgumentOutOfRangeException(nameof(index));
        var segment = Array.BinarySearch(starts, index);
        if (segment < 0) segment = ~segment - 1;
        return segments[segment][index - starts[segment]];
    }
}


internal sealed class CoflowSnapshot
{
    private readonly IReadOnlyDictionary<Type, CoflowTable> _tables;
    private readonly IReadOnlyDictionary<Type, object> _singletons;
    private readonly IReadOnlyDictionary<CoflowFunctionIdentity, CoflowFunctionEntry> _functions;
    private readonly ValueEntry[] _values;
    private readonly CoflowRecordArena _arena;
    private readonly CoflowLinkedFunction[] _linkedFunctions;
    private readonly CoflowFunctionEntry[] _functionTargets;
    private readonly CoflowClosureTemplate[] _closureTargets;
    private readonly IReadOnlyDictionary<(CoflowTypeId TypeId, int FieldId), CoflowFunctionEntry> _defaultFunctions;
    private readonly CoflowSchemaIndex _schemaIndex;
    private readonly int[][] _functionSets;
    private readonly CoflowEscapeStore _escapeStore;

    internal CoflowSnapshot(
        ICoflowSchema schema,
        IReadOnlyDictionary<Type, CoflowTable> tables,
        IReadOnlyDictionary<Type, object> singletons,
        IReadOnlyList<CoflowFunctionEntry> functions,
        ValueEntry[] values,
        CoflowRecordArena arena,
        int[][] functionSets,
        CoflowClosureTemplate[] closureTargets,
        CoflowLayoutRegistry layouts,
        CoflowSchemaIndex schemaIndex,
        global::Coflow.Runtime.CoflowOptions options,
        uint generation,
        uint snapshotId)
    {
        Schema = schema;
        Runtime = schema.Runtime;
        _tables = tables;
        _singletons = singletons;
        _functions = functions.ToDictionary(value => value.Identity);
        _functionTargets = functions.OrderBy(value => value.TargetIndex).ToArray();
        _closureTargets = closureTargets;
        _schemaIndex = schemaIndex;
        Layouts = layouts;
        _values = values;
        _arena = arena;
        _linkedFunctions = functions.GroupBy(function => function.ProgramIndex)
            .OrderBy(group => group.Key)
            .Select(group =>
            {
                var definition = group.First();
                return new CoflowLinkedFunction(definition.CompiledProgram, definition);
            }).ToArray();
        _functionSets = functionSets;
        _defaultFunctions = functions.Where(function => function.IsDefault)
            .GroupBy(function => schemaIndex.FunctionSlot(
                function.Identity.DeclaredType, function.Identity.FieldName))
            .ToDictionary(group => group.Key, group => group.First());
        Generation = generation;
        SnapshotId = snapshotId;
        _escapeStore = new CoflowEscapeStore(
            options,
            checked((uint)values.Length),
            checked((uint)_arena.CollectionCount));
    }

    internal ICoflowSchema Schema { get; }
    internal CoflowSchemaRuntime Runtime { get; }
    internal CoflowLayoutRegistry Layouts { get; }
    internal IReadOnlyDictionary<CoflowFunctionIdentity, CoflowFunctionEntry> Functions => _functions;
    internal uint Generation { get; }
    internal uint SnapshotId { get; }
    internal int ValueCount => _values.Length;
    internal int CollectionCount => _arena.CollectionCount;
    internal int EscapedValueCount => _escapeStore.ValueCount;

    internal uint InvocationValueIndexBase => _escapeStore.InvocationValueIndexBase;
    internal uint InvocationCollectionIndexBase => _escapeStore.InvocationCollectionIndexBase;

    internal void CommitInvocationIndexes(uint valueIndex, uint collectionIndex)
    {
        _escapeStore.CommitInvocationIndexes(valueIndex, collectionIndex);
    }

    internal void Promote(
        IEnumerable<CoflowExternalValue> values,
        IEnumerable<(CoflowValueId Id, CoflowClosure Closure)> closures,
        IEnumerable<CoflowCollectionArena> collections)
    {
        _escapeStore.Promote(values, closures, collections);
    }

    internal CoflowLinkedFunction LinkedFunction(int programIndex)
    {
        if ((uint)programIndex >= (uint)_linkedFunctions.Length)
            throw new InvalidOperationException("The VM program references an invalid snapshot function index.");
        return _linkedFunctions[programIndex];
    }

    internal CoflowFunctionTarget Function(CoflowFunctionId functionId, CoflowValueId environmentId,
        CoflowTransientValues transient)
    {
        if (!functionId.IsValid)
            throw new CoflowFunctionNotBoundException();
        if (functionId.SnapshotId != SnapshotId)
            throw new CoflowStaleValueException();
        if (functionId.Kind == CoflowFunctionKind.Closure)
        {
            if ((uint)functionId.TargetIndex >= (uint)_closureTargets.Length)
                throw new CoflowStaleValueException();
            return new CoflowFunctionTarget(Closure(environmentId, functionId.TargetIndex, transient));
        }
        if ((uint)functionId.TargetIndex >= (uint)_functionTargets.Length)
            throw new CoflowStaleValueException();
        var entry = _functionTargets[functionId.TargetIndex];
        var expectedKind = entry.CompiledProgram is null ? CoflowFunctionKind.Native : CoflowFunctionKind.Program;
        if (functionId.Kind != expectedKind) throw new CoflowStaleValueException();
        object? receiver = null;
        if (environmentId.IsValid)
            receiver = ApiValue(environmentId, entry.ReceiverType, transient);
        else if (entry.CompiledProgram is not null)
            throw new CoflowStaleValueException();
        return new CoflowFunctionTarget(entry, receiver);
    }

    internal CoflowClosure Closure(CoflowValueId environmentId, int targetIndex,
        CoflowTransientValues transient)
    {
        if (!environmentId.IsValid || environmentId.SnapshotId != SnapshotId ||
            (!transient.TryGetClosure(environmentId, out var closure) &&
             !_escapeStore.TryGetClosure(environmentId.Index, out closure!)) ||
            !ReferenceEquals(closure.Program, _closureTargets[targetIndex].Program))
            throw new CoflowStaleValueException();
        return closure;
    }

    public TTable Table<TTable>(ICoflowTableToken<TTable> token) where TTable : CoflowTable
    {
        if (token is null) throw new ArgumentNullException(nameof(token));
        return _tables.TryGetValue(token.RecordType, out var table) ? (TTable)table : token.Empty;
    }

    public Option<T> Singleton<T>() where T : class => _singletons.TryGetValue(typeof(T), out var value)
        ? Option<T>.Some((T)value) : Option<T>.None;


    internal CoflowFunctionTarget Function(
        CoflowValueId valueId,
        CoflowTypeId typeId,
        CoflowFieldId fieldId,
        CoflowTransientValues transient)
    {
        if (!valueId.IsValid || valueId.SnapshotId != SnapshotId || valueId.Index == 0)
            throw new CoflowStaleValueException();
        if (valueId.Index > _values.Length)
        {
            if (!transient.TryGet(valueId, out var external) &&
                !_escapeStore.TryGetValue(valueId.Index, out external!))
                throw new CoflowStaleValueException();
            ValidateAssignable(external.TypeId, typeId);
            var externalMetadata = _schemaIndex.ById[typeId];
            if ((uint)fieldId.Value >= (uint)externalMetadata.Fields.Count)
                throw new ArgumentOutOfRangeException(nameof(fieldId));
            return _defaultFunctions.TryGetValue((typeId, fieldId.Value), out var defaultFunction)
                ? new CoflowFunctionTarget(defaultFunction, external.ApiValue)
                : throw new CoflowFunctionNotBoundException();
        }
        var value = _values[valueId.Index - 1];
        ValidateAssignable(value.TypeId, typeId);
        var functionSet = _functionSets[value.FunctionSetIndex];
        if ((uint)fieldId.Value >= (uint)functionSet.Length)
            throw new ArgumentOutOfRangeException(nameof(fieldId));
        var programIndex = functionSet[fieldId.Value];
        return programIndex >= 0
            ? new CoflowFunctionTarget(_linkedFunctions[programIndex].Entry, value.ApiValue)
            : throw new CoflowFunctionNotBoundException();
    }

    internal void ValidateValue(CoflowValueId valueId, CoflowTypeId typeId,
        CoflowTransientValues transient)
    {
        if (!valueId.IsValid || valueId.SnapshotId != SnapshotId || valueId.Index == 0)
            throw new CoflowStaleValueException();
        if (valueId.Index <= _values.Length)
        {
            ValidateAssignable(_values[valueId.Index - 1].TypeId, typeId);
            return;
        }
        if (!transient.TryGet(valueId, out var external) &&
            !_escapeStore.TryGetValue(valueId.Index, out external!))
            throw new CoflowStaleValueException();
        ValidateAssignable(external.TypeId, typeId);
    }

    internal void CollectArenaRowValueIds(
        CoflowExternalValue value,
        CoflowValueIdCollector collector,
        HashSet<CoflowCollectionId> visitedCollections,
        Func<CoflowCollectionId, CoflowCollectionArena> resolve)
    {
        var fieldTypes = _schemaIndex.ById[value.TypeId].Fields
            .Where(field => !field.Binding.IsFunction)
            .Select(field => field.Binding.RuntimeType);
        CoflowClosure.CollectArenaRowValueIds(
            value.ArenaValue ?? throw new InvalidOperationException("An external value has no Arena row."),
            fieldTypes, collector, visitedCollections, resolve);
    }

    internal object ApiValue(CoflowValueId valueId, Type expectedType,
        CoflowTransientValues transient)
    {
        if (!valueId.IsValid || valueId.SnapshotId != SnapshotId || valueId.Index == 0)
            throw new CoflowStaleValueException();
        object value;
        if (valueId.Index <= _values.Length) value = _values[valueId.Index - 1].ApiValue;
        else if (transient.TryGet(valueId, out var external) ||
                 _escapeStore.TryGetValue(valueId.Index, out external!)) value = external.ApiValue;
        else throw new CoflowStaleValueException();
        if (!expectedType.IsInstanceOfType(value)) throw new CoflowStaleValueException();
        return value;
    }

    internal bool IsType(CoflowValueId valueId, Type expectedType,
        CoflowTransientValues transient)
    {
        if (!valueId.IsValid || valueId.SnapshotId != SnapshotId || valueId.Index == 0)
            throw new CoflowStaleValueException();
        CoflowTypeId concrete;
        if (valueId.Index <= _values.Length) concrete = _values[valueId.Index - 1].TypeId;
        else if (transient.TryGet(valueId, out var external) ||
                 _escapeStore.TryGetValue(valueId.Index, out external!)) concrete = external.TypeId;
        else throw new CoflowStaleValueException();
        return _schemaIndex.ByRuntimeType.TryGetValue(expectedType, out var metadata) &&
            _schemaIndex.IsAssignable(concrete, metadata.TypeId);
    }

    internal long ReadArenaInteger(CoflowValueId id, int offset, CoflowTransientValues transient) =>
        id.Index <= _values.Length
            ? _arena.ReadInteger(checked((int)id.Index - 1), offset)
            : ExternalArenaValue(id, transient).Integers[offset];

    internal double ReadArenaFloat(CoflowValueId id, int offset, CoflowTransientValues transient) =>
        id.Index <= _values.Length
            ? _arena.ReadFloat(checked((int)id.Index - 1), offset)
            : ExternalArenaValue(id, transient).Floats[offset];

    internal object? ReadArenaReference(CoflowValueId id, int offset, CoflowTransientValues transient) =>
        id.Index <= _values.Length
            ? _arena.ReadReference(checked((int)id.Index - 1), offset)
            : ExternalArenaValue(id, transient).References[offset];

    private CoflowEncodedValue ExternalArenaValue(CoflowValueId id, CoflowTransientValues transient)
    {
        if (!id.IsValid || id.SnapshotId != SnapshotId || id.Index == 0)
            throw new CoflowStaleValueException();
        if (id.Index <= _values.Length)
            throw new InvalidOperationException("A published Arena row must be read directly.");
        if ((!transient.TryGet(id, out var external) &&
             !_escapeStore.TryGetValue(id.Index, out external!)) || external.ArenaValue is null)
            throw new CoflowStaleValueException();
        return external.ArenaValue;
    }

    internal void CopyArenaField(CoflowValueId id, CoflowFieldAccess access,
        CoflowExecutionSession context, CoflowValueRegister target, CoflowTransientValues transient)
    {
        if (!id.IsValid || id.SnapshotId != SnapshotId || id.Index == 0)
            throw new CoflowStaleValueException();
        if (id.Index <= _values.Length)
        {
            _arena.CopyField(checked((int)id.Index - 1), access, context, target);
            return;
        }
        var value = ExternalArenaValue(id, transient);
        for (var index = 0; index < target.Shape.IntegerCount; index++)
            context.Registers.WriteInteger(new CoflowRegister(CoflowRegisterKind.Integer, target.IntegerBase + index),
                value.Integers[access.IntegerOffset + index]);
        for (var index = 0; index < target.Shape.FloatCount; index++)
            context.Registers.WriteFloat(new CoflowRegister(CoflowRegisterKind.Float, target.FloatBase + index),
                value.Floats[access.FloatOffset + index]);
        for (var index = 0; index < target.Shape.ReferenceCount; index++)
            context.Registers.WriteReference(new CoflowRegister(CoflowRegisterKind.Reference, target.ReferenceBase + index),
                value.References[access.ReferenceOffset + index]);
    }

    internal void ValidateEnum<TEnum>(TEnum value) where TEnum : struct, Enum
    {
        if (!_schemaIndex.EnumsByRuntimeType.TryGetValue(typeof(TEnum), out var metadata))
            throw new CoflowBoundaryException($"Enum `{typeof(TEnum)}` is not part of this Coflow schema.");
        var raw = Convert.ToInt64(value, System.Globalization.CultureInfo.InvariantCulture);
        var declared = metadata.Variants.Values.Select(item => Convert.ToInt64(
            item, System.Globalization.CultureInfo.InvariantCulture));
        if (metadata.IsFlags ? (raw & ~declared.Aggregate(0L, (mask, item) => mask | item)) != 0
                : !declared.Contains(raw))
            throw new CoflowBoundaryException($"Enum value `{value}` is not declared by `{metadata.DeclaredType}`.");
    }

    internal CoflowCollectionKind CollectionKind(CoflowCollectionId id) => _arena.CollectionKind(id);
    internal CoflowCollectionArena CollectionArena(CoflowCollectionId id)
    {
        if (_arena.ContainsCollection(id)) return _arena.Collections;
        if (_escapeStore.FindCollection(id) is { } escaped) return escaped;
        throw new CoflowStaleValueException(
            $"Collection snapshot/index is {id.SnapshotId}/{id.Index}; current snapshot is {SnapshotId}.");
    }
    internal int CollectionItemCount(CoflowCollectionId id) => _arena.CollectionItemCount(id);
    internal CoflowEncodedValue ReadArrayItem(CoflowCollectionId id, int index) =>
        _arena.ReadArrayItem(id, index);
    internal void CopyArrayItem(CoflowCollectionId id, int index,
        CoflowExecutionSession context, CoflowValueRegister target) =>
        _arena.CopyArrayItem(id, index, context, target);
    internal CoflowEncodedValue ReadDictionaryKey(CoflowCollectionId id, int index) =>
        _arena.ReadDictionaryKey(id, index);
    internal CoflowEncodedValue ReadDictionaryValue(CoflowCollectionId id, int index) =>
        _arena.ReadDictionaryValue(id, index);
    internal void CopyDictionaryKey(CoflowCollectionId id, int index,
        CoflowExecutionSession context, CoflowValueRegister target) =>
        _arena.CopyDictionaryKey(id, index, context, target);
    internal void CopyDictionaryValue(CoflowCollectionId id, int index,
        CoflowExecutionSession context, CoflowValueRegister target) =>
        _arena.CopyDictionaryValue(id, index, context, target);
    internal int FindDictionaryKey(CoflowCollectionId id,
        CoflowExecutionSession context, CoflowValueRegister key) =>
        _arena.FindDictionaryKey(id, context, key);

    private void ValidateAssignable(CoflowTypeId concrete, CoflowTypeId target)
    {
        if (!_schemaIndex.IsAssignable(concrete, target))
            throw new CoflowStaleValueException();
    }

    internal readonly record struct ValueEntry(
        CoflowTypeId TypeId,
        string RecordKey,
        object ApiValue,
        int FunctionSetIndex);

}
