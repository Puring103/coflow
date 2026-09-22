using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Text;

namespace Coflow
{
    // 生成器显式引用所有工厂，IL2CPP 无需反射或运行时构造泛型代码。
    public abstract class TypeBinding
    {
        internal Type ManagedType { get; }
        internal string Name { get; }
        internal abstract object Materialize(Projection value);
        protected TypeBinding(Type type, string name) { ManagedType = type; Name = name; }
    }
    public sealed class TypeBinding<T> : TypeBinding
    {
        internal Func<Projection, T> Read { get; }
        internal override object Materialize(Projection value) => Read(value)!;
        public TypeBinding(string name, Func<Projection, T> read) : base(typeof(T), name) { Read = read; }
    }
    public sealed class Contract : IDisposable
    {
        internal NativeHandle Handle { get; }
        private readonly byte[] identity;
        private readonly Dictionary<Type, TypeBinding> bindings = new Dictionary<Type, TypeBinding>();
        private readonly Dictionary<string, TypeBinding> namedBindings = new Dictionary<string, TypeBinding>(StringComparer.Ordinal);
        internal TypeBinding Binding(string name) => namedBindings.TryGetValue(name, out var binding) ? binding : throw new CoflowException("Value type is not registered in this contract.");
        public Contract(byte[] bytes, byte[] expectedIdentity, params TypeBinding[] types)
        {
            if (bytes == null) throw new ArgumentNullException(nameof(bytes));
            if (expectedIdentity == null) throw new ArgumentNullException(nameof(expectedIdentity));
            Handle = new NativeHandle(Native.Call(NativeOperation.LoadContract, data: bytes).Handle);
            try
            {
                // 契约内容只交给 Rust 解析；托管层仅核对它与生成绑定的身份是否一致。
                identity = Native.ReadBuffer(Native.Call(NativeOperation.ContractIdentity, Handle));
                if (!Matches(expectedIdentity)) throw new CoflowException("Contract file does not match the generated C# bindings.");
                foreach (var type in types) { bindings.Add(type.ManagedType, type); namedBindings.Add(type.Name, type); }
            }
            catch { Handle.Dispose(); throw; }
        }
        internal TypeBinding<T> Binding<T>() => bindings.TryGetValue(typeof(T), out var binding)
            ? (TypeBinding<T>)binding : throw new CoflowException("Type is not registered in this contract.");
        internal bool Matches(Contract other)
        {
            if (ReferenceEquals(this, other)) return true;
            return Matches(other.identity);
        }
        internal bool Matches(byte[] expected)
        {
            if (identity.Length != expected.Length) return false;
            for (int i = 0; i < identity.Length; ++i) if (identity[i] != expected[i]) return false;
            return true;
        }
        public void Dispose() => Handle.Dispose();
    }
    public sealed class RuntimeBuilder : IDisposable
    {
        private readonly List<HostBinding> hosts = new List<HostBinding>();
        internal NativeHandle Handle { get; }
        internal Contract Contract { get; }
        public RuntimeBuilder(Contract contract)
        {
            Contract = contract ?? throw new ArgumentNullException(nameof(contract));
            Handle = new NativeHandle(Native.Call(NativeOperation.CreateBuilder, contract.Handle).Handle);
        }
        public RuntimeBuilder AddSource(string text, string? sourceName = null)
        {
            if (text == null) throw new ArgumentNullException(nameof(text));
            Native.Call(NativeOperation.AddDataSource, Handle, sourceName ?? "", Encoding.UTF8.GetBytes(text));
            return this;
        }
        public RuntimeBuilder BindHost(HostBinding binding)
        {
            if (binding == null) throw new ArgumentNullException(nameof(binding));
            HostBridge.Bind(this, binding);
            hosts.Add(binding);
            return this;
        }
        public Runtime Build()
        {
            try {
                var bindings = hosts.ToArray();
                var handle = Native.Call(NativeOperation.BuildRuntime, Handle).Handle;
                try { return new Runtime(handle, Contract, bindings); }
                finally { hosts.Clear(); }
            }
            finally { GC.KeepAlive(this); }
        }
        public void Dispose() { Handle.Dispose(); hosts.Clear(); }
    }

}
