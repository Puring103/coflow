using System;
using System.Collections;
using System.Collections.Generic;
using System.Text;

namespace Coflow
{
    public sealed class Table<T> : IReadOnlyCollection<T>
    {
        private readonly Runtime runtime;
        private readonly TypeBinding<T> binding;
        internal Table(Runtime runtime, TypeBinding<T> binding) { this.runtime = runtime; this.binding = binding; }
        public int Count => checked((int)Native.Call(21, runtime.Handle, binding.Name).Length);
        public T this[string key] => TryGet(key, out var record) ? record : throw new KeyNotFoundException(key);
        public bool TryGet(string key, out T record)
        {
            if (key == null) throw new ArgumentNullException(nameof(key));
            var result = Native.Call(32, runtime.Handle, binding.Name, Encoding.UTF8.GetBytes(key));
            if (result.Handle == 0) { record = default!; return false; }
            record = binding.Read(new RuntimeValue(runtime, result.Handle)); return true;
        }
        public IEnumerator<T> GetEnumerator()
        {
            int count = Count;
            for (int i = 0; i < count; ++i) yield return binding.Read(new RuntimeValue(runtime, Native.Call(22, runtime.Handle, binding.Name, index: (ulong)i).Handle));
        }
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
    }
    public sealed class RuntimeArray<T> : IReadOnlyList<T>, IRuntimeValue
    {
        public RuntimeValue RuntimeValue { get; }
        private readonly Func<RuntimeValue, T> read;
        public RuntimeArray(RuntimeValue value, Func<RuntimeValue, T> read) { RuntimeValue = value; this.read = read; }
        public int Count => RuntimeValue.Count;
        public T this[int index] => read(RuntimeValue.At(index));
        public IEnumerator<T> GetEnumerator() { int count = Count; for (int i = 0; i < count; ++i) yield return this[i]; }
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
    }
    // 仅编码 ABI 标量；键类型判断与查找由 Rust 完成。
    public readonly struct DictionaryKey
    {
        internal ulong Tag { get; }
        internal string TypeName { get; }
        internal byte[] Bytes { get; }
        private DictionaryKey(ulong tag, byte[] bytes, string name = "") { Tag = tag; Bytes = bytes; TypeName = name; }
        public static DictionaryKey Bool(bool value) => new DictionaryKey(1, new byte[] { value ? (byte)1 : (byte)0 });
        public static DictionaryKey Int(int value) => new DictionaryKey(2, LittleEndian(unchecked((uint)value)));
        public static DictionaryKey String(string value) => new DictionaryKey(4, Encoding.UTF8.GetBytes(value));
        public static DictionaryKey Enum(string type, uint value) => new DictionaryKey(5, LittleEndian(value), type);
        private static byte[] LittleEndian(uint value) => new byte[] { (byte)value, (byte)(value >> 8), (byte)(value >> 16), (byte)(value >> 24) };
    }
    public sealed class RuntimeDictionary<K, V> : IReadOnlyCollection<KeyValuePair<K, V>>, IRuntimeValue
    {
        public RuntimeValue RuntimeValue { get; }
        private readonly Func<RuntimeValue, K> key;
        private readonly Func<RuntimeValue, V> read;
        private readonly Func<K, DictionaryKey> encode;
        public RuntimeDictionary(RuntimeValue value, Func<RuntimeValue, K> key, Func<RuntimeValue, V> read, Func<K, DictionaryKey> encode)
        { RuntimeValue = value; this.key = key; this.read = read; this.encode = encode; }
        public int Count => RuntimeValue.Count;
        public V this[K requested] => TryGetValue(requested, out var found) ? found : throw new KeyNotFoundException();
        public bool TryGetValue(K requested, out V found)
        {
            var value = RuntimeValue.Find(encode(requested));
            if (value.IsMissing) { found = default!; return false; }
            found = read(value); return true;
        }
        public KeyValuePair<K, V> At(int index) => new KeyValuePair<K, V>(key(RuntimeValue.KeyAt(index)), read(RuntimeValue.ValueAt(index)));
        public IEnumerator<KeyValuePair<K, V>> GetEnumerator() { int count = Count; for (int i = 0; i < count; ++i) yield return At(i); }
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
    }
}
