using System;
using System.Collections;
using System.Collections.Generic;

namespace Coflow.Runtime
{
    public sealed class CoflowArray<T> : IReadOnlyList<T>, IDisposable, ICoflowValue
    {
        private readonly CoflowValue value;
        private readonly Func<CoflowValue, T> read;
        public CoflowArray(CoflowValue value, Func<CoflowValue, T> read) { this.value = value; this.read = read; }
        public int Count => value.Count;
        public T this[int index] => read(value.At(index));
        public IEnumerator<T> GetEnumerator() { for (int i = 0; i < Count; ++i) yield return this[i]; }
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
        public CoflowValue RetainValue() => value.Retain();
        public void Dispose() => value.Dispose();
    }
    public sealed class CoflowDictionary<K,V> : IReadOnlyCollection<KeyValuePair<K,V>>, IDisposable, ICoflowValue
    {
        private readonly CoflowValue value;
        private readonly Func<CoflowValue, K> key;
        private readonly Func<CoflowValue, V> read;
        public CoflowDictionary(CoflowValue value, Func<CoflowValue,K> key, Func<CoflowValue,V> read)
        { this.value = value; this.key = key; this.read = read; }
        public int Count => value.Count;
        public KeyValuePair<K,V> At(int index) => new KeyValuePair<K,V>(key(value.KeyAt(index)), read(value.ValueAt(index)));
        public V this[K requested] => TryGetValue(requested, out var found) ? found : throw new KeyNotFoundException();
        public bool TryGetValue(K requested, out V found)
        {
            for (int i = 0; i < Count; ++i)
            {
                if (!EqualityComparer<K>.Default.Equals(key(value.KeyAt(i)), requested)) continue;
                found = read(value.ValueAt(i));
                return true;
            }
            found = default!;
            return false;
        }
        public IEnumerator<KeyValuePair<K,V>> GetEnumerator() { for (int i = 0; i < Count; ++i) yield return At(i); }
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
        public CoflowValue RetainValue() => value.Retain();
        public void Dispose() => value.Dispose();
    }
    public sealed class CoflowOptional<T> : IDisposable, ICoflowValue
    {
        private readonly CoflowValue value;
        private readonly Func<CoflowValue,T> read;
        public CoflowOptional(CoflowValue value, Func<CoflowValue,T> read) { this.value = value; this.read = read; }
        public bool HasValue => !value.IsNone;
        public T GetValue() => HasValue ? read(value.Retain()) : throw new InvalidOperationException("Optional value is None.");
        public CoflowValue RetainValue() => value.Retain();
        public void Dispose() => value.Dispose();
    }
}
