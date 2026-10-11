using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

using System.Collections;

internal enum CoflowCollectionKind : byte { Array, Dictionary }

internal readonly struct CoflowCollectionId : IEquatable<CoflowCollectionId>
{
    private readonly ulong _value;

    internal CoflowCollectionId(uint snapshotId, uint index)
    {
        _value = ((ulong)snapshotId << 32) | index;
    }

    internal uint SnapshotId => (uint)(_value >> 32);
    internal uint Index => (uint)_value;
    internal ulong Packed => _value;
    internal bool IsValid => SnapshotId != 0 && Index != 0;
    internal static CoflowCollectionId FromPacked(ulong value) =>
        new((uint)(value >> 32), (uint)value);
    public bool Equals(CoflowCollectionId other) => _value == other._value;
    public override bool Equals(object? obj) => obj is CoflowCollectionId other && Equals(other);
    public override int GetHashCode() => _value.GetHashCode();
    public static bool operator ==(CoflowCollectionId left, CoflowCollectionId right) => left.Equals(right);
    public static bool operator !=(CoflowCollectionId left, CoflowCollectionId right) => !left.Equals(right);
}

/// <summary>调用期集合使用的列式存储；句柄为一基索引，零始终表示无效集合。</summary>
internal sealed class CoflowCollectionArena
{
    private readonly List<long> _integers = new();
    private readonly List<double> _floats = new();
    private readonly List<object?> _references = new();
    private readonly List<Entry> _entries = new();
    private readonly Dictionary<uint, int> _entryIndexes = new();
    private uint _snapshotId;
    private uint _firstIndex;
    private uint _lastIndex;
    private Func<uint>? _allocateIndex;
    private CoflowExecutionBudget? _budget;

    internal int Count => _entries.Count;
    internal IEnumerable<uint> Indexes => _entryIndexes.Keys;
    internal uint LastIndex => _lastIndex;
    internal int StorageLaneCount => checked(_integers.Count + _floats.Count + _references.Count);

    internal void Reset(uint snapshotId, uint firstIndex = 0, Func<uint>? allocateIndex = null,
        CoflowExecutionBudget? budget = null)
    {
        if (snapshotId == 0) throw new ArgumentOutOfRangeException(nameof(snapshotId));
        Clear();
        _snapshotId = snapshotId;
        _firstIndex = firstIndex;
        _lastIndex = firstIndex;
        _allocateIndex = allocateIndex;
        _budget = budget;
    }

    internal CoflowCollectionId AddArray(
        CoflowValueShape elementShape,
        IReadOnlyList<CoflowEncodedValue> elements,
        bool budgetAlreadyCharged = false) =>
        Add(CoflowCollectionKind.Array, elementShape, null, elements, null, budgetAlreadyCharged);

    internal CoflowCollectionId AddDictionary(
        CoflowValueShape keyShape,
        CoflowValueShape valueShape,
        IReadOnlyList<CoflowEncodedValue> keys,
        IReadOnlyList<CoflowEncodedValue> values,
        bool budgetAlreadyCharged = false)
    {
        if (keys.Count != values.Count)
            throw new InvalidOperationException("Dictionary key and value counts must match.");
        return Add(CoflowCollectionKind.Dictionary, keyShape, valueShape, keys, values,
            budgetAlreadyCharged);
    }

    internal void ReserveElements(int count) => _budget?.CollectionElements(count);

    internal CoflowCollectionId AddArray(
        CoflowValueShape elementShape,
        CoflowExecutionSession context,
        IReadOnlyList<CoflowValueRegister> elements) =>
        AddFromRegisters(CoflowCollectionKind.Array, elementShape, null, context, elements, null);

    internal CoflowCollectionId AddDictionary(
        CoflowValueShape keyShape,
        CoflowValueShape valueShape,
        CoflowExecutionSession context,
        IReadOnlyList<CoflowValueRegister> keys,
        IReadOnlyList<CoflowValueRegister> values)
    {
        if (keys.Count != values.Count)
            throw new InvalidOperationException("Dictionary key and value counts must match.");
        return AddFromRegisters(CoflowCollectionKind.Dictionary, keyShape, valueShape,
            context, keys, values);
    }

    internal CoflowCollectionId BeginArray(CoflowValueShape elementShape, int capacity)
    {
        if (_snapshotId == 0)
            throw new InvalidOperationException("The collection Arena is not attached to a snapshot.");
        if (capacity < 0) throw new ArgumentOutOfRangeException(nameof(capacity));
        _budget?.CollectionElements(capacity);
        var entry = new Entry(CoflowCollectionKind.Array, elementShape, null, 0, capacity,
            _integers.Count, _floats.Count, _references.Count);
        AddDefaults(_integers, checked(capacity * elementShape.IntegerCount));
        AddDefaults(_floats, checked(capacity * elementShape.FloatCount));
        AddDefaults(_references, checked(capacity * elementShape.ReferenceCount));
        _entries.Add(entry);
        return NewId();
    }

    internal void AppendArray(
        CoflowCollectionId id,
        CoflowExecutionSession context,
        CoflowValueRegister source)
    {
        var entry = EntryAt(id);
        if (entry.Kind != CoflowCollectionKind.Array)
            throw new InvalidOperationException("A dictionary handle cannot be used as an array builder.");
        if (entry.Count >= entry.Capacity)
            throw new InvalidOperationException("The array builder exceeded its reserved capacity.");
        RequireLayout(entry.FirstShape, source.Shape);
        source = context.OffsetRelative(source);
        var (integerBase, floatBase, referenceBase) = Bases(entry, entry.Count, second: false);
        for (var lane = 0; lane < source.Shape.IntegerCount; lane++)
            _integers[integerBase + lane] = context.Registers.ReadInteger(
                new CoflowRegister(CoflowRegisterKind.Integer, source.IntegerBase + lane));
        for (var lane = 0; lane < source.Shape.FloatCount; lane++)
            _floats[floatBase + lane] = context.Registers.ReadFloat(
                new CoflowRegister(CoflowRegisterKind.Float, source.FloatBase + lane));
        for (var lane = 0; lane < source.Shape.ReferenceCount; lane++)
            _references[referenceBase + lane] = context.Registers.ReadReference(
                new CoflowRegister(CoflowRegisterKind.Reference, source.ReferenceBase + lane));
        entry.Count++;
    }

    internal CoflowCollectionKind Kind(CoflowCollectionId id) => EntryAt(id).Kind;
    internal int ItemCount(CoflowCollectionId id) => EntryAt(id).Count;
    internal bool Contains(CoflowCollectionId id) => id.IsValid &&
        id.SnapshotId == _snapshotId && _entryIndexes.ContainsKey(id.Index);

    internal void CopyArrayItem(
        CoflowCollectionId id,
        int index,
        CoflowExecutionSession context,
        CoflowValueRegister target)
    {
        var entry = EntryAt(id);
        if (entry.Kind != CoflowCollectionKind.Array)
            throw new InvalidOperationException("A dictionary handle cannot be read as an array.");
        Copy(entry, entry.FirstShape, index, second: false, context, target);
    }

    internal void CopyDictionaryKey(
        CoflowCollectionId id,
        int index,
        CoflowExecutionSession context,
        CoflowValueRegister target)
    {
        var entry = RequireDictionary(id);
        Copy(entry, entry.FirstShape, index, second: false, context, target);
    }

    internal void CopyDictionaryValue(
        CoflowCollectionId id,
        int index,
        CoflowExecutionSession context,
        CoflowValueRegister target)
    {
        var entry = RequireDictionary(id);
        Copy(entry, entry.SecondShape!, index, second: true, context, target);
    }

    internal int FindDictionaryKey(
        CoflowCollectionId id,
        CoflowExecutionSession context,
        CoflowValueRegister key)
    {
        var entry = RequireDictionary(id);
        RequireLayout(entry.FirstShape, key.Shape);
        key = context.OffsetRelative(key);
        return FindDictionaryKey(id, context.Registers.View(key));
    }

    internal CoflowValueView View(CoflowCollectionId id, int index, bool second = false)
    {
        var position = Bases(EntryAt(id), index, second);
        return new(_integers, _floats, _references, position.Integer, position.Float, position.Reference);
    }

    internal int FindDictionaryKey(CoflowCollectionId id, CoflowValueView key)
    {
        var entry = EntryAt(id);
        return entry.IntegerKeys is { } integers
            ? integers.TryGetValue(key.Integer, out var index) ? index : -1
            : entry.StringKeys!.TryGetValue((string)key.Reference!, out var stringIndex) ? stringIndex : -1;
    }

    internal bool ValueEquals(
        CoflowCollectionId id,
        int index,
        bool second,
        CoflowCollectionArena other,
        CoflowCollectionId otherId,
        int otherIndex,
        bool otherSecond)
    {
        var entry = EntryAt(id);
        var otherEntry = other.EntryAt(otherId);
        var shape = second ? entry.SecondShape! : entry.FirstShape;
        var otherShape = otherSecond ? otherEntry.SecondShape! : otherEntry.FirstShape;
        RequireLayout(shape, otherShape);
        RequireIndex(entry, index);
        RequireIndex(otherEntry, otherIndex);
        var bases = Bases(entry, index, second);
        var otherBases = Bases(otherEntry, otherIndex, otherSecond);
        for (var lane = 0; lane < shape.IntegerCount; lane++)
            if (_integers[bases.Integer + lane] != other._integers[otherBases.Integer + lane]) return false;
        for (var lane = 0; lane < shape.FloatCount; lane++)
            if (!_floats[bases.Float + lane].Equals(other._floats[otherBases.Float + lane])) return false;
        for (var lane = 0; lane < shape.ReferenceCount; lane++)
            if (!Equals(_references[bases.Reference + lane],
                    other._references[otherBases.Reference + lane])) return false;
        return true;
    }

    internal bool ValueEqualsRegister(CoflowCollectionId id, int index, bool second,
        CoflowExecutionSession context, CoflowValueRegister value)
    {
        var entry = EntryAt(id);
        var shape = second ? entry.SecondShape! : entry.FirstShape;
        RequireLayout(shape, value.Shape);
        RequireIndex(entry, index);
        value = context.OffsetRelative(value);
        var bases = Bases(entry, index, second);
        for (var lane = 0; lane < shape.IntegerCount; lane++)
            if (_integers[bases.Integer + lane] != context.Registers.ReadInteger(
                    new CoflowRegister(CoflowRegisterKind.Integer, value.IntegerBase + lane))) return false;
        for (var lane = 0; lane < shape.FloatCount; lane++)
            if (!_floats[bases.Float + lane].Equals(context.Registers.ReadFloat(
                    new CoflowRegister(CoflowRegisterKind.Float, value.FloatBase + lane)))) return false;
        for (var lane = 0; lane < shape.ReferenceCount; lane++)
            if (!Equals(_references[bases.Reference + lane], context.Registers.ReadReference(
                    new CoflowRegister(CoflowRegisterKind.Reference, value.ReferenceBase + lane)))) return false;
        return true;
    }

    internal int Compare(CoflowCollectionId id, int left, int right)
    {
        var entry = EntryAt(id);
        RequireIndex(entry, left);
        RequireIndex(entry, right);
        var leftBases = Bases(entry, left, second: false);
        var rightBases = Bases(entry, right, second: false);
        var type = entry.FirstShape.Type;
        if (type == typeof(double))
            return _floats[leftBases.Float].CompareTo(_floats[rightBases.Float]);
        if (type == typeof(string))
            return string.CompareOrdinal((string?)_references[leftBases.Reference],
                (string?)_references[rightBases.Reference]);
        return _integers[leftBases.Integer].CompareTo(_integers[rightBases.Integer]);
    }

    internal long ReadInteger(CoflowCollectionId id, int index)
    {
        var entry = EntryAt(id);
        RequireIndex(entry, index);
        return _integers[Bases(entry, index, second: false).Integer];
    }

    internal double ReadFloat(CoflowCollectionId id, int index)
    {
        var entry = EntryAt(id);
        RequireIndex(entry, index);
        return _floats[Bases(entry, index, second: false).Float];
    }

    internal CoflowCollectionId AddDictionaryProjection(
        CoflowCollectionArena source,
        CoflowCollectionId sourceId,
        bool values)
    {
        var sourceEntry = source.RequireDictionary(sourceId);
        _budget?.CollectionElements(sourceEntry.Count);
        var shape = values ? sourceEntry.SecondShape! : sourceEntry.FirstShape;
        var entry = new Entry(CoflowCollectionKind.Array, shape, null,
            sourceEntry.Count, sourceEntry.Count, _integers.Count, _floats.Count, _references.Count);
        // 快照和调用期 Arena 之间直接复制列，投影不会物化 CLR 值或临时 encoded 数组。
        for (var index = 0; index < sourceEntry.Count; index++)
        {
            var bases = Bases(sourceEntry, index, values);
            for (var lane = 0; lane < shape.IntegerCount; lane++)
                _integers.Add(source._integers[bases.Integer + lane]);
            for (var lane = 0; lane < shape.FloatCount; lane++)
                _floats.Add(source._floats[bases.Float + lane]);
            for (var lane = 0; lane < shape.ReferenceCount; lane++)
                _references.Add(source._references[bases.Reference + lane]);
        }
        _entries.Add(entry);
        return NewId();
    }

    internal CoflowEncodedValue ReadArrayItem(CoflowCollectionId id, int index)
    {
        var entry = EntryAt(id);
        if (entry.Kind != CoflowCollectionKind.Array)
            throw new InvalidOperationException("A dictionary handle cannot be read as an array.");
        return Read(entry, entry.FirstShape, index, second: false);
    }

    internal CoflowEncodedValue ReadDictionaryKey(CoflowCollectionId id, int index)
    {
        var entry = RequireDictionary(id);
        return Read(entry, entry.FirstShape, index, second: false);
    }

    internal CoflowEncodedValue ReadDictionaryValue(CoflowCollectionId id, int index)
    {
        var entry = RequireDictionary(id);
        return Read(entry, entry.SecondShape!, index, second: true);
    }

    internal void CollectValueIds(
        CoflowCollectionId id,
        CoflowValueIdCollector collector,
        HashSet<CoflowCollectionId> visited,
        Func<CoflowCollectionId, CoflowCollectionArena> resolve)
    {
        if (!visited.Add(id)) return;
        var entry = EntryAt(id);
        for (var index = 0; index < entry.Count; index++)
        {
            CoflowClosure.CollectEncodedValueIds(
                Read(entry, entry.FirstShape, index, second: false), collector, visited, resolve);
            if (entry.SecondShape is not null)
                CoflowClosure.CollectEncodedValueIds(
                    Read(entry, entry.SecondShape, index, second: true), collector, visited, resolve);
        }
    }

    internal void Clear()
    {
        // 调用结束时同时释放 reference 列，不能让池化执行上下文延长应用对象生命周期。
        _integers.Clear();
        _floats.Clear();
        _references.Clear();
        _entries.Clear();
        _entryIndexes.Clear();
        _allocateIndex = null;
        _budget = null;
    }

    internal CoflowCollectionArena Freeze()
    {
        var frozen = new CoflowCollectionArena
        {
            _snapshotId = _snapshotId,
            _firstIndex = _firstIndex,
            _lastIndex = _lastIndex,
        };
        frozen._integers.AddRange(_integers);
        frozen._floats.AddRange(_floats);
        frozen._references.AddRange(_references);
        foreach (var entry in _entries)
            frozen._entries.Add(new Entry(entry.Kind, entry.FirstShape, entry.SecondShape,
                entry.Count, entry.Count, entry.IntegerBase, entry.FloatBase, entry.ReferenceBase)
            { IntegerKeys = entry.IntegerKeys, StringKeys = entry.StringKeys });
        foreach (var pair in _entryIndexes) frozen._entryIndexes.Add(pair.Key, pair.Value);
        return frozen;
    }

    internal CoflowCollectionArena Freeze(HashSet<CoflowCollectionId> reachable)
    {
        if (reachable is null) throw new ArgumentNullException(nameof(reachable));
        var selected = _entryIndexes
            .Where(pair => reachable.Contains(new CoflowCollectionId(_snapshotId, pair.Key)))
            .OrderBy(pair => pair.Value)
            .ToArray();
        var frozen = new CoflowCollectionArena
        {
            _snapshotId = _snapshotId,
            _firstIndex = _firstIndex,
            _lastIndex = selected.Length == 0 ? _firstIndex : selected.Max(pair => pair.Key),
        };
        foreach (var pair in selected)
        {
            var source = _entries[pair.Value];
            var target = new Entry(source.Kind, source.FirstShape, source.SecondShape,
                source.Count, source.Count, frozen._integers.Count,
                frozen._floats.Count, frozen._references.Count)
            { IntegerKeys = source.IntegerKeys, StringKeys = source.StringKeys };
            for (var index = 0; index < source.Count; index++)
            {
                CopyValue(source, source.FirstShape, index, second: false, frozen);
                if (source.SecondShape is not null)
                    CopyValue(source, source.SecondShape, index, second: true, frozen);
            }
            frozen._entryIndexes.Add(pair.Key, frozen._entries.Count);
            frozen._entries.Add(target);
        }
        return frozen;

        void CopyValue(Entry source, CoflowValueShape shape, int index, bool second,
            CoflowCollectionArena target)
        {
            var bases = Bases(source, index, second);
            for (var lane = 0; lane < shape.IntegerCount; lane++)
                target._integers.Add(_integers[bases.Integer + lane]);
            for (var lane = 0; lane < shape.FloatCount; lane++)
                target._floats.Add(_floats[bases.Float + lane]);
            for (var lane = 0; lane < shape.ReferenceCount; lane++)
                target._references.Add(_references[bases.Reference + lane]);
        }
    }

    private CoflowCollectionId Add(
        CoflowCollectionKind kind,
        CoflowValueShape firstShape,
        CoflowValueShape? secondShape,
        IReadOnlyList<CoflowEncodedValue> first,
        IReadOnlyList<CoflowEncodedValue>? second,
        bool budgetAlreadyCharged = false)
    {
        if (_snapshotId == 0)
            throw new InvalidOperationException("The collection Arena is not attached to a snapshot.");
        if (!budgetAlreadyCharged) _budget?.CollectionElements(first.Count);
        var entry = new Entry(kind, firstShape, secondShape, first.Count, first.Count,
            _integers.Count, _floats.Count, _references.Count);
        for (var index = 0; index < first.Count; index++)
        {
            Append(firstShape, first[index]);
            if (second is not null) Append(secondShape!, second[index]);
        }
        IndexDictionary(entry);
        _entries.Add(entry);
        return NewId();
    }

    private CoflowCollectionId AddFromRegisters(
        CoflowCollectionKind kind,
        CoflowValueShape firstShape,
        CoflowValueShape? secondShape,
        CoflowExecutionSession context,
        IReadOnlyList<CoflowValueRegister> first,
        IReadOnlyList<CoflowValueRegister>? second)
    {
        if (_snapshotId == 0)
            throw new InvalidOperationException("The collection Arena is not attached to a snapshot.");
        _budget?.CollectionElements(first.Count);
        var entry = new Entry(kind, firstShape, secondShape, first.Count, first.Count,
            _integers.Count, _floats.Count, _references.Count);
        for (var index = 0; index < first.Count; index++)
        {
            Append(context, firstShape, first[index]);
            if (second is not null) Append(context, secondShape!, second[index]);
        }
        IndexDictionary(entry);
        _entries.Add(entry);
        return NewId();
    }

    // key 类型由编译器确定；索引只构建一次，冻结副本共享只读索引。
    private void IndexDictionary(Entry entry)
    {
        if (entry.Kind != CoflowCollectionKind.Dictionary) return;
        if (entry.FirstShape.Type == typeof(string))
        {
            entry.StringKeys = new Dictionary<string, int>(entry.Count, StringComparer.Ordinal);
            for (var index = 0; index < entry.Count; index++)
                entry.StringKeys.TryAdd((string)_references[Bases(entry, index, false).Reference]!, index);
        }
        else
        {
            entry.IntegerKeys = new Dictionary<long, int>(entry.Count);
            for (var index = 0; index < entry.Count; index++)
                entry.IntegerKeys.TryAdd(_integers[Bases(entry, index, false).Integer], index);
        }
    }

    internal CoflowCollectionId EncodeCollection(object value, int count,
        Type[] arguments, Func<Type, object?, CoflowEncodedValue> encode)
    {
        var dictionary = arguments.Length == 2;
        var first = CoflowValueShape.Of(arguments[0]);
        var second = dictionary ? CoflowValueShape.Of(arguments[1]) : null;
        var entry = new Entry(dictionary ? CoflowCollectionKind.Dictionary : CoflowCollectionKind.Array,
            first, second, count, count, _integers.Count, _floats.Count, _references.Count);
        // 先保留外层连续空间，递归编码的集合随后追加，互不覆盖。
        AddDefaults(_integers, checked(count * (first.IntegerCount + (second?.IntegerCount ?? 0))));
        AddDefaults(_floats, checked(count * (first.FloatCount + (second?.FloatCount ?? 0))));
        AddDefaults(_references, checked(count * (first.ReferenceCount + (second?.ReferenceCount ?? 0))));
        var accessors = dictionary ? CoflowDictionaryEntryAccessors.For(arguments[0], arguments[1]) : null;
        var index = 0;
        foreach (var item in (IEnumerable)value)
        {
            var position = Bases(entry, index, false);
            CoflowEncodedValue.Encode(first, dictionary ? accessors!.Key(item) : item,
                position.Integer, position.Float, position.Reference, _integers, _floats, _references, encode);
            if (dictionary)
            {
                position = Bases(entry, index, true);
                CoflowEncodedValue.Encode(second!, accessors!.Value(item),
                    position.Integer, position.Float, position.Reference, _integers, _floats, _references, encode);
            }
            index++;
        }
        IndexDictionary(entry);
        _entries.Add(entry);
        return NewId();
    }

    private void Append(CoflowValueShape shape, CoflowEncodedValue value)
    {
        RequireLayout(shape, value.Shape);
        _integers.AddRange(value.Integers);
        _floats.AddRange(value.Floats);
        _references.AddRange(value.References);
    }

    private void Append(
        CoflowExecutionSession context,
        CoflowValueShape shape,
        CoflowValueRegister source)
    {
        RequireLayout(shape, source.Shape);
        source = context.OffsetRelative(source);
        for (var lane = 0; lane < shape.IntegerCount; lane++)
            _integers.Add(context.Registers.ReadInteger(
                new CoflowRegister(CoflowRegisterKind.Integer, source.IntegerBase + lane)));
        for (var lane = 0; lane < shape.FloatCount; lane++)
            _floats.Add(context.Registers.ReadFloat(
                new CoflowRegister(CoflowRegisterKind.Float, source.FloatBase + lane)));
        for (var lane = 0; lane < shape.ReferenceCount; lane++)
            _references.Add(context.Registers.ReadReference(
                new CoflowRegister(CoflowRegisterKind.Reference, source.ReferenceBase + lane)));
    }

    private CoflowEncodedValue Read(Entry entry, CoflowValueShape shape, int index, bool second)
    {
        RequireIndex(entry, index);
        var (integerBase, floatBase, referenceBase) = Bases(entry, index, second);
        return new CoflowEncodedValue(shape,
            Slice(_integers, integerBase, shape.IntegerCount),
            Slice(_floats, floatBase, shape.FloatCount),
            Slice(_references, referenceBase, shape.ReferenceCount));
    }

    private void Copy(Entry entry, CoflowValueShape shape, int index, bool second,
        CoflowExecutionSession context, CoflowValueRegister target)
    {
        RequireLayout(shape, target.Shape);
        RequireIndex(entry, index);
        var (integerBase, floatBase, referenceBase) = Bases(entry, index, second);
        for (var lane = 0; lane < shape.IntegerCount; lane++)
            context.Registers.WriteInteger(new CoflowRegister(CoflowRegisterKind.Integer, target.IntegerBase + lane),
                _integers[integerBase + lane]);
        for (var lane = 0; lane < shape.FloatCount; lane++)
            context.Registers.WriteFloat(new CoflowRegister(CoflowRegisterKind.Float, target.FloatBase + lane),
                _floats[floatBase + lane]);
        for (var lane = 0; lane < shape.ReferenceCount; lane++)
            context.Registers.WriteReference(new CoflowRegister(CoflowRegisterKind.Reference, target.ReferenceBase + lane),
                _references[referenceBase + lane]);
    }

    private static (int Integer, int Float, int Reference) Bases(Entry entry, int index, bool second)
    {
        var first = entry.FirstShape;
        var secondShape = entry.SecondShape;
        var integerStride = first.IntegerCount + (secondShape?.IntegerCount ?? 0);
        var floatStride = first.FloatCount + (secondShape?.FloatCount ?? 0);
        var referenceStride = first.ReferenceCount + (secondShape?.ReferenceCount ?? 0);
        return (
            entry.IntegerBase + index * integerStride + (second ? first.IntegerCount : 0),
            entry.FloatBase + index * floatStride + (second ? first.FloatCount : 0),
            entry.ReferenceBase + index * referenceStride + (second ? first.ReferenceCount : 0));
    }

    private Entry RequireDictionary(CoflowCollectionId id)
    {
        var entry = EntryAt(id);
        if (entry.Kind != CoflowCollectionKind.Dictionary)
            throw new InvalidOperationException("An array handle cannot be read as a dictionary.");
        return entry;
    }

    private Entry EntryAt(CoflowCollectionId id)
    {
        if (!Contains(id))
            throw new InvalidOperationException("The collection handle is invalid or stale.");
        return _entries[_entryIndexes[id.Index]];
    }

    private CoflowCollectionId NewId()
    {
        uint index = _allocateIndex?.Invoke() ?? checked(_firstIndex + (uint)_entries.Count);
        if (index <= _firstIndex || !_entryIndexes.TryAdd(index, _entries.Count - 1))
            throw new InvalidOperationException("The collection index allocator returned an invalid or duplicate index.");
        _lastIndex = Math.Max(_lastIndex, index);
        return new CoflowCollectionId(_snapshotId, index);
    }

    private static void RequireIndex(Entry entry, int index)
    {
        if ((uint)index >= (uint)entry.Count) throw new ArgumentOutOfRangeException(nameof(index));
    }

    private static void RequireLayout(CoflowValueShape expected, CoflowValueShape actual)
    {
        if (expected.IntegerCount != actual.IntegerCount ||
            expected.FloatCount != actual.FloatCount ||
            expected.ReferenceCount != actual.ReferenceCount)
            throw new InvalidOperationException(
                $"collection value layout mismatch: `{actual.Type}` to `{expected.Type}`");
    }

    private static T[] Slice<T>(List<T> source, int start, int count)
    {
        if (count == 0) return Array.Empty<T>();
        var result = new T[count];
        source.CopyTo(start, result, 0, count);
        return result;
    }

    private static void AddDefaults<T>(List<T> target, int count)
    {
        if (count != 0) target.AddRange(new T[count]);
    }

    private sealed class Entry
    {
        internal Entry(CoflowCollectionKind kind, CoflowValueShape firstShape,
            CoflowValueShape? secondShape, int count, int capacity,
            int integerBase, int floatBase, int referenceBase)
        {
            Kind = kind;
            FirstShape = firstShape;
            SecondShape = secondShape;
            Count = count;
            Capacity = capacity;
            IntegerBase = integerBase;
            FloatBase = floatBase;
            ReferenceBase = referenceBase;
        }

        internal Dictionary<long, int>? IntegerKeys { get; set; }
        internal Dictionary<string, int>? StringKeys { get; set; }
        internal CoflowCollectionKind Kind { get; }
        internal CoflowValueShape FirstShape { get; }
        internal CoflowValueShape? SecondShape { get; }
        internal int Count { get; set; }
        internal int Capacity { get; }
        internal int IntegerBase { get; }
        internal int FloatBase { get; }
        internal int ReferenceBase { get; }
    }
}

internal static class CoflowCollectionEncoding
{
    internal static CoflowEncodedValue Encode(
        Type type,
        object? value,
        CoflowCollectionArena arena,
        bool budgetAlreadyCharged = false)
    {
        if (value is null) throw new CoflowBoundaryException($"A required `{type}` collection is null.");
        if (!TryCollection(type, out var definition, out var arguments))
            return CoflowEncodedValue.EncodeArenaField(type, value,
                (nestedType, nestedValue) => Encode(
                    nestedType, nestedValue, arena, budgetAlreadyCharged));

        var recursive = new Func<Type, object?, CoflowEncodedValue>(
            (nestedType, nestedValue) => Encode(
                nestedType, nestedValue, arena, budgetAlreadyCharged));
        var count = CollectionCount(type, value);
        if (!budgetAlreadyCharged) arena.ReserveElements(count);
        var id = arena.EncodeCollection(value, count, arguments, recursive);
        return new CoflowEncodedValue(CoflowValueShape.Of(type),
            new[] { unchecked((long)id.Packed) }, Array.Empty<double>(), Array.Empty<object?>());
    }

    private static int CollectionCount(Type type, object value)
    {
        var property = type.GetProperty(nameof(IReadOnlyCollection<object>.Count)) ??
            type.GetInterfaces().Select(candidate =>
                    candidate.GetProperty(nameof(IReadOnlyCollection<object>.Count)))
                .FirstOrDefault(candidate => candidate is not null);
        return (int)(property?.GetValue(value) ??
            throw new InvalidOperationException($"Collection `{type}` has no Count property."));
    }

    private static bool TryCollection(Type type, out Type definition, out Type[] arguments)
    {
        if (type.IsGenericType)
        {
            definition = type.GetGenericTypeDefinition();
            arguments = type.GetGenericArguments();
            if (definition == typeof(IReadOnlyList<>) ||
                definition == typeof(IReadOnlyDictionary<,>)) return true;
        }
        definition = null!;
        arguments = Array.Empty<Type>();
        return false;
    }
}

internal sealed record CoflowDictionaryEntryAccessors(
    Func<object, object?> Key,
    Func<object, object?> Value)
{
    private static readonly System.Runtime.CompilerServices.ConditionalWeakTable<
        Type, System.Runtime.CompilerServices.ConditionalWeakTable<Type, CoflowDictionaryEntryAccessors>> Cache = new();

    internal static CoflowDictionaryEntryAccessors For(Type key, Type value)
    {
        // collectible 类型不进入进程缓存，避免嵌套弱表的 value 图间接固定外层 key。
        if (CoflowExpressionCompiler.IsCollectible(key) || CoflowExpressionCompiler.IsCollectible(value))
            return Build(key, value);
        return Cache.GetValue(key, static _ => new()).GetValue(value, _ => Build(key, value));
    }

    private static CoflowDictionaryEntryAccessors Build(Type key, Type value)
    {
        var pairType = typeof(KeyValuePair<,>).MakeGenericType(key, value);
        var pair = System.Linq.Expressions.Expression.Parameter(typeof(object), "pair");
        var typed = System.Linq.Expressions.Expression.Convert(pair, pairType);
        Func<string, Func<object, object?>> reader = name => CoflowExpressionCompiler.CompileCollectibleSafe(
            System.Linq.Expressions.Expression.Lambda<Func<object, object?>>(
                System.Linq.Expressions.Expression.Convert(
                    System.Linq.Expressions.Expression.Property(typed, name), typeof(object)),
                pair), key, value);
        return new CoflowDictionaryEntryAccessors(reader("Key"), reader("Value"));
    }
}

internal static class CoflowCollectionMaterializer<T>
{
    internal static readonly Func<CoflowExecutionSession, CoflowCollectionId, T> Read = Build();

    private static Func<CoflowExecutionSession, CoflowCollectionId, T> Build()
    {
        var type = typeof(T);
        if (!type.IsGenericType)
            throw new InvalidOperationException($"`{type}` is not a Coflow collection type.");
        var definition = type.GetGenericTypeDefinition();
        var arguments = type.GetGenericArguments();
        var methodName = definition == typeof(IReadOnlyList<>) ? nameof(ReadArray) :
            definition == typeof(IReadOnlyDictionary<,>) ? nameof(ReadDictionary) :
            throw new InvalidOperationException($"`{type}` is not a Coflow collection type.");
        var method = typeof(CoflowCollectionMaterializer<T>).GetMethod(methodName,
            System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic)!
            .MakeGenericMethod(arguments);
        return (Func<CoflowExecutionSession, CoflowCollectionId, T>)method.CreateDelegate(
            typeof(Func<CoflowExecutionSession, CoflowCollectionId, T>));
    }

    private static IReadOnlyList<TElement> ReadArray<TElement>(
        CoflowExecutionSession context,
        CoflowCollectionId id)
    {
        if (context.CollectionKind(id) != CoflowCollectionKind.Array)
            throw new InvalidOperationException("A dictionary collection cannot be materialized as an array.");
        var values = new TElement[context.CollectionItemCount(id)];
        for (var index = 0; index < values.Length; index++)
            values[index] = context.DecodeEncoded<TElement>(context.ReadArrayItem(id, index));
        return Array.AsReadOnly(values);
    }

    private static IReadOnlyDictionary<TKey, TValue> ReadDictionary<TKey, TValue>(
        CoflowExecutionSession context,
        CoflowCollectionId id) where TKey : notnull
    {
        if (context.CollectionKind(id) != CoflowCollectionKind.Dictionary)
            throw new InvalidOperationException("An array collection cannot be materialized as a dictionary.");
        var values = new Dictionary<TKey, TValue>();
        var count = context.CollectionItemCount(id);
        for (var index = 0; index < count; index++)
            values.Add(
                context.DecodeEncoded<TKey>(context.ReadDictionaryKey(id, index)),
                context.DecodeEncoded<TValue>(context.ReadDictionaryValue(id, index)));
        return new System.Collections.ObjectModel.ReadOnlyDictionary<TKey, TValue>(values);
    }
}
}
