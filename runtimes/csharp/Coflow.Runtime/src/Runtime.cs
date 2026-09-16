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
    public sealed class Contract : IDisposable
    {
        internal NativeHandle Handle { get; }
        private readonly byte[] identity;
        private readonly Dictionary<Type, TypeBinding> bindings = new Dictionary<Type, TypeBinding>();
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
                foreach (var type in types) bindings.Add(type.ManagedType, type);
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
            return this;
        }
        public Runtime Build() => new Runtime(Native.Call(NativeOperation.BuildRuntime, Handle).Handle, Contract);
        public void Dispose() => Handle.Dispose();
    }
    public sealed class Runtime : IDisposable
    {
        private static readonly object RegistryLock = new object();
        private static readonly Dictionary<ulong, WeakReference<Runtime>> Registry = new Dictionary<ulong, WeakReference<Runtime>>();
        internal NativeHandle Handle { get; }
        internal Contract Contract { get; }
        internal Runtime(ulong handle, Contract contract)
        {
            Handle = new NativeHandle(handle); Contract = contract;
            lock (RegistryLock) Registry.Add(handle, new WeakReference<Runtime>(this));
        }
        internal static Runtime Lookup(ulong handle)
        {
            lock (RegistryLock)
            {
                if (Registry.TryGetValue(handle, out var weak) && weak.TryGetTarget(out var runtime)) return runtime;
            }
            throw new CoflowException("Runtime is no longer available.");
        }
        public Table<T> Table<T>()
        {
            var binding = Contract.Binding<T>();
            Native.Call(NativeOperation.TableLength, Handle, binding.Name);
            return new Table<T>(this, binding);
        }
        public T Singleton<T>()
        {
            var binding = Contract.Binding<T>();
            return binding.Read(new RuntimeValue(this, Native.Call(NativeOperation.Singleton, Handle, binding.Name).Handle));
        }
        public CheckResult RunChecks(CheckOptions? options = null) => CheckResult.Run(this, options ?? CheckOptions.Default);
        public void Dispose()
        {
            lock (RegistryLock) Registry.Remove(Handle.Id);
            Handle.Dispose();
        }
    }
    public enum ValueKind : uint { None, Bool, Int, Float, String, Enum, Object, Array, Dictionary, Function, Template, Dimension }
    public interface IRuntimeValue { RuntimeValue RuntimeValue { get; } }

    // 值 ID 只在所属 Runtime 内有效，不分配独立原生句柄，也不要求单独释放。
    public readonly struct RuntimeValue : IEquatable<RuntimeValue>, IRuntimeValue
    {
        internal Runtime Owner { get; }
        internal ulong Id { get; }
        internal RuntimeValue(Runtime owner, ulong id) { Owner = owner; Id = id; }
        RuntimeValue IRuntimeValue.RuntimeValue => this;
        internal Response Request(NativeOperation op, string key = "", byte[]? data = null, ulong index = 0)
        {
            if (Owner == null || Id == 0) throw new CoflowException("Invalid value.");
            return Native.Call(op, Owner.Handle, key, data, index, Id);
        }
        private RuntimeValue Child(Response result) => new RuntimeValue(Owner, result.Handle);
        public ValueKind Kind => (ValueKind)Request(NativeOperation.InspectValue).Tag;
        public int Count => checked((int)Request(NativeOperation.InspectValue).Length);
        public bool IsNone => Kind == ValueKind.None;
        public bool Bool { get { var r = Request(NativeOperation.InspectValue); Require(r.Tag, 1); return r.Integer != 0; } }
        public int Int { get { var r = Request(NativeOperation.InspectValue); Require(r.Tag, 2); return checked((int)r.Integer); } }
        public float Float { get { var r = Request(NativeOperation.InspectValue); Require(r.Tag, 3); return (float)r.Number; } }
        public uint Enum { get { var r = Request(NativeOperation.InspectValue); Require(r.Tag, 5); return checked((uint)r.Integer); } }
        public string Text => Encoding.UTF8.GetString(Native.ReadBuffer(Request(NativeOperation.ReadText)));
        public string TypeName => Encoding.UTF8.GetString(Native.ReadBuffer(Request(NativeOperation.TypeName)));
        public string ProgramSource => Encoding.UTF8.GetString(Native.ReadBuffer(Request(NativeOperation.ProgramSource)));
        public RuntimeValue Field(string name) => Child(Request(NativeOperation.ReadField, name));
        public RuntimeValue Canonical() => Child(Request(NativeOperation.CanonicalValue));
        public RuntimeValue DimensionDefault() => Child(Request(NativeOperation.DimensionDefault));
        public RuntimeValue DimensionValue(string variant) => Child(Request(NativeOperation.DimensionVariant, variant));
        public string DimensionVariantAt(int index)
        {
            if (index < 0) throw new ArgumentOutOfRangeException(nameof(index));
            return Encoding.UTF8.GetString(Native.ReadBuffer(Request(NativeOperation.DimensionVariantKey, index: (ulong)index)));
        }
        public RuntimeValue At(int index) => Element(NativeOperation.ArrayValue, index);
        public RuntimeValue KeyAt(int index) => Element(NativeOperation.DictionaryKey, index);
        public RuntimeValue ValueAt(int index) => Element(NativeOperation.DictionaryValue, index);
        private RuntimeValue Element(NativeOperation op, int index)
        {
            if (index < 0) throw new ArgumentOutOfRangeException(nameof(index));
            return Child(Request(op, index: (ulong)index));
        }
        internal RuntimeValue Find(DictionaryKey key) => Child(Request(NativeOperation.DictionaryFind, key.TypeName, key.Bytes, key.Tag));
        internal bool IsMissing => Id == 0;
        internal Response Invoke(byte[] arguments) => Request(NativeOperation.Invoke, data: arguments);
        public void RequireContract(byte[] identity)
        {
            if (Owner == null || !Owner.Contract.Matches(identity)) throw new CoflowException("Generated types do not match the runtime contract.");
        }
        public bool ValueEquals(RuntimeValue other) => ReferenceEquals(Owner, other.Owner) && Request(NativeOperation.ValueEquals, index: other.Id).Integer != 0;
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
    public readonly struct Unit : IEquatable<Unit>
    {
        public bool Equals(Unit other) => true;
        public override bool Equals(object? other) => other is Unit;
        public override int GetHashCode() => 0;
        public override string ToString() => "()";
        public static bool operator ==(Unit left, Unit right) => true;
        public static bool operator !=(Unit left, Unit right) => false;
    }
    public sealed class InvocationCodec<T>
    {
        internal Func<RuntimeValue, T> ReadValue { get; }
        internal Action<InvocationWriter, T> WriteArgument { get; }
        internal Func<Runtime, Response, T> ReadResult { get; }
        internal InvocationCodec(Func<RuntimeValue, T> readValue, Action<InvocationWriter, T> writeArgument,
            Func<Runtime, Response, T> readResult)
        {
            ReadValue = readValue;
            WriteArgument = writeArgument;
            ReadResult = readResult;
        }
    }
    internal sealed class InvocationWriter
    {
        private readonly List<byte> bytes = new List<byte>();
        internal InvocationWriter(int count) { WriteUInt32(checked((uint)count)); }
        internal void WriteByte(byte value) => bytes.Add(value);
        internal void WriteInt32(int value) => WriteUInt32(unchecked((uint)value));
        internal void WriteUInt32(uint value)
        {
            bytes.Add((byte)value); bytes.Add((byte)(value >> 8)); bytes.Add((byte)(value >> 16)); bytes.Add((byte)(value >> 24));
        }
        internal void WriteUInt64(ulong value)
        {
            WriteUInt32((uint)value); WriteUInt32((uint)(value >> 32));
        }
        internal void WriteString(string value)
        {
            if (value == null) throw new ArgumentNullException(nameof(value));
            var encoded = Encoding.UTF8.GetBytes(value);
            WriteUInt32(checked((uint)encoded.Length));
            bytes.AddRange(encoded);
        }
        internal byte[] Finish() => bytes.ToArray();
    }
    public abstract class RuntimeFunction : IRuntimeValue
    {
        public RuntimeValue RuntimeValue { get; }
        protected RuntimeFunction(RuntimeValue value) { RuntimeValue = value; }
        public string Source => RuntimeValue.ProgramSource;
        internal Response InvokeCore(InvocationWriter writer) => RuntimeValue.Invoke(writer.Finish());
    }
    public sealed class RuntimeFunction<TResult> : RuntimeFunction
    {
        private readonly InvocationCodec<TResult> result;
        public RuntimeFunction(RuntimeValue value, InvocationCodec<TResult> result) : base(value) { this.result = result; }
        public TResult Invoke() => result.ReadResult(RuntimeValue.Owner, InvokeCore(new InvocationWriter(0)));
    }
    public sealed class RuntimeFunction<T1, TResult> : RuntimeFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<TResult> result;
        public RuntimeFunction(RuntimeValue value, InvocationCodec<T1> c1, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.result = result; }
        public TResult Invoke(T1 a1) { var w = new InvocationWriter(1); c1.WriteArgument(w, a1); return result.ReadResult(RuntimeValue.Owner, InvokeCore(w)); }
    }
    public sealed class RuntimeFunction<T1, T2, TResult> : RuntimeFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<TResult> result;
        public RuntimeFunction(RuntimeValue value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2) { var w = new InvocationWriter(2); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); return result.ReadResult(RuntimeValue.Owner, InvokeCore(w)); }
    }
    public sealed class RuntimeFunction<T1, T2, T3, TResult> : RuntimeFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<T3> c3; private readonly InvocationCodec<TResult> result;
        public RuntimeFunction(RuntimeValue value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<T3> c3, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.c3 = c3; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2, T3 a3) { var w = new InvocationWriter(3); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); c3.WriteArgument(w, a3); return result.ReadResult(RuntimeValue.Owner, InvokeCore(w)); }
    }
    public sealed class RuntimeFunction<T1, T2, T3, T4, TResult> : RuntimeFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<T3> c3; private readonly InvocationCodec<T4> c4; private readonly InvocationCodec<TResult> result;
        public RuntimeFunction(RuntimeValue value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<T3> c3, InvocationCodec<T4> c4, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.c3 = c3; this.c4 = c4; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2, T3 a3, T4 a4) { var w = new InvocationWriter(4); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); c3.WriteArgument(w, a3); c4.WriteArgument(w, a4); return result.ReadResult(RuntimeValue.Owner, InvokeCore(w)); }
    }
    public sealed class RuntimeFunction<T1, T2, T3, T4, T5, TResult> : RuntimeFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<T3> c3; private readonly InvocationCodec<T4> c4; private readonly InvocationCodec<T5> c5; private readonly InvocationCodec<TResult> result;
        public RuntimeFunction(RuntimeValue value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<T3> c3, InvocationCodec<T4> c4, InvocationCodec<T5> c5, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.c3 = c3; this.c4 = c4; this.c5 = c5; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2, T3 a3, T4 a4, T5 a5) { var w = new InvocationWriter(5); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); c3.WriteArgument(w, a3); c4.WriteArgument(w, a4); c5.WriteArgument(w, a5); return result.ReadResult(RuntimeValue.Owner, InvokeCore(w)); }
    }
    public sealed class RuntimeFunction<T1, T2, T3, T4, T5, T6, TResult> : RuntimeFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<T3> c3; private readonly InvocationCodec<T4> c4; private readonly InvocationCodec<T5> c5; private readonly InvocationCodec<T6> c6; private readonly InvocationCodec<TResult> result;
        public RuntimeFunction(RuntimeValue value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<T3> c3, InvocationCodec<T4> c4, InvocationCodec<T5> c5, InvocationCodec<T6> c6, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.c3 = c3; this.c4 = c4; this.c5 = c5; this.c6 = c6; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2, T3 a3, T4 a4, T5 a5, T6 a6) { var w = new InvocationWriter(6); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); c3.WriteArgument(w, a3); c4.WriteArgument(w, a4); c5.WriteArgument(w, a5); c6.WriteArgument(w, a6); return result.ReadResult(RuntimeValue.Owner, InvokeCore(w)); }
    }
    public sealed class RuntimeFunction<T1, T2, T3, T4, T5, T6, T7, TResult> : RuntimeFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<T3> c3; private readonly InvocationCodec<T4> c4; private readonly InvocationCodec<T5> c5; private readonly InvocationCodec<T6> c6; private readonly InvocationCodec<T7> c7; private readonly InvocationCodec<TResult> result;
        public RuntimeFunction(RuntimeValue value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<T3> c3, InvocationCodec<T4> c4, InvocationCodec<T5> c5, InvocationCodec<T6> c6, InvocationCodec<T7> c7, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.c3 = c3; this.c4 = c4; this.c5 = c5; this.c6 = c6; this.c7 = c7; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2, T3 a3, T4 a4, T5 a5, T6 a6, T7 a7) { var w = new InvocationWriter(7); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); c3.WriteArgument(w, a3); c4.WriteArgument(w, a4); c5.WriteArgument(w, a5); c6.WriteArgument(w, a6); c7.WriteArgument(w, a7); return result.ReadResult(RuntimeValue.Owner, InvokeCore(w)); }
    }
    public sealed class RuntimeFunction<T1, T2, T3, T4, T5, T6, T7, T8, TResult> : RuntimeFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<T3> c3; private readonly InvocationCodec<T4> c4; private readonly InvocationCodec<T5> c5; private readonly InvocationCodec<T6> c6; private readonly InvocationCodec<T7> c7; private readonly InvocationCodec<T8> c8; private readonly InvocationCodec<TResult> result;
        public RuntimeFunction(RuntimeValue value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<T3> c3, InvocationCodec<T4> c4, InvocationCodec<T5> c5, InvocationCodec<T6> c6, InvocationCodec<T7> c7, InvocationCodec<T8> c8, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.c3 = c3; this.c4 = c4; this.c5 = c5; this.c6 = c6; this.c7 = c7; this.c8 = c8; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2, T3 a3, T4 a4, T5 a5, T6 a6, T7 a7, T8 a8) { var w = new InvocationWriter(8); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); c3.WriteArgument(w, a3); c4.WriteArgument(w, a4); c5.WriteArgument(w, a5); c6.WriteArgument(w, a6); c7.WriteArgument(w, a7); c8.WriteArgument(w, a8); return result.ReadResult(RuntimeValue.Owner, InvokeCore(w)); }
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
        public static InvocationCodec<Unit> UnitInvocation { get; } = new InvocationCodec<Unit>(
            _ => default, (writer, _) => writer.WriteByte(0), (_, response) => response.Tag == 0 ? default : throw TypeMismatch());
        public static InvocationCodec<int> IntInvocation { get; } = new InvocationCodec<int>(
            Int, (writer, value) => { writer.WriteByte(2); writer.WriteInt32(value); }, (_, response) => response.Tag == 2 ? checked((int)response.Integer) : throw TypeMismatch());
        public static InvocationCodec<float> FloatInvocation { get; } = new InvocationCodec<float>(
            Float, (writer, value) => { writer.WriteByte(3); writer.WriteUInt32(unchecked((uint)BitConverter.SingleToInt32Bits(value))); }, (_, response) => response.Tag == 3 ? (float)response.Number : throw TypeMismatch());
        public static InvocationCodec<bool> BoolInvocation { get; } = new InvocationCodec<bool>(
            Bool, (writer, value) => { writer.WriteByte(1); writer.WriteByte(value ? (byte)1 : (byte)0); }, (_, response) => response.Tag == 1 ? response.Integer != 0 : throw TypeMismatch());
        public static InvocationCodec<string> StringInvocation { get; } = new InvocationCodec<string>(
            String, (writer, value) => { writer.WriteByte(4); writer.WriteString(value); }, (_, response) => response.Tag == 4 ? Encoding.UTF8.GetString(Native.ReadBuffer(response)) : throw TypeMismatch());
        public static InvocationCodec<T> EnumInvocation<T>(string typeName, Func<uint, T> read, Func<T, uint> write) => new InvocationCodec<T>(
            value => read(value.Enum), (writer, value) => { writer.WriteByte(5); writer.WriteString(typeName); writer.WriteUInt32(write(value)); },
            (_, response) => response.Tag == 5 && Encoding.UTF8.GetString(Native.ReadBuffer(response)) == typeName ? read(checked((uint)response.Integer)) : throw TypeMismatch());
        public static InvocationCodec<T> RuntimeInvocation<T>(Func<RuntimeValue, T> read) where T : IRuntimeValue => new InvocationCodec<T>(
            read, (writer, value) => WriteRuntime(writer, value), (runtime, response) => read(ReadRuntime(runtime, response)));
        public static InvocationCodec<T?> OptionalValueInvocation<T>(InvocationCodec<T> inner) where T : struct => new InvocationCodec<T?>(
            value => value.IsNone ? (T?)null : inner.ReadValue(value),
            (writer, value) => { if (value.HasValue) inner.WriteArgument(writer, value.Value); else writer.WriteByte(0); },
            (runtime, response) => response.Tag == 0 ? (T?)null : inner.ReadResult(runtime, response));
        public static InvocationCodec<T?> OptionalReferenceInvocation<T>(InvocationCodec<T> inner) where T : class => new InvocationCodec<T?>(
            value => value.IsNone ? null : inner.ReadValue(value),
            (writer, value) => { if (value == null) writer.WriteByte(0); else inner.WriteArgument(writer, value); },
            (runtime, response) => response.Tag == 0 ? null : inner.ReadResult(runtime, response));
        private static void WriteRuntime<T>(InvocationWriter writer, T value) where T : IRuntimeValue
        {
            if (value == null) throw new ArgumentNullException(nameof(value));
            var runtimeValue = value.RuntimeValue;
            runtimeValue.Request(NativeOperation.InspectValue);
            writer.WriteByte(11); writer.WriteUInt64(runtimeValue.Owner.Handle.Id); writer.WriteUInt64(runtimeValue.Id);
        }
        private static RuntimeValue ReadRuntime(Runtime runtime, Response response)
        {
            if (response.Tag != 11 || response.Handle != runtime.Handle.Id || response.Length == 0) throw TypeMismatch();
            return new RuntimeValue(runtime, response.Length);
        }
        private static CoflowException TypeMismatch() => new CoflowException("Function value type mismatch.");
    }
}
