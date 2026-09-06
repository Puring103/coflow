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
    internal abstract Delegate KeyReader { get; }
    public abstract IEnumerator GetEnumerator();
}

public sealed class CoflowStringTable<T> : CoflowTable, IReadOnlyList<T> where T : class
{
    private readonly IReadOnlyList<T>[] _segments;
    private readonly int[] _segmentStarts;
    private readonly int _count;
    private readonly Dictionary<string, T> _index;
    private readonly Func<T, string> _key;

    internal CoflowStringTable(IReadOnlyList<T> records, Func<T, string> key)
        : this(new[] { records }, key) { }

    internal CoflowStringTable(IReadOnlyList<T>[] segments, Func<T, string> key)
    {
        _segments = segments.Where(segment => segment.Count != 0).ToArray();
        (_segmentStarts, _count) = CoflowSegments.Index(_segments);
        _key = key;
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
    internal override Delegate KeyReader => _key;
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
    private readonly Func<T, TKey> _key;

    internal CoflowEnumTable(IReadOnlyList<T> records, Func<T, TKey> key) : this(new[] { records }, key) { }
    internal CoflowEnumTable(IReadOnlyList<T>[] segments, Func<T, TKey> key)
    {
        _segments = segments.Where(segment => segment.Count != 0).ToArray();
        (_segmentStarts, _count) = CoflowSegments.Index(_segments);
        _key = key;
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
    internal override Delegate KeyReader => _key;
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
    private readonly IReadOnlyDictionary<CoflowTypeId, ICoflowTypeMetadata> _metadataById;
    private readonly int[][] _functionSets;
    private readonly HashSet<long> _assignableTypes;
    private readonly IReadOnlyDictionary<Type, ICoflowEnumMetadata> _enumMetadata;
    private readonly Dictionary<uint, CoflowInvocationContext.ExternalValue> _escapedValues = new();
    private readonly Dictionary<uint, CoflowClosure> _escapedClosures = new();
    private readonly List<CoflowCollectionArena> _escapedCollections = new();
    private readonly global::Coflow.Runtime.CoflowOptions _options;
    private uint _nextValueIndex;
    private uint _nextCollectionIndex;
    private long _escapedLanes;

    private CoflowSnapshot(
        ICoflowSchema schema,
        IReadOnlyDictionary<Type, CoflowTable> tables,
        IReadOnlyDictionary<Type, object> singletons,
        IReadOnlyList<CoflowFunctionEntry> functions,
        ValueEntry[] values,
        CoflowRecordArena arena,
        int[][] functionSets,
        CoflowClosureTemplate[] closureTargets,
        CoflowLayoutRegistry layouts,
        global::Coflow.Runtime.CoflowOptions options,
        uint generation,
        uint snapshotId)
    {
        Schema = schema;
        _tables = tables;
        _singletons = singletons;
        _functions = functions.ToDictionary(value => value.Identity);
        _functionTargets = functions.OrderBy(value => value.TargetIndex).ToArray();
        _closureTargets = closureTargets;
        Layouts = layouts;
        _options = options;
        _values = values;
        _arena = arena;
        _linkedFunctions = functions.GroupBy(function => function.ProgramIndex)
            .OrderBy(group => group.Key)
            .Select(group =>
            {
                var definition = group.First();
                return new CoflowLinkedFunction(definition.CompiledProgram, definition);
            }).ToArray();
        _metadataById = schema.Types.ToDictionary(metadata => metadata.TypeId);
        _assignableTypes = schema.Types.SelectMany(metadata => metadata.AssignableTypeIds,
            static (metadata, target) => TypePair(metadata.TypeId, target)).ToHashSet();
        _enumMetadata = schema.Enums.ToDictionary(metadata => metadata.RuntimeType);
        _functionSets = functionSets;
        _defaultFunctions = functions.Where(function => function.IsDefault)
            .GroupBy(function => (schema.Types.Single(type =>
                    type.DeclaredType == function.Identity.DeclaredType).TypeId,
                IndexOf(schema.Types.Single(type =>
                    type.DeclaredType == function.Identity.DeclaredType).Fields.Select(field => field.Name).ToArray(),
                    function.Identity.FieldName)))
            .ToDictionary(group => group.Key, group => group.First());
        Generation = generation;
        SnapshotId = snapshotId;
        _nextValueIndex = checked((uint)values.Length);
        _nextCollectionIndex = checked((uint)_arena.CollectionCount);
    }

    internal ICoflowSchema Schema { get; }
    internal CoflowLayoutRegistry Layouts { get; }
    internal IReadOnlyDictionary<CoflowFunctionIdentity, CoflowFunctionEntry> Functions => _functions;
    internal uint Generation { get; }
    internal uint SnapshotId { get; }
    internal int ValueCount => _values.Length;
    internal int CollectionCount => _arena.CollectionCount;
    internal int EscapedValueCount => _escapedValues.Count;

    internal uint InvocationValueIndexBase => _nextValueIndex;
    internal uint InvocationCollectionIndexBase => _nextCollectionIndex;

    internal void CommitInvocationIndexes(uint valueIndex, uint collectionIndex)
    {
        // 只有逃逸调用才推进持久游标；未逃逸调用可安全复用同一段临时 ID。
        if (valueIndex > _nextValueIndex) _nextValueIndex = valueIndex;
        if (collectionIndex > _nextCollectionIndex) _nextCollectionIndex = collectionIndex;
    }

    internal void Promote(
        IEnumerable<CoflowInvocationContext.ExternalValue> values,
        IEnumerable<(CoflowValueId Id, CoflowClosure Closure)> closures,
        IEnumerable<CoflowCollectionArena> collections)
    {
        // 只提升返回对象图可达的调用期值；发布新快照即可整体使这些 ID 失效。
        var valueAdditions = values.Where(value => !_escapedValues.ContainsKey(value.Id.Index)).ToArray();
        var closureAdditions = closures.Where(closure => !_escapedClosures.ContainsKey(closure.Id.Index)).ToArray();
        var collectionAdditions = collections.Where(collection =>
            !_escapedCollections.Any(existing => ReferenceEquals(existing, collection))).ToArray();
        var lanes =
            valueAdditions.Sum(value => (long)(value.ArenaValue?.LaneCount ?? 0)) +
            closureAdditions.Sum(closure => (long)closure.Closure.StorageLaneCount) +
            collectionAdditions.Sum(collection => (long)collection.StorageLaneCount);
        EnsureEscapeCapacity(checked(valueAdditions.Length + closureAdditions.Length), lanes);
        foreach (var value in valueAdditions) _escapedValues.Add(value.Id.Index, value);
        foreach (var closure in closureAdditions) _escapedClosures.Add(closure.Id.Index, closure.Closure);
        _escapedCollections.AddRange(collectionAdditions);
        _escapedLanes = checked(_escapedLanes + lanes);
    }

    private void EnsureEscapeCapacity(int additions, long lanes)
    {
        if (additions > _options.MaxEscapedValues - _escapedValues.Count - _escapedClosures.Count)
            throw new global::Coflow.Runtime.CoflowExecutionLimitException(nameof(_options.MaxEscapedValues));
        if (lanes > _options.MaxEscapedLanes - _escapedLanes)
            throw new global::Coflow.Runtime.CoflowExecutionLimitException(nameof(_options.MaxEscapedLanes));
    }

    internal CoflowLinkedFunction LinkedFunction(int programIndex)
    {
        if ((uint)programIndex >= (uint)_linkedFunctions.Length)
            throw new InvalidOperationException("The VM program references an invalid snapshot function index.");
        return _linkedFunctions[programIndex];
    }

    internal CoflowFunctionTarget Function(CoflowFunctionId functionId, CoflowValueId environmentId)
    {
        if (!functionId.IsValid)
            throw new CoflowFunctionNotBoundException();
        if (functionId.SnapshotId != SnapshotId)
            throw new CoflowStaleValueException();
        if (functionId.Kind == CoflowFunctionKind.Closure)
        {
            if ((uint)functionId.TargetIndex >= (uint)_closureTargets.Length)
                throw new CoflowStaleValueException();
            return new CoflowFunctionTarget(Closure(environmentId, functionId.TargetIndex));
        }
        if ((uint)functionId.TargetIndex >= (uint)_functionTargets.Length)
            throw new CoflowStaleValueException();
        var entry = _functionTargets[functionId.TargetIndex];
        var expectedKind = entry.CompiledProgram is null ? CoflowFunctionKind.Native : CoflowFunctionKind.Program;
        if (functionId.Kind != expectedKind) throw new CoflowStaleValueException();
        object? receiver = null;
        if (environmentId.IsValid)
            receiver = ApiValue(environmentId, entry.ReceiverType);
        else if (entry.CompiledProgram is not null)
            throw new CoflowStaleValueException();
        return new CoflowFunctionTarget(entry, receiver);
    }

    internal CoflowClosure Closure(CoflowValueId environmentId, int targetIndex)
    {
        if (!environmentId.IsValid || environmentId.Generation != Generation ||
            (!CoflowInvocationContext.TryGetClosure(environmentId, out var closure) &&
             !_escapedClosures.TryGetValue(environmentId.Index, out closure!)) ||
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

    internal static CoflowSnapshot Build(
        CfdDocument[] documents,
        ICoflowSchema schema,
        IReadOnlyDictionary<Type, object> hostBindings,
        IReadOnlyDictionary<long, CoflowModule> modules,
        CoflowLayoutRegistry layouts,
        global::Coflow.Runtime.CoflowOptions options,
        uint generation,
        uint snapshotId)
    {
        if (documents is null) throw new ArgumentNullException(nameof(documents));
        if (hostBindings is null) throw new ArgumentNullException(nameof(hostBindings));
        ValidateSchema(schema);
        var context = new CfdLoadContext(
            documents, schema.Types, schema.Constants, generation, snapshotId);
        var metadataByName = schema.Types.ToDictionary(value => value.DeclaredType, StringComparer.Ordinal);
        var allRecords = context.Records.All;
        foreach (var record in allRecords)
        {
            if (!metadataByName.TryGetValue(record.DeclaredType, out var metadata))
                throw Error("CFD-REF-UNKNOWN-TYPE", $"unknown record type `{record.DeclaredType}`", record.Span);
            if (metadata is ICoflowHostMetadata)
                throw Error("CFD-HOST-RECORD", $"CFD cannot declare @Host `{record.DeclaredType}`", record.Span);
            if (metadata is not ICoflowRecordMetadata)
                throw Error("CFD-REF-UNKNOWN-TYPE", $"type `{record.DeclaredType}` cannot be declared as a record", record.Span);
        }

        var records = new CoflowRecordCatalog();
        var singletons = new Dictionary<Type, object>();
        foreach (var record in allRecords)
        {
            var metadata = (ICoflowRecordMetadata)metadataByName[record.DeclaredType];
            var shell = context.AttachRecordValue(metadata, record.Key, metadata.CreateRecord(record.Key, context));
            records.Add(record.DeclaredType, record.Key, shell);
            context.RegisterRecord(record.DeclaredType, record.Key, shell);
        }
        foreach (var record in allRecords)
        {
            ((ICoflowRecordMetadata)metadataByName[record.DeclaredType]).PopulateRecord(
                records.Get(record.DeclaredType, record.Key), record, context);
            context.CompleteRecordValue(record.DeclaredType, record.Key);
        }
        foreach (var metadata in schema.Types)
        {
            if (metadata is ICoflowHostMetadata hostMetadata)
            {
                hostBindings.TryGetValue(metadata.RuntimeType, out var binding);
                var host = hostMetadata.BindHost(binding, context);
                if (host is not null)
                {
                    host = context.AttachRecordValue(metadata, string.Empty, host);
                    context.CompleteRecordValue(metadata.DeclaredType, string.Empty);
                    singletons.Add(metadata.RuntimeType, host);
                    records.Add(metadata.DeclaredType, string.Empty, host);
                }
                continue;
            }
            var nodes = context.Records.OfType(metadata.DeclaredType);
            if (metadata.IsSingleton && nodes.Count > 1)
                throw Error("CFD-SINGLETON-COUNT", $"singleton `{metadata.DeclaredType}` appears more than once");
            if (metadata.IsSingleton && nodes.Count == 1)
                singletons.Add(metadata.RuntimeType, records.Get(metadata.DeclaredType, nodes[0].Key));
        }

        var tables = new Dictionary<Type, CoflowTable>();
        foreach (var metadata in schema.Types.OfType<ICoflowRecordMetadata>()
                     .Where(value => !value.IsSingleton && !value.IsAbstract))
        {
            var values = context.Records.AssignableTo(metadata.DeclaredType)
                .Select(value => records.Get(value.DeclaredType, value.Key)).ToArray();
            if (values.Length != 0) tables.Add(metadata.RuntimeType, CreateTable(metadata, values));
        }
        var closureTargets = CoflowCompiler.Compile(context.Functions, schema, records, context, modules);
        var functionSets = new List<int[]>();
        var valueEntries = context.Values.Select(value =>
        {
            var metadata = schema.Types.First(item => item.TypeId == value.TypeId);
            var slots = Enumerable.Repeat(-1, metadata.Fields.Count).ToArray();
            foreach (var function in value.Functions)
            {
                var slot = IndexOf(metadata.Fields.Select(field => field.Name).ToArray(), function.Identity.FieldName);
                if (slot < 0)
                    throw new InvalidOperationException($"Function `{function.Identity}` has no schema field slot.");
                slots[slot] = function.ProgramIndex;
            }
            var functionSetIndex = functionSets.Count;
            functionSets.Add(slots);
            return new ValueEntry(value.TypeId, value.RecordKey, value.ApiValue, functionSetIndex);
        }).ToArray();
        var arena = CoflowRecordArena.Build(context.Values,
            schema.Types.ToDictionary(metadata => metadata.TypeId), generation);
        return new CoflowSnapshot(schema, tables, singletons, context.Functions,
            valueEntries, arena, functionSets.ToArray(), closureTargets, layouts, options, generation, snapshotId);
    }

    internal CoflowBoundFunction Function(
        CoflowValueId valueId,
        CoflowTypeId typeId,
        CoflowFieldId fieldId)
    {
        if (!valueId.IsValid || valueId.Generation != Generation || valueId.Index == 0)
            throw new CoflowStaleValueException();
        if (valueId.Index > _values.Length)
        {
            if (!CoflowInvocationContext.TryGetExternal(valueId, out var external) &&
                !_escapedValues.TryGetValue(valueId.Index, out external!))
                throw new CoflowStaleValueException();
            ValidateAssignable(external.TypeId, typeId);
            var externalMetadata = _metadataById[typeId];
            if ((uint)fieldId.Value >= (uint)externalMetadata.Fields.Count)
                throw new ArgumentOutOfRangeException(nameof(fieldId));
            return _defaultFunctions.TryGetValue((typeId, fieldId.Value), out var defaultFunction)
                ? new CoflowBoundFunction(defaultFunction, external.ApiValue)
                : throw new CoflowFunctionNotBoundException();
        }
        var value = _values[valueId.Index - 1];
        ValidateAssignable(value.TypeId, typeId);
        var metadata = _metadataById[typeId];
        if ((uint)fieldId.Value >= (uint)metadata.Fields.Count)
            throw new ArgumentOutOfRangeException(nameof(fieldId));
        var programIndex = _functionSets[value.FunctionSetIndex][fieldId.Value];
        return programIndex >= 0
            ? new CoflowBoundFunction(_linkedFunctions[programIndex].Entry, value.ApiValue)
            : throw new CoflowFunctionNotBoundException();
    }

    internal void ValidateValue(CoflowValueId valueId, CoflowTypeId typeId)
    {
        if (!valueId.IsValid || valueId.Generation != Generation || valueId.Index == 0)
            throw new CoflowStaleValueException();
        if (valueId.Index <= _values.Length)
        {
            ValidateAssignable(_values[valueId.Index - 1].TypeId, typeId);
            return;
        }
        if (!CoflowInvocationContext.TryGetExternal(valueId, out var external) &&
            !_escapedValues.TryGetValue(valueId.Index, out external!))
            throw new CoflowStaleValueException();
        ValidateAssignable(external.TypeId, typeId);
    }

    internal void CollectArenaRowValueIds(
        CoflowInvocationContext.ExternalValue value,
        CoflowValueIdCollector collector,
        HashSet<CoflowCollectionId> visitedCollections,
        Func<CoflowCollectionId, CoflowCollectionArena> resolve)
    {
        var fieldTypes = _metadataById[value.TypeId].Fields
            .Where(field => !field.Binding.IsFunction)
            .Select(field => field.Binding.RuntimeType);
        CoflowClosure.CollectArenaRowValueIds(
            value.ArenaValue ?? throw new InvalidOperationException("An external value has no Arena row."),
            fieldTypes, collector, visitedCollections, resolve);
    }

    internal object ApiValue(CoflowValueId valueId, Type expectedType)
    {
        if (!valueId.IsValid || valueId.Generation != Generation || valueId.Index == 0)
            throw new CoflowStaleValueException();
        object value;
        if (valueId.Index <= _values.Length) value = _values[valueId.Index - 1].ApiValue;
        else if (CoflowInvocationContext.TryGetExternal(valueId, out var external) ||
                 _escapedValues.TryGetValue(valueId.Index, out external!)) value = external.ApiValue;
        else throw new CoflowStaleValueException();
        if (!expectedType.IsInstanceOfType(value)) throw new CoflowStaleValueException();
        return value;
    }

    internal bool IsType(CoflowValueId valueId, Type expectedType)
    {
        if (!valueId.IsValid || valueId.Generation != Generation || valueId.Index == 0)
            throw new CoflowStaleValueException();
        CoflowTypeId concrete;
        if (valueId.Index <= _values.Length) concrete = _values[valueId.Index - 1].TypeId;
        else if (CoflowInvocationContext.TryGetExternal(valueId, out var external) ||
                 _escapedValues.TryGetValue(valueId.Index, out external!)) concrete = external.TypeId;
        else throw new CoflowStaleValueException();
        return _metadataById.Values.Any(metadata => metadata.RuntimeType == expectedType &&
            _assignableTypes.Contains(TypePair(concrete, metadata.TypeId)));
    }

    internal long ReadArenaInteger(CoflowValueId id, int offset) =>
        id.Index <= _values.Length
            ? _arena.ReadInteger(checked((int)id.Index - 1), offset)
            : ExternalArenaValue(id).Integers[offset];

    internal double ReadArenaFloat(CoflowValueId id, int offset) =>
        id.Index <= _values.Length
            ? _arena.ReadFloat(checked((int)id.Index - 1), offset)
            : ExternalArenaValue(id).Floats[offset];

    internal object? ReadArenaReference(CoflowValueId id, int offset) =>
        id.Index <= _values.Length
            ? _arena.ReadReference(checked((int)id.Index - 1), offset)
            : ExternalArenaValue(id).References[offset];

    private CoflowEncodedValue ExternalArenaValue(CoflowValueId id)
    {
        if (!id.IsValid || id.Generation != Generation || id.Index == 0)
            throw new CoflowStaleValueException();
        if (id.Index <= _values.Length)
            throw new InvalidOperationException("A published Arena row must be read directly.");
        if ((!CoflowInvocationContext.TryGetExternal(id, out var external) &&
             !_escapedValues.TryGetValue(id.Index, out external!)) || external.ArenaValue is null)
            throw new CoflowStaleValueException();
        return external.ArenaValue;
    }

    internal void CopyArenaField(CoflowValueId id, CoflowFieldAccess access,
        CoflowVm.CoflowExecutionContext context, CoflowValueRegister target)
    {
        if (!id.IsValid || id.Generation != Generation || id.Index == 0)
            throw new CoflowStaleValueException();
        if (id.Index <= _values.Length)
        {
            _arena.CopyField(checked((int)id.Index - 1), access, context, target);
            return;
        }
        var value = ExternalArenaValue(id);
        for (var index = 0; index < target.Shape.IntegerCount; index++)
            context.WriteInteger(new CoflowRegister(CoflowRegisterKind.Integer, target.IntegerBase + index),
                value.Integers[access.IntegerOffset + index]);
        for (var index = 0; index < target.Shape.FloatCount; index++)
            context.WriteFloat(new CoflowRegister(CoflowRegisterKind.Float, target.FloatBase + index),
                value.Floats[access.FloatOffset + index]);
        for (var index = 0; index < target.Shape.ReferenceCount; index++)
            context.WriteReference(new CoflowRegister(CoflowRegisterKind.Reference, target.ReferenceBase + index),
                value.References[access.ReferenceOffset + index]);
    }

    internal void ValidateEnum<TEnum>(TEnum value) where TEnum : struct, Enum
    {
        if (!_enumMetadata.TryGetValue(typeof(TEnum), out var metadata))
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
        foreach (var arena in _escapedCollections)
            if (arena.Contains(id)) return arena;
        throw new CoflowStaleValueException();
    }
    internal int CollectionItemCount(CoflowCollectionId id) => _arena.CollectionItemCount(id);
    internal CoflowEncodedValue ReadArrayItem(CoflowCollectionId id, int index) =>
        _arena.ReadArrayItem(id, index);
    internal void CopyArrayItem(CoflowCollectionId id, int index,
        CoflowVm.CoflowExecutionContext context, CoflowValueRegister target) =>
        _arena.CopyArrayItem(id, index, context, target);
    internal CoflowEncodedValue ReadDictionaryKey(CoflowCollectionId id, int index) =>
        _arena.ReadDictionaryKey(id, index);
    internal CoflowEncodedValue ReadDictionaryValue(CoflowCollectionId id, int index) =>
        _arena.ReadDictionaryValue(id, index);
    internal void CopyDictionaryKey(CoflowCollectionId id, int index,
        CoflowVm.CoflowExecutionContext context, CoflowValueRegister target) =>
        _arena.CopyDictionaryKey(id, index, context, target);
    internal void CopyDictionaryValue(CoflowCollectionId id, int index,
        CoflowVm.CoflowExecutionContext context, CoflowValueRegister target) =>
        _arena.CopyDictionaryValue(id, index, context, target);
    internal int FindDictionaryKey(CoflowCollectionId id,
        CoflowVm.CoflowExecutionContext context, CoflowValueRegister key) =>
        _arena.FindDictionaryKey(id, context, key);

    private void ValidateAssignable(CoflowTypeId concrete, CoflowTypeId target)
    {
        if (!_assignableTypes.Contains(TypePair(concrete, target)))
            throw new CoflowStaleValueException();
    }

    internal readonly record struct ValueEntry(
        CoflowTypeId TypeId,
        string RecordKey,
        object ApiValue,
        int FunctionSetIndex);

    private static int IndexOf(IReadOnlyList<string> values, string value)
    {
        for (var index = 0; index < values.Count; index++)
            if (string.Equals(values[index], value, StringComparison.Ordinal)) return index;
        return -1;
    }

    private static long TypePair(CoflowTypeId concrete, CoflowTypeId target) =>
        ((long)concrete.Value << 32) | (uint)target.Value;

    private static CoflowTable CreateTable(ICoflowRecordMetadata metadata, object[] values)
    {
        var method = typeof(CoflowSnapshot).GetMethod(nameof(CreateTypedTable), System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic)!
            .MakeGenericMethod(metadata.RuntimeType, metadata.KeyType);
        return InvokeTableFactory(method, new object[] { metadata, values });
    }

    private static CoflowTable CreateTypedTable<T, TKey>(ICoflowRecordMetadata metadata, object[] values) where T : class
    {
        var records = values.Cast<T>().ToArray();
        return typeof(TKey) == typeof(string)
            ? new CoflowStringTable<T>(records, (Func<T, string>)metadata.GetKeyReader())
            : CreateEnumTable<T, TKey>(metadata, records);
    }

    private static CoflowTable CreateEnumTable<T, TKey>(ICoflowRecordMetadata metadata, IReadOnlyList<T> records) where T : class
    {
        var method = typeof(CoflowSnapshot).GetMethod(nameof(CreateEnumTableCore), System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic)!
            .MakeGenericMethod(typeof(T), typeof(TKey));
        return InvokeTableFactory(method, new object[] { metadata, records });
    }

    private static CoflowTable CreateEnumTableCore<T, TKey>(ICoflowRecordMetadata metadata, IReadOnlyList<T> records)
        where T : class where TKey : struct, Enum =>
        new CoflowEnumTable<T, TKey>(records, (Func<T, TKey>)metadata.GetKeyReader());

    internal static CoflowTable InvokeTableFactory(System.Reflection.MethodInfo method, object[] arguments)
    {
        try { return (CoflowTable)method.Invoke(null, arguments)!; }
        catch (System.Reflection.TargetInvocationException error) when (error.InnerException is not null)
        {
            System.Runtime.ExceptionServices.ExceptionDispatchInfo.Capture(error.InnerException).Throw();
            throw;
        }
    }

    private static void ValidateSchema(ICoflowSchema schema)
    {
        if (schema is null) throw new ArgumentNullException(nameof(schema));
        var invalidTypeId = schema.Types.FirstOrDefault(value => !value.TypeId.IsValid);
        if (invalidTypeId is not null)
            throw Error("COFLOW-METADATA-TYPE-ID", $"schema type `{invalidTypeId.DeclaredType}` has an invalid TypeId");
        var duplicateTypeId = schema.Types.GroupBy(value => value.TypeId)
            .FirstOrDefault(value => value.Count() > 1);
        if (duplicateTypeId is not null)
            throw Error("COFLOW-METADATA-TYPE-ID", $"schema TypeId `{duplicateTypeId.Key.Value}` is duplicated");
        var duplicateName = schema.Types.Select(value => value.DeclaredType)
            .Concat(schema.Enums.Select(value => value.DeclaredType)).GroupBy(value => value, StringComparer.Ordinal)
            .FirstOrDefault(value => value.Count() > 1);
        if (duplicateName is not null) throw Error("COFLOW-METADATA-DUPLICATE-NAME", $"schema name `{duplicateName.Key}` is duplicated");
        var duplicateType = schema.Types.Select(value => value.RuntimeType)
            .Concat(schema.Enums.Select(value => value.RuntimeType)).GroupBy(value => value).FirstOrDefault(value => value.Count() > 1);
        if (duplicateType is not null) throw Error("COFLOW-METADATA-DUPLICATE-TYPE", $"runtime type `{duplicateType.Key}` is duplicated");
    }

    private static CoflowLoadException Error(string code, string message, CfdSpan? span = null) =>
        new(new[] { new CfdDiagnostic(code, message, string.Empty, span) });
}
