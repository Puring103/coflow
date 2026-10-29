using System;
using System.Collections;
using System.Collections.Generic;
using System.Collections.ObjectModel;

namespace Coflow.Loader
{
/// <summary>A read-only table of records sharing their loaded identity.</summary>
public sealed class Table<T> : IReadOnlyDictionary<string, T>
{
    private readonly Dictionary<string, T> _records;
    private readonly ReadOnlyDictionary<string, T> _view;
    internal Table(Dictionary<string, T> records) { _records = records; _view = new ReadOnlyDictionary<string, T>(records); }
    // 字典由加载器独占且建表后只读；热查询直接访问具体类型，键和值仍交付原只读视图。
    public T this[string key] => _records[key];
    public int Count => _records.Count;
    public IEnumerable<string> Keys => _view.Keys;
    public IEnumerable<T> Values => _view.Values;
    public bool ContainsKey(string key) => _records.ContainsKey(key);
    public bool TryGetValue(string key, out T value) => _records.TryGetValue(key, out value!);
    public IEnumerator<KeyValuePair<string, T>> GetEnumerator() => _records.GetEnumerator();
    IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
}

/// <summary>Maps generated enum IDs to their declared CFD record IDs.</summary>
public sealed class TableIdMap<TId> where TId : struct, Enum
{
    private readonly IReadOnlyDictionary<TId, string> _ids;
    public TableIdMap(IReadOnlyDictionary<TId, string> ids) => _ids = ids;
    public string GetId(TId id) => _ids.TryGetValue(id, out var text)
        ? text : throw new KeyNotFoundException($"Unknown table id `{id}`.");
    public bool TryGetId(TId id, out string text) => _ids.TryGetValue(id, out text!);
}
}
