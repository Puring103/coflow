using System;
using System.Collections;
using System.Collections.Generic;

namespace Coflow
{
    public sealed class Table<T> : IReadOnlyCollection<T>
    {
        private readonly Runtime runtime;
        private readonly TypeBinding<T> binding;
        private readonly ulong[] ids;
        private readonly Dictionary<string, T> loaded = new Dictionary<string, T>(StringComparer.Ordinal);
        internal Table(Runtime runtime, TypeBinding<T> binding)
        {
            this.runtime = runtime; this.binding = binding;
            int count = checked((int)runtime.Execute(NativeOperation.TableLength, binding.Name).Length);
            ids = new ulong[count];
            for (int i = 0; i < count; ++i)
                ids[i] = runtime.Execute(NativeOperation.TableValue, binding.Name, index: (ulong)i).Handle;
        }
        public int Count { get { runtime.RequireThread(); return ids.Length; } }
        // 强类型表的唯一读取入口；调用方不需要接触快照或底层节点。
        public T Get(string key) => TryGet(key, out var record) ? record : throw new KeyNotFoundException(key);
        public bool TryGet(string key, out T record)
        {
            runtime.RequireThread();
            if (key == null) throw new ArgumentNullException(nameof(key));
            if (loaded.TryGetValue(key, out record!)) return true;
            var response = runtime.Execute(NativeOperation.TryFindRecord, binding.Name, System.Text.Encoding.UTF8.GetBytes(key));
            if (response.Handle != 0) { record = runtime.ResolveRecord<T>(response.Handle); loaded.Add(key, record); return true; }
            record = default!; return false;
        }
        public IEnumerator<T> GetEnumerator()
        {
            for (int i = 0; i < ids.Length; ++i)
                yield return runtime.ResolveRecord<T>(ids[i]);
        }
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
    }
    public sealed class CoflowArray<T> : IReadOnlyList<T>, ICoflowValue
    {
        private Projection projection;
        void ICoflowValue.Encode(ArgumentWriter writer) => writer.Write(projection);
        private readonly T[] managed;
        public CoflowArray(IEnumerable<T> values, Func<T, Projection>? encode = null)
        {
            if (values == null) throw new ArgumentNullException(nameof(values));
            managed = new List<T>(values).ToArray();
            var nodes = new Projection[managed.Length];
            for (int i = 0; i < nodes.Length; ++i) nodes[i] = encode == null ? Coflow.Projection.From(managed[i]) : encode(managed[i]);
            projection = Coflow.Projection.Collection(7, nodes);
        }
        public CoflowArray(Projection value, Func<Projection, T> read)
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
    public sealed class CoflowDictionary<K, V> : IReadOnlyCollection<KeyValuePair<K, V>>, ICoflowValue
    {
        private Projection projection;
        void ICoflowValue.Encode(ArgumentWriter writer) => writer.Write(projection);
        private readonly KeyValuePair<K, V>[] managed;
        private readonly Dictionary<K, int> managedIndex;
        public CoflowDictionary(IEnumerable<KeyValuePair<K, V>> values, Func<K, Projection>? encodeKey = null, Func<V, Projection>? encodeValue = null)
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
        public CoflowDictionary(Projection value, Func<Projection, K> key, Func<Projection, V> read)
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
