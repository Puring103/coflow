using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Text;

namespace Coflow
{
    public sealed class Runtime : IDisposable
    {
        private HostBinding[] hosts;
        private readonly List<ProjectionLease> leases = new List<ProjectionLease>();
        private int lastLeaseCollection = GC.CollectionCount(0);
        private sealed class ProjectionLease
        {
            internal readonly WeakReference<ValueImage> Image;
            internal readonly NativeHandle Handle;
            internal ProjectionLease(ValueImage image, NativeHandle handle) { Image = new WeakReference<ValueImage>(image); Handle = handle; }
        }
        internal void RetainProjection(ValueImage image, NativeHandle lease) => leases.Add(new ProjectionLease(image, lease));
        private void CollectProjectionLeases()
        {
            int collection = GC.CollectionCount(0);
            if (lastLeaseCollection == collection) return;
            lastLeaseCollection = collection;
            // 托管 GC 只改变弱引用；原生保活始终由下一次创建线程请求释放。
            for (int i = leases.Count - 1; i >= 0; --i)
                if (!leases[i].Image.TryGetTarget(out _)) {
                    leases[i].Handle.Dispose();
                    // lease 无顺序语义，交换尾项避免逐个移动剩余元素。
                    int last = leases.Count - 1;
                    leases[i] = leases[last];
                    leases.RemoveAt(last);
                }
        }
        [ThreadStatic] private static Dictionary<ulong, WeakReference<Runtime>>? registry;
        private static Dictionary<ulong, WeakReference<Runtime>> Registry => registry ??= new Dictionary<ulong, WeakReference<Runtime>>();
        internal NativeHandle Handle { get; }
        internal Contract Contract { get; }
        private readonly MaterializationCache records = new MaterializationCache();
        private readonly int executionThread = Environment.CurrentManagedThreadId;
        internal void RequireThread()
        {
            if (Environment.CurrentManagedThreadId != executionThread) throw new CoflowException("Runtime must be used on its creating thread.");
        }
        internal void RequireExecution()
        {
            RequireThread();
            if (Handle.IsClosed) throw new ObjectDisposedException(nameof(Runtime));
        }
        internal Response Execute(NativeOperation operation, string key = "", byte[]? data = null, ulong index = 0, ulong value = 0)
        {
            RequireExecution();
            CollectProjectionLeases();
            try { return Native.Call(operation, Handle, key, data, index, value); }
            finally { GC.KeepAlive(hosts); GC.KeepAlive(this); }
        }
        internal Runtime(ulong handle, Contract contract, HostBinding[] hosts)
        {
            Handle = new NativeHandle(handle); Contract = contract; this.hosts = hosts;
            Registry.Add(handle, new WeakReference<Runtime>(this));
        }
        internal static Runtime Lookup(ulong handle)
        {
            if (Registry.TryGetValue(handle, out var weak) && weak.TryGetTarget(out var runtime)) { runtime.RequireExecution(); return runtime; }
            Registry.Remove(handle);
            throw new CoflowException("Runtime is no longer available on its creating thread.");
        }
        internal static void ClearRegistry()
        {
            if (registry == null) return;
            foreach (var weak in registry.Values)
                if (weak.TryGetTarget(out var runtime)) runtime.ReleaseManagedResources();
            registry.Clear();
        }
        public Table<T> Table<T>()
        {
            RequireExecution();
            var binding = Contract.Binding<T>();
            return new Table<T>(this, binding);
        }
        public T Get<T>()
        {
            RequireExecution();
            var binding = Contract.Binding<T>();
            var response = Execute(NativeOperation.Singleton, binding.Name);
            return ResolveRecord<T>(response.Handle);
        }
        internal T ResolveRecord<T>(ulong id)
        {
            RequireThread();
            if (records.Values.TryGetValue(id, out var existing))
                return existing is T typed ? typed : throw new CoflowException("Record type mismatch.");
            RequireExecution();
            var image = ValueImage.Read(Native.ReadBuffer(Execute(NativeOperation.ReadRecord, value: id)));
            image.Objects = records;
            return Materialize<T>(id, image);
        }
        internal T ResolveEmbedded<T>(ulong id, ValueImage image)
        {
            RequireThread();
            if (image.Objects.Values.TryGetValue(id, out var existing))
                return existing is T typed ? typed : throw new CoflowException("Data type mismatch.");
            return Materialize<T>(id, image);
        }
        private T Materialize<T>(ulong id, ValueImage image)
        {
            var node = image.Get(id);
            var aliasSet = new HashSet<ulong>();
            var aliasQueue = new Queue<ulong>();
            foreach (var alias in node.Bases.Values) aliasQueue.Enqueue(alias);
            while (aliasQueue.Count != 0)
            {
                var alias = aliasQueue.Dequeue();
                if (!aliasSet.Add(alias)) continue;
                foreach (var parent in image.Get(alias).Bases.Values) aliasQueue.Enqueue(parent);
            }
            var aliases = new ulong[aliasSet.Count];
            aliasSet.CopyTo(aliases);
            image.Objects.PendingAliases[id] = aliases;
            try
            {
                var value = Contract.Binding(node.Type).Materialize(new Projection(this, id, image));
                if (!image.Objects.Values.TryGetValue(id, out var existing)) { existing = value; image.Objects.Values.Add(id, value); }
                return existing is T result ? result : throw new CoflowException("Runtime object type mismatch.");
            }
            catch
            {
                image.Objects.Values.Remove(id);
                foreach (var alias in aliases) image.Objects.Values.Remove(alias);
                throw;
            }
            finally { image.Objects.PendingAliases.Remove(id); }
        }
        internal void PublishRecord(ulong id, object value, ValueImage image)
        {
            if (image.Objects.Values.TryGetValue(id, out var existing) && !ReferenceEquals(existing, value))
                throw new CoflowException("Record was already materialized.");
            image.Objects.Values[id] = value;
            if (image.Objects.PendingAliases.TryGetValue(id, out var aliases))
                foreach (var alias in aliases) image.Objects.Values[alias] = value;
        }
        public CheckResult RunChecks(CheckOptions? options = null) => CheckResult.Run(this, options ?? CheckOptions.Default);
        public void Dispose()
        {
            RequireThread();
            if (Handle.IsClosed) { ReleaseManagedResources(); return; }
            // 原生层在活动调用和 Host 重入期间拒绝释放；失败时必须保留完整托管状态。
            if (!Handle.DisposeExplicit()) throw new InvalidOperationException("Runtime is busy and cannot be disposed.");
            Registry.Remove(Handle.Id);
            ReleaseManagedResources();
        }
        private void ReleaseManagedResources()
        {
            foreach (var lease in leases) lease.Handle.Dispose();
            leases.Clear();
            records.PendingAliases.Clear();
            hosts = Array.Empty<HostBinding>();
        }
    }
    public static class RuntimeThread
    {
        public static void Shutdown()
        {
            if (Native.ThreadShutdown() != 0)
                throw new InvalidOperationException("The Coflow runtime thread is busy and cannot be shut down.");
            ++Native.DomainVersion;
            Runtime.ClearRegistry();
        }
    }

}
