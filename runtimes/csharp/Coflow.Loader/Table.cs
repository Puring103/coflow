using System;
using System.Collections;
using System.Collections.Generic;
using System.Collections.ObjectModel;

namespace Coflow.Loader
{
/// <summary>A read-only table of records sharing their loaded identity.</summary>
public sealed class Table<T> : IReadOnlyDictionary<string, T>
{
    private readonly IReadOnlyDictionary<string, T> _records;
    internal Table(Dictionary<string, T> records) => _records = new ReadOnlyDictionary<string, T>(records);
    public T this[string key] => _records[key];
    public int Count => _records.Count;
    public IEnumerable<string> Keys => _records.Keys;
    public IEnumerable<T> Values => _records.Values;
    public bool ContainsKey(string key) => _records.ContainsKey(key);
    public bool TryGetValue(string key, out T value) => _records.TryGetValue(key, out value!);
    public IEnumerator<KeyValuePair<string, T>> GetEnumerator() => _records.GetEnumerator();
    IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
}

/// <summary>Maps generated enum keys to their declared CFD record keys.</summary>
public sealed class TableKeyMap<TKey> where TKey : struct, Enum
{
    private readonly IReadOnlyDictionary<TKey, string> _keys;
    public TableKeyMap(IReadOnlyDictionary<TKey, string> keys) => _keys = keys;
    public string GetKey(TKey key) => _keys.TryGetValue(key, out var text)
        ? text : throw new KeyNotFoundException($"Unknown table key `{key}`.");
    public bool TryGetKey(TKey key, out string text) => _keys.TryGetValue(key, out text!);
}
}
