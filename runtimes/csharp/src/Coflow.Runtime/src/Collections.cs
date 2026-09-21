using System;
using System.Collections;
using System.Collections.Generic;

namespace Coflow
{
    public sealed class Table<T> : IReadOnlyCollection<T>
    {
        private readonly Runtime runtime;
        private readonly TypeBinding<T> binding;
        internal Table(Runtime runtime, TypeBinding<T> binding) { this.runtime = runtime; this.binding = binding; }
        public int Count => runtime.Image.Tables[binding.Name].Length;
        // 强类型表的唯一读取入口；调用方不需要接触快照或底层节点。
        public T Get(string key) => TryGet(key, out var record) ? record : throw new KeyNotFoundException(key);
        public bool TryGet(string key, out T record)
        {
            if (key == null) throw new ArgumentNullException(nameof(key));
            if (runtime.Image.TableKeys[binding.Name].TryGetValue(key, out var id)) { record = binding.Read(new Projection(runtime, id)); return true; }
            record = default!; return false;
        }
        public IEnumerator<T> GetEnumerator()
        {
            foreach (ulong id in runtime.Image.Tables[binding.Name]) yield return binding.Read(new Projection(runtime, id));
        }
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
    }
    public sealed class RuntimeArray<T> : IReadOnlyList<T>, IRuntimeArgument
    {
        private Projection projection;
        void IRuntimeArgument.Encode(ArgumentWriter writer) => writer.Write(projection);
        private readonly T[] managed;
        public RuntimeArray(IEnumerable<T> values, Func<T, Projection>? encode = null)
        {
            if (values == null) throw new ArgumentNullException(nameof(values));
            managed = new List<T>(values).ToArray();
            var nodes = new Projection[managed.Length];
            for (int i = 0; i < nodes.Length; ++i) nodes[i] = encode == null ? Coflow.Projection.From(managed[i]) : encode(managed[i]);
            projection = Coflow.Projection.Collection(7, nodes);
        }
        public RuntimeArray(Projection value, Func<Projection, T> read)
        {
            projection = value;
            managed = new T[value.Count];
            for (int i = 0; i < managed.Length; ++i) managed[i] = value.At(i).ReadProjected(read);
        }
        public int Count => managed.Length;
        public T this[int index] => managed[index];
        public IEnumerator<T> GetEnumerator() { int count = Count; for (int i = 0; i < count; ++i) yield return this[i]; }
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
    }
    public sealed class RuntimeDictionary<K, V> : IReadOnlyCollection<KeyValuePair<K, V>>, IRuntimeArgument
    {
        private Projection projection;
        void IRuntimeArgument.Encode(ArgumentWriter writer) => writer.Write(projection);
        private readonly KeyValuePair<K, V>[] managed;
        private readonly Dictionary<K, int> managedIndex;
        public RuntimeDictionary(IEnumerable<KeyValuePair<K, V>> values, Func<K, Projection>? encodeKey = null, Func<V, Projection>? encodeValue = null)
        {
            if (values == null) throw new ArgumentNullException(nameof(values));
            managed = new List<KeyValuePair<K, V>>(values).ToArray();
            managedIndex = new Dictionary<K, int>();
            var nodes = new Projection[checked(managed.Length * 2)];
            for (int i = 0; i < managed.Length; ++i) {
                if (managedIndex.ContainsKey(managed[i].Key)) throw new ArgumentException("Duplicate dictionary key.");
                managedIndex.Add(managed[i].Key, i);
                nodes[2 * i] = encodeKey == null ? Coflow.Projection.From(managed[i].Key) : encodeKey(managed[i].Key);
                nodes[2 * i + 1] = encodeValue == null ? Coflow.Projection.From(managed[i].Value) : encodeValue(managed[i].Value);
            }
            projection = Coflow.Projection.Collection(8, nodes);
        }
        public RuntimeDictionary(Projection value, Func<Projection, K> key, Func<Projection, V> read)
        {
            projection = value;
            managed = new KeyValuePair<K, V>[value.Count]; managedIndex = new Dictionary<K, int>();
            for (int i = 0; i < managed.Length; ++i) {
                var entryKey = value.KeyAt(i).ReadProjected(key);
                managed[i] = new KeyValuePair<K, V>(entryKey, value.ValueAt(i).ReadProjected(read));
                managedIndex.Add(entryKey, i);
            }
        }
        public int Count => managed.Length;
        public V this[K requested] => TryGetValue(requested, out var found) ? found : throw new KeyNotFoundException();
        public bool TryGetValue(K requested, out V found)
        {
            if (managedIndex.TryGetValue(requested, out var index)) { found = managed[index].Value; return true; }
            found = default!; return false;
        }
        public KeyValuePair<K, V> At(int index) => managed[index];
        public IEnumerator<KeyValuePair<K, V>> GetEnumerator() { int count = Count; for (int i = 0; i < count; ++i) yield return At(i); }
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
    }
}
