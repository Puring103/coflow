using System;

namespace Coflow.Loader
{
// 仅复用字段与类型名称；记录 id 和普通值不进入池，容量随 schema 名称而非健康记录数增长。
internal sealed class CfdNamePool
{
    private struct Entry
    {
        internal int Hash;
        internal int Next;
        internal string Value;
    }

    private int[] _buckets = new int[16];
    private Entry[] _entries = new Entry[16];
    private int _count;

    internal string GetOrAdd(ReadOnlySpan<char> text)
    {
        var builder = new HashCode();
        // 两个 UTF-16 码元无损合并后再混合，减少 Mono 上逐字符 HashCode.Add 的调用。
        // 长度区分末尾半字，仍使用随机种子及完整 Ordinal 比较处理碰撞。
        builder.Add(text.Length);
        var character = 0;
        for (; character + 1 < text.Length; character += 2)
            builder.Add(text[character] | (text[character + 1] << 16));
        if (character < text.Length) builder.Add((int)text[character]);
        var hash = builder.ToHashCode() & int.MaxValue;
        var bucket = hash & (_buckets.Length - 1);
        for (var index = _buckets[bucket] - 1; index >= 0; index = _entries[index].Next)
            if (_entries[index].Hash == hash && text.SequenceEqual(_entries[index].Value.AsSpan()))
                return _entries[index].Value;

        if (_count == _entries.Length)
        {
            System.Array.Resize(ref _entries, checked(_entries.Length * 2));
            _buckets = new int[_entries.Length];
            for (var index = 0; index < _count; index++)
            {
                var target = _entries[index].Hash & (_buckets.Length - 1);
                _entries[index].Next = _buckets[target] - 1;
                _buckets[target] = index + 1;
            }
            bucket = hash & (_buckets.Length - 1);
        }
        var value = text.ToString();
        _entries[_count] = new Entry { Hash = hash, Next = _buckets[bucket] - 1, Value = value };
        _buckets[bucket] = ++_count;
        return value;
    }
}
}
