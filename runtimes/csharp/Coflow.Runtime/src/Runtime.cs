using System;
using System.Collections.Generic;
using System.Text;

namespace Coflow
{
    // 生成器显式引用所有工厂，IL2CPP 无需反射或运行时构造泛型代码。
    public abstract class TypeBinding
    {
        internal Type ManagedType { get; }
        internal string Name { get; }
        protected TypeBinding(Type type, string name) { ManagedType = type; Name = name; }
    }
    public sealed class TypeBinding<T> : TypeBinding
    {
        internal Func<RuntimeValue, T> Read { get; }
        public TypeBinding(string name, Func<RuntimeValue, T> read) : base(typeof(T), name) { Read = read; }
    }
    public sealed class Contract
    {
        internal NativeHandle Handle { get; }
        private readonly byte[] identity;
        private readonly Dictionary<Type, TypeBinding> bindings = new Dictionary<Type, TypeBinding>();
        public Contract(byte[] bytes, params TypeBinding[] types)
        {
            Handle = new NativeHandle(Native.Call(1, data: bytes).Handle);
            try
            {
                identity = Native.ReadBuffer(Native.Call(8, Handle));
                foreach (var type in types) bindings.Add(type.ManagedType, type);
            }
            catch { Handle.Dispose(); throw; }
        }
        internal TypeBinding<T> Binding<T>() => bindings.TryGetValue(typeof(T), out var binding)
            ? (TypeBinding<T>)binding : throw new CoflowException("Type is not registered in this contract.");
        internal bool Matches(Contract other)
        {
            if (ReferenceEquals(this, other)) return true;
            if (identity.Length != other.identity.Length) return false;
            for (int i = 0; i < identity.Length; ++i) if (identity[i] != other.identity[i]) return false;
            return true;
        }
    }
    public sealed class RuntimeBuilder : IDisposable
    {
        internal NativeHandle Handle { get; }
        internal Contract Contract { get; }
        public RuntimeBuilder(Contract contract)
        {
            Contract = contract ?? throw new ArgumentNullException(nameof(contract));
            Handle = new NativeHandle(Native.Call(10, contract.Handle).Handle);
        }
        public RuntimeBuilder AddSource(string text, string? sourceName = null)
        {
            if (text == null) throw new ArgumentNullException(nameof(text));
            Native.Call(11, Handle, sourceName ?? "", Encoding.UTF8.GetBytes(text));
            return this;
        }
        public RuntimeBuilder BindHost(HostBinding binding)
        {
            if (binding == null) throw new ArgumentNullException(nameof(binding));
            if (!Contract.Matches(binding.Contract)) throw new CoflowException("Host belongs to a different contract.");
            HostBridge.Bind(this, binding);
            return this;
        }
        public Runtime Build() => new Runtime(Native.Call(12, Handle).Handle, Contract);
        public void Dispose() => Handle.Dispose();
    }
    public sealed class Runtime : IDisposable
    {
        internal NativeHandle Handle { get; }
        internal Contract Contract { get; }
        internal Runtime(ulong handle, Contract contract) { Handle = new NativeHandle(handle); Contract = contract; }
        public Table<T> Table<T>()
        {
            var binding = Contract.Binding<T>();
            Native.Call(21, Handle, binding.Name);
            return new Table<T>(this, binding);
        }
        public T Singleton<T>()
        {
            var binding = Contract.Binding<T>();
            return binding.Read(new RuntimeValue(this, Native.Call(37, Handle, binding.Name).Handle));
        }
        public void Dispose() => Handle.Dispose();
    }
    public enum ValueKind : uint { None, Bool, Int, Float, String, Enum, Object, Array, Dictionary, Function, Template }
    public interface IRuntimeValue { RuntimeValue RuntimeValue { get; } }

    // 值 ID 只在所属 Runtime 内有效，不分配独立原生句柄，也不要求单独释放。
    public readonly struct RuntimeValue : IEquatable<RuntimeValue>, IRuntimeValue
    {
        internal Runtime Owner { get; }
        internal ulong Id { get; }
        internal RuntimeValue(Runtime owner, ulong id) { Owner = owner; Id = id; }
        RuntimeValue IRuntimeValue.RuntimeValue => this;
        internal Response Request(uint op, string key = "", byte[]? data = null, ulong index = 0)
        {
            if (Owner == null || Id == 0) throw new CoflowException("Invalid value.");
            return Native.Call(op, Owner.Handle, key, data, index, Id);
        }
        private RuntimeValue Child(Response result) => new RuntimeValue(Owner, result.Handle);
        public ValueKind Kind => (ValueKind)Request(24).Tag;
        public int Count => checked((int)Request(24).Length);
        public bool IsNone => Kind == ValueKind.None;
        public bool Bool { get { var r = Request(24); Require(r.Tag, 1); return r.Integer != 0; } }
        public int Int { get { var r = Request(24); Require(r.Tag, 2); return checked((int)r.Integer); } }
        public float Float { get { var r = Request(24); Require(r.Tag, 3); return (float)r.Number; } }
        public uint Enum { get { var r = Request(24); Require(r.Tag, 5); return checked((uint)r.Integer); } }
        public string Text => Encoding.UTF8.GetString(Native.ReadBuffer(Request(25)));
        public string TypeName => Encoding.UTF8.GetString(Native.ReadBuffer(Request(30)));
        public string ProgramSource => Encoding.UTF8.GetString(Native.ReadBuffer(Request(31)));
        public RuntimeValue Field(string name) => Child(Request(23, name));
        public RuntimeValue Canonical() => Child(Request(39));
        public RuntimeValue DimensionDefault() => Child(Request(36));
        public RuntimeValue DimensionValue(string variant) => Child(Request(34, variant));
        public RuntimeValue At(int index) => Element(26, index);
        public RuntimeValue KeyAt(int index) => Element(27, index);
        public RuntimeValue ValueAt(int index) => Element(28, index);
        private RuntimeValue Element(uint op, int index)
        {
            if (index < 0) throw new ArgumentOutOfRangeException(nameof(index));
            return Child(Request(op, index: (ulong)index));
        }
        internal RuntimeValue Find(DictionaryKey key) => Child(Request(38, key.TypeName, key.Bytes, key.Tag));
        internal bool IsMissing => Id == 0;
        public void Call() => Request(29);
        public void RequireContract(Contract contract)
        {
            if (Owner == null || !Owner.Contract.Matches(contract)) throw new CoflowException("Generated types do not match the runtime contract.");
        }
        public bool ValueEquals(RuntimeValue other) => ReferenceEquals(Owner, other.Owner) && Request(35, index: other.Id).Integer != 0;
        public bool Equals(RuntimeValue other) => ReferenceEquals(Owner, other.Owner) && Id == other.Id;
        public override bool Equals(object? other) => other is RuntimeValue value && Equals(value);
        public override int GetHashCode() => unchecked((Owner?.GetHashCode() ?? 0) * 397 ^ Id.GetHashCode());
        private static void Require(uint actual, uint expected) { if (actual != expected) throw new CoflowException("Value type mismatch."); }
    }
    public abstract class RuntimeObject : IRuntimeValue
    {
        protected RuntimeValue Value { get; }
        protected RuntimeObject(RuntimeValue value) { Value = value; }
        public RuntimeValue RuntimeValue => Value;
        public string ActualType => Value.TypeName;
        protected T Read<T>(string field, Func<RuntimeValue, T> read) => read(Value.Field(field));
        public bool ValueEquals(RuntimeObject other) => other != null && Value.ValueEquals(other.Value);
        public override bool Equals(object? other) => other is RuntimeObject value && Value.Equals(value.Value);
        public override int GetHashCode() => Value.GetHashCode();
        public static bool operator ==(RuntimeObject? left, RuntimeObject? right) => ReferenceEquals(left, right) || (!(left is null) && left.Equals(right));
        public static bool operator !=(RuntimeObject? left, RuntimeObject? right) => !(left == right);
    }
    public sealed class RuntimeFunction : IRuntimeValue
    {
        public RuntimeValue RuntimeValue { get; }
        public RuntimeFunction(RuntimeValue value) { RuntimeValue = value; }
        public string Source => RuntimeValue.ProgramSource;
        public void Call() => RuntimeValue.Call();
    }
    public static class ValueCodecs
    {
        public static T? OptionalValue<T>(RuntimeValue value, Func<RuntimeValue, T> read) where T : struct => value.IsNone ? (T?)null : read(value);
        public static T? OptionalReference<T>(RuntimeValue value, Func<RuntimeValue, T> read) where T : class => value.IsNone ? null : read(value);
        public static int Int(RuntimeValue value) => value.Int;
        public static float Float(RuntimeValue value) => value.Float;
        public static bool Bool(RuntimeValue value) => value.Bool;
        public static string String(RuntimeValue value) => value.Text;
        public static uint Enum(RuntimeValue value) => value.Enum;
    }
}
