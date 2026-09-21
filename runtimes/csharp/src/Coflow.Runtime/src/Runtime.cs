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
        internal abstract void Initialize(object value);
        protected TypeBinding(Type type, string name) { ManagedType = type; Name = name; }
    }
    public sealed class TypeBinding<T> : TypeBinding
    {
        internal Func<Projection, T> Read { get; }
        internal override object Materialize(Projection value) => Read(value)!;
        private readonly Action<T>? initialize;
        internal override void Initialize(object value) => initialize?.Invoke((T)value);
        public TypeBinding(string name, Func<Projection, T> read, Action<T>? initialize = null) : base(typeof(T), name) { Read = read; this.initialize = initialize; }
    }
    public sealed class Contract : IDisposable
    {
        internal NativeHandle Handle { get; }
        private readonly byte[] identity;
        private readonly Dictionary<Type, TypeBinding> bindings = new Dictionary<Type, TypeBinding>();
        private readonly Dictionary<string, TypeBinding> namedBindings = new Dictionary<string, TypeBinding>(StringComparer.Ordinal);
        internal TypeBinding Binding(string name) => namedBindings.TryGetValue(name, out var binding) ? binding : throw new CoflowException("Snapshot type is not registered in this contract.");
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
    public sealed class Runtime : IDisposable
    {
        private HostBinding[] hosts;
        private static readonly object RegistryLock = new object();
        private static readonly Dictionary<ulong, WeakReference<Runtime>> Registry = new Dictionary<ulong, WeakReference<Runtime>>();
        internal NativeHandle Handle { get; }
        internal Contract Contract { get; }
        internal ProjectImage Image { get; }
        private readonly int executionThread = Environment.CurrentManagedThreadId;
        internal void RequireExecution()
        {
            if (Handle.IsClosed) throw new ObjectDisposedException(nameof(Runtime));
            if (Environment.CurrentManagedThreadId != executionThread) throw new CoflowException("Runtime must execute on its creating thread.");
        }
        internal Response Execute(NativeOperation operation, string key = "", byte[]? data = null, ulong index = 0, ulong value = 0)
        {
            RequireExecution();
            try { return Native.Call(operation, Handle, key, data, index, value); }
            finally { GC.KeepAlive(hosts); GC.KeepAlive(this); }
        }
        internal Runtime(ulong handle, Contract contract, HostBinding[] hosts)
        {
            Handle = new NativeHandle(handle); Contract = contract; this.hosts = hosts;
            try { Image = ProjectImage.Read(Native.ReadBuffer(Native.Call(NativeOperation.ProjectSnapshot, Handle))); Image.Materialize(this); }
            catch { Handle.Dispose(); throw; }
            lock (RegistryLock) Registry.Add(handle, new WeakReference<Runtime>(this));
        }
        ~Runtime()
        {
            // 终结线程只移除弱注册项；原生资源仍由 SafeHandle 的创建线程队列释放。
            if (Handle != null) { lock (RegistryLock) Registry.Remove(Handle.Id); }
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
            if (!Image.Tables.ContainsKey(binding.Name)) throw new CoflowException("Unknown table.");
            return new Table<T>(this, binding);
        }
        public T Get<T>()
        {
            var binding = Contract.Binding<T>();
            return binding.Read(new Projection(this, Image.Singletons.TryGetValue(binding.Name, out var id) ? id : throw new CoflowException("Unknown singleton.")));
        }
        public CheckResult RunChecks(CheckOptions? options = null) => CheckResult.Run(this, options ?? CheckOptions.Default);
        public void Dispose()
        {
            if (Handle.IsClosed) return;
            RequireExecution();
            // 原生层在活动调用和 Host 重入期间拒绝释放；失败时必须保留完整托管状态。
            if (!Handle.DisposeExplicit()) throw new InvalidOperationException("Runtime is busy and cannot be disposed.");
            lock (RegistryLock) Registry.Remove(Handle.Id);
            hosts = Array.Empty<HostBinding>();
            GC.SuppressFinalize(this);
        }
    }
    public static class RuntimeThread
    {
        public static ulong DrainFinalizers() => Native.ThreadDrain();
        public static void Shutdown()
        {
            if (Native.ThreadShutdown() != 0)
                throw new InvalidOperationException("The Coflow runtime thread is busy and cannot be shut down.");
        }
    }
    public enum ValueKind : uint { None, Bool, Int, Float, String, Enum, Object, Array, Dictionary, Function, Template, Dimension }
    [EditorBrowsable(EditorBrowsableState.Never)]
    public interface IRuntimeArgument { void Encode(ArgumentWriter writer); }

    // 值 ID 只在所属 Runtime 内有效，不分配独立原生句柄，也不要求单独释放。
    [EditorBrowsable(EditorBrowsableState.Never)]
    public readonly struct Projection : IEquatable<Projection>, IRuntimeArgument
    {
        internal Runtime Owner { get; }
        internal ulong Id { get; }
        private readonly ProjectImage snapshot;
        private readonly ProjectImage.Node? detached;
        internal Projection(Runtime owner, ulong id, bool adopt = false)
        {
            Owner = owner; Id = id; detached = null;
            if (id == 0 || owner.Image.Contains(id)) { snapshot = owner.Image; return; }
            // 先取得拥有型根，投影或解码失败也必须释放；纯内容图投影后立即解除保活。
            NativeHandle? lease = new NativeHandle(Native.Call(NativeOperation.CreateValueLease, owner.Handle, value: id, index: adopt ? 1UL : 0UL).Handle);
            try {
                snapshot = ProjectImage.Read(Native.ReadBuffer(Native.Call(NativeOperation.ProjectSnapshot, owner.Handle, value: id)));
                snapshot.Materialize(owner);
                if (snapshot.NeedsLease(owner.Image)) { snapshot.Lease = lease; lease = null; }
            }
            finally { lease?.Dispose(); }
        }
        internal Projection(Runtime owner, ulong id, ProjectImage snapshot) { Owner = owner; Id = id; this.snapshot = snapshot; detached = null; }
        private Projection(ProjectImage.Node node) { Owner = null!; Id = 0; snapshot = null!; detached = node; }
        private ProjectImage.Node Node => detached ?? (snapshot ?? throw new CoflowException("Uninitialized runtime value.")).Get(Id);
        public T ReadProjected<T>(Func<Projection, T> read)
        {
            if (Node.Kind == 12) {
                if (snapshot != null && snapshot.IsMaterializing) return default!;
                // 一个公开 getter 只读取一次 Host，再对具体结果做 None/类型/字段转换。
                return Canonical().ReadProjected(read);
            }
            if (Node.Views != null && Node.Views.TryGetValue(typeof(T), out var value)) return (T)value!;
            var result = read(this);
            if (snapshot != null && snapshot.IsMaterializing) {
                if (Node.Views == null) Node.Views = new Dictionary<Type, object?>();
                Node.Views.Add(typeof(T), result);
            }
            return result;
        }
        public bool TryGetProjection<T>(out T value)
        {
            if (Node.Projection is T projection) { value = projection; return true; }
            value = default!; return false;
        }
        public static Projection From(object? value)
        {
            switch (value)
            {
                case null: return new Projection(new ProjectImage.Node { Kind = 0 });
                case bool v: return new Projection(new ProjectImage.Node { Kind = 1, Bits = v ? 1U : 0U });
                case int v: return new Projection(new ProjectImage.Node { Kind = 2, Bits = unchecked((uint)v) });
                case float v: return new Projection(new ProjectImage.Node { Kind = 3, Bits = unchecked((uint)BitConverter.SingleToInt32Bits(v)) });
                case string v: return new Projection(new ProjectImage.Node { Kind = 4, Text = v });
                case IRuntimeArgument v: return ArgumentWriter.Capture(v);
                default: throw new CoflowException("Unsupported managed value.");
            }
        }
        public static Projection EnumValue(string type, uint value) => new Projection(new ProjectImage.Node { Kind = 5, Type = type, Bits = value });
        public static Projection Data(byte[] identity, string type, string[] names, Projection[] values)
        {
            if (names.Length != values.Length) throw new ArgumentException("Field count mismatch.");
            var fields = new Dictionary<string, Projection>(StringComparer.Ordinal);
            for (int i = 0; i < names.Length; ++i) fields.Add(names[i], values[i]);
            return new Projection(new ProjectImage.Node { Kind = 6, Type = type, Identity = (byte[])identity.Clone(), Members = fields });
        }
        internal static Projection Collection(byte kind, Projection[] values)
        {
            var node = new ProjectImage.Node { Kind = kind, Children = (Projection[])values.Clone() };
            if (kind == 8) {
                node.Lookup = new Dictionary<SnapshotKey, int>();
                for (int i = 0; i < values.Length; i += 2) node.Lookup.Add(values[i].Node.ScalarKey, i / 2);
            }
            return new Projection(node);
        }
        private Projection Child(ulong id) => new Projection(Owner, id, snapshot);
        void IRuntimeArgument.Encode(ArgumentWriter writer) => writer.Write(this);
        internal Response Request(NativeOperation op, string key = "", byte[]? data = null, ulong index = 0)
        {
            if (Owner == null || Id == 0) throw new CoflowException("Invalid value.");
            return Owner.Execute(op, key, data, index, Id);
        }
        private Projection Child(Response result) => new Projection(Owner, result.Handle);
        public ValueKind Kind => Node.Kind == 12 ? (ValueKind)Request(NativeOperation.InspectValue).Tag : (ValueKind)Node.Kind;
        public int Count => Node.Children != null ? Node.Children.Length / (Node.Kind == 8 ? 2 : 1) : Node.Members != null ? Node.Members.Count : Node.Kind == 12 ? checked((int)Request(NativeOperation.InspectValue).Length) : Node.Kind == 8 ? Node.Items.Length / 2 : Node.Kind == 7 ? Node.Items.Length : Node.Fields.Count;
        public bool IsNone => Kind == ValueKind.None;
        public bool Bool { get { var node = Node; if (node.Kind == 12) { var value = Request(NativeOperation.InspectValue); Require(value.Tag, 1); return value.Integer != 0; } Require(node.Kind, 1); return node.Bits != 0; } }
        public int Int { get { var node = Node; if (node.Kind == 12) { var value = Request(NativeOperation.InspectValue); Require(value.Tag, 2); return checked((int)value.Integer); } Require(node.Kind, 2); return unchecked((int)node.Bits); } }
        public float Float { get { var node = Node; if (node.Kind == 12) { var value = Request(NativeOperation.InspectValue); Require(value.Tag, 3); return (float)value.Number; } Require(node.Kind, 3); return BitConverter.Int32BitsToSingle(unchecked((int)node.Bits)); } }
        public uint Enum { get { var node = Node; if (node.Kind == 12) { var value = Request(NativeOperation.InspectValue); Require(value.Tag, 5); return checked((uint)value.Integer); } Require(node.Kind, 5); return node.Bits; } }
        public string Text => Node.Kind == 4 ? Node.Text : Encoding.UTF8.GetString(Native.ReadBuffer(Request(NativeOperation.ReadText)));
        public string TypeName => Node.Kind == 12 ? Encoding.UTF8.GetString(Native.ReadBuffer(Request(NativeOperation.TypeName))) : Node.Type;
        public string ProgramSource => Node.Kind == 9 || Node.Kind == 10 ? Node.Text : throw new CoflowException("Expected execution capability.");
        public Projection Field(string name) => Node.Members != null ? Node.Members[name] : Node.Kind == 12 ? Child(Request(NativeOperation.ReadField, name)) : Child(Node.Fields.TryGetValue(name, out var id) ? id : throw new CoflowException("Unknown field."));
        public Projection Canonical() => Node.Kind == 12 ? new Projection(Owner, Request(NativeOperation.CanonicalValue).Handle, adopt: true) : this;
        public Projection DimensionDefault() { Require((uint)Kind, 11); return Child(Node.Default); }
        public Projection DimensionValue(string variant) { Require((uint)Kind, 11); return Child(Node.Fields.TryGetValue(variant, out var id) ? id : Node.Default); }
        public bool HasDimensionVariant(string variant) { Require((uint)Kind, 11); return Node.Explicit.ContainsKey(variant); }
        public string DimensionVariantAt(int index)
        {
            Require((uint)Kind, 11);
            if (index < 0 || index >= Node.Fields.Count) throw new ArgumentOutOfRangeException(nameof(index));
            foreach (var name in Node.Fields.Keys) if (index-- == 0) return name;
            throw new ArgumentOutOfRangeException(nameof(index));
        }
        public Projection At(int index) { Require((uint)Kind, 7); return Element(index); }
        public Projection KeyAt(int index) { Require((uint)Kind, 8); return Element(checked(index * 2)); }
        public Projection ValueAt(int index) { Require((uint)Kind, 8); return Element(checked(index * 2 + 1)); }
        private Projection Element(int index)
        {
            if (Node.Children != null) { if (index < 0 || index >= Node.Children.Length) throw new ArgumentOutOfRangeException(nameof(index)); return Node.Children[index]; }
            if (index < 0 || index >= Node.Items.Length) throw new ArgumentOutOfRangeException(nameof(index));
            return Child(Node.Items[index]);
        }

        internal void Write(ArgumentWriter writer, int depth = 0)
        {
            if (depth >= 128) throw new CoflowException("Managed value nesting limit exceeded.");
            var node = Node;
            if (node.Kind == 6 && node.Key != null || node.Kind == 9 || node.Kind == 10 || node.Kind == 12) {
                Request(NativeOperation.InspectValue);
                writer.WriteByte(11); writer.WriteUInt64(Owner.Handle.Id); writer.WriteUInt64(Id); return;
            }
            writer.WriteByte(node.Kind);
            switch (node.Kind) {
                case 0: break;
                case 1: writer.WriteByte((byte)node.Bits); break;
                case 2: case 3: writer.WriteUInt32(node.Bits); break;
                case 4: writer.WriteString(node.Text); break;
                case 5: writer.WriteString(node.Type); writer.WriteUInt32(node.Bits); break;
                case 6:
                    writer.WriteString(node.Type); writer.WriteUInt32((uint)Count);
                    if (node.Members != null) { foreach (var field in node.Members) { writer.WriteString(field.Key); writer.WriteUInt32(1); field.Value.Write(writer, depth + 1); } }
                    else { foreach (var field in node.Fields) { writer.WriteString(field.Key); writer.WriteUInt32(1); Child(field.Value).Write(writer, depth + 1); } }
                    break;
                case 7:
                    writer.WriteUInt32((uint)Count); for (int i = 0; i < Count; ++i) At(i).Write(writer, depth + 1); break;
                case 8:
                    writer.WriteUInt32((uint)Count); for (int i = 0; i < Count; ++i) { writer.WriteUInt32(2); KeyAt(i).Write(writer, depth + 1); ValueAt(i).Write(writer, depth + 1); } break;
                default: throw new CoflowException("This value cannot be imported.");
            }
        }
        public void RequireContract(byte[] identity)
        {
            if (detached != null && Node.Identity != null) {
                if (Node.Identity.Length != identity.Length) throw new CoflowException("Generated types do not match data.");
                for (int i = 0; i < identity.Length; ++i) if (Node.Identity[i] != identity[i]) throw new CoflowException("Generated types do not match data.");
                return;
            }
            if (Owner == null || !Owner.Contract.Matches(identity)) throw new CoflowException("Generated types do not match the runtime contract.");
        }
        public bool ValueEquals(Projection other)
        {
            var pending = new Stack<(Projection, Projection)>();
            pending.Push((this, other));
            while (pending.Count != 0) {
                var pair = pending.Pop(); var left = pair.Item1; var right = pair.Item2;
                var a = left.Node; var b = right.Node;
                if (a.Kind == 10 || b.Kind == 10) {
                    if ((a.Kind != 4 && a.Kind != 10) || (b.Kind != 4 && b.Kind != 10) || left.Text != right.Text) return false;
                    continue;
                }
                if (a.Kind != b.Kind) return false;
                if (a.Kind == 6 && (a.Key != null || b.Key != null) || a.Kind == 9) { if (!left.Equals(right)) return false; continue; }
                switch (a.Kind) {
                    case 0: break;
                    case 1: case 2: if (a.Bits != b.Bits) return false; break;
                    case 3: if (left.Float != right.Float) return false; break;
                    case 4: if (a.Text != b.Text) return false; break;
                    case 5: if (a.Type != b.Type || a.Bits != b.Bits) return false; break;
                    case 6:
                        if (a.Type != b.Type || left.Count != right.Count) return false;
                        // 栈按逆序压入，模板比较仍按声明顺序执行。
                        var names = new List<string>(a.Members != null ? (IEnumerable<string>)a.Members.Keys : a.Fields.Keys);
                        for (int i = names.Count - 1; i >= 0; --i) pending.Push((left.Field(names[i]), right.Field(names[i])));
                        break;
                    case 7:
                        if (left.Count != right.Count) return false;
                        for (int i = left.Count - 1; i >= 0; --i) pending.Push((left.At(i), right.At(i))); break;
                    case 8:
                        if (left.Count != right.Count) return false;
                        for (int i = left.Count - 1; i >= 0; --i) {
                            if (!b.Lookup!.TryGetValue(left.KeyAt(i).Node.ScalarKey, out var index)) return false;
                            pending.Push((left.ValueAt(i), right.ValueAt(index)));
                        }
                        break;
                    default: throw new CoflowException("Value does not support managed content comparison.");
                }
            }
            return true;
        }
        public bool Equals(Projection other) => detached != null || other.detached != null ? ReferenceEquals(detached, other.detached) : ReferenceEquals(Owner, other.Owner) && Id == other.Id;
        public override bool Equals(object? other) => other is Projection value && Equals(value);
        public override int GetHashCode() => detached != null ? System.Runtime.CompilerServices.RuntimeHelpers.GetHashCode(detached) : unchecked((Owner?.GetHashCode() ?? 0) * 397 ^ Id.GetHashCode());
        private static void Require(uint actual, uint expected) { if (actual != expected) throw new CoflowException("Value type mismatch."); }
    }
    public abstract class RuntimeObject : IRuntimeArgument
    {
        protected Projection Value { get; }
        protected RuntimeObject(Projection value) { Value = value; }
        void IRuntimeArgument.Encode(ArgumentWriter writer) => writer.Write(Value);
        internal Projection ArgumentProjection => Value;
        public string ActualType => Value.TypeName;
        protected T Read<T>(string field, Func<Projection, T> read) => Value.Field(field).ReadProjected(read);
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
        internal Func<Projection, T> ReadValue { get; }
        internal Action<ArgumentWriter, T> WriteArgument { get; }
        internal Func<Runtime, Response, T> ReadResult { get; }
        internal InvocationCodec(Func<Projection, T> readValue, Action<ArgumentWriter, T> writeArgument,
            Func<Runtime, Response, T> readResult)
        {
            ReadValue = readValue;
            WriteArgument = writeArgument;
            ReadResult = readResult;
        }
    }
    [EditorBrowsable(EditorBrowsableState.Never)]
    public sealed class ArgumentWriter
    {
        private readonly List<byte> bytes = new List<byte>();
        private Projection? captured;
        internal ArgumentWriter(int count) { WriteUInt32(checked((uint)count)); }
        private ArgumentWriter() { }
        public void Write(Projection value)
        {
            if (captured.HasValue) throw new CoflowException("An argument can only be encoded once.");
            if (bytes.Count == 0) { captured = value; return; }
            value.Write(this);
        }
        internal static Projection Capture(IRuntimeArgument value)
        {
            var writer = new ArgumentWriter();
            value.Encode(writer);
            return writer.captured ?? throw new CoflowException("Argument did not encode a value.");
        }
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
    public sealed class RuntimeTemplate : IRuntimeArgument
    {
        private readonly Projection value;
        void IRuntimeArgument.Encode(ArgumentWriter writer) => writer.Write(value);
        public RuntimeTemplate(Projection value) { this.value = value; }
        public RuntimeTemplate(string text) { value = Projection.From(text ?? throw new ArgumentNullException(nameof(text))); }
        public string Source => value.ProgramSource;
        public string Render() { value.Owner?.RequireExecution(); return value.Text; }
    }
    // 函数只持有执行目标；字段投影不会承担调用职责。
    internal sealed class ExecutionTarget
    {
        internal Projection Projection { get; }
        internal Runtime Owner => Projection.Owner;
        internal ExecutionTarget(Projection projection) { Projection = projection; }
        internal string Source => Projection.ProgramSource;
        internal Response Invoke(byte[] arguments) => Projection.Request(NativeOperation.Invoke, data: arguments);
    }
    public abstract class RuntimeFunction : IRuntimeArgument
    {
        private readonly ExecutionTarget target;
        void IRuntimeArgument.Encode(ArgumentWriter writer) => writer.Write(target.Projection);
        protected Runtime Owner => target.Owner;
        protected RuntimeFunction(Projection value) { target = new ExecutionTarget(value); }
        public string Source => target.Source;
        internal Response InvokeCore(ArgumentWriter writer) => target.Invoke(writer.Finish());
    }
    public sealed class RuntimeFunction<TResult> : RuntimeFunction
    {
        private readonly InvocationCodec<TResult> result;
        public RuntimeFunction(Projection value, InvocationCodec<TResult> result) : base(value) { this.result = result; }
        public TResult Invoke() => result.ReadResult(Owner, InvokeCore(new ArgumentWriter(0)));
    }
    public sealed class RuntimeFunction<T1, TResult> : RuntimeFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<TResult> result;
        public RuntimeFunction(Projection value, InvocationCodec<T1> c1, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.result = result; }
        public TResult Invoke(T1 a1) { var w = new ArgumentWriter(1); c1.WriteArgument(w, a1); return result.ReadResult(Owner, InvokeCore(w)); }
    }
    public sealed class RuntimeFunction<T1, T2, TResult> : RuntimeFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<TResult> result;
        public RuntimeFunction(Projection value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2) { var w = new ArgumentWriter(2); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); return result.ReadResult(Owner, InvokeCore(w)); }
    }
    public sealed class RuntimeFunction<T1, T2, T3, TResult> : RuntimeFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<T3> c3; private readonly InvocationCodec<TResult> result;
        public RuntimeFunction(Projection value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<T3> c3, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.c3 = c3; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2, T3 a3) { var w = new ArgumentWriter(3); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); c3.WriteArgument(w, a3); return result.ReadResult(Owner, InvokeCore(w)); }
    }
    public sealed class RuntimeFunction<T1, T2, T3, T4, TResult> : RuntimeFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<T3> c3; private readonly InvocationCodec<T4> c4; private readonly InvocationCodec<TResult> result;
        public RuntimeFunction(Projection value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<T3> c3, InvocationCodec<T4> c4, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.c3 = c3; this.c4 = c4; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2, T3 a3, T4 a4) { var w = new ArgumentWriter(4); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); c3.WriteArgument(w, a3); c4.WriteArgument(w, a4); return result.ReadResult(Owner, InvokeCore(w)); }
    }
    public sealed class RuntimeFunction<T1, T2, T3, T4, T5, TResult> : RuntimeFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<T3> c3; private readonly InvocationCodec<T4> c4; private readonly InvocationCodec<T5> c5; private readonly InvocationCodec<TResult> result;
        public RuntimeFunction(Projection value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<T3> c3, InvocationCodec<T4> c4, InvocationCodec<T5> c5, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.c3 = c3; this.c4 = c4; this.c5 = c5; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2, T3 a3, T4 a4, T5 a5) { var w = new ArgumentWriter(5); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); c3.WriteArgument(w, a3); c4.WriteArgument(w, a4); c5.WriteArgument(w, a5); return result.ReadResult(Owner, InvokeCore(w)); }
    }
    public sealed class RuntimeFunction<T1, T2, T3, T4, T5, T6, TResult> : RuntimeFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<T3> c3; private readonly InvocationCodec<T4> c4; private readonly InvocationCodec<T5> c5; private readonly InvocationCodec<T6> c6; private readonly InvocationCodec<TResult> result;
        public RuntimeFunction(Projection value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<T3> c3, InvocationCodec<T4> c4, InvocationCodec<T5> c5, InvocationCodec<T6> c6, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.c3 = c3; this.c4 = c4; this.c5 = c5; this.c6 = c6; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2, T3 a3, T4 a4, T5 a5, T6 a6) { var w = new ArgumentWriter(6); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); c3.WriteArgument(w, a3); c4.WriteArgument(w, a4); c5.WriteArgument(w, a5); c6.WriteArgument(w, a6); return result.ReadResult(Owner, InvokeCore(w)); }
    }
    public sealed class RuntimeFunction<T1, T2, T3, T4, T5, T6, T7, TResult> : RuntimeFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<T3> c3; private readonly InvocationCodec<T4> c4; private readonly InvocationCodec<T5> c5; private readonly InvocationCodec<T6> c6; private readonly InvocationCodec<T7> c7; private readonly InvocationCodec<TResult> result;
        public RuntimeFunction(Projection value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<T3> c3, InvocationCodec<T4> c4, InvocationCodec<T5> c5, InvocationCodec<T6> c6, InvocationCodec<T7> c7, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.c3 = c3; this.c4 = c4; this.c5 = c5; this.c6 = c6; this.c7 = c7; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2, T3 a3, T4 a4, T5 a5, T6 a6, T7 a7) { var w = new ArgumentWriter(7); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); c3.WriteArgument(w, a3); c4.WriteArgument(w, a4); c5.WriteArgument(w, a5); c6.WriteArgument(w, a6); c7.WriteArgument(w, a7); return result.ReadResult(Owner, InvokeCore(w)); }
    }
    public sealed class RuntimeFunction<T1, T2, T3, T4, T5, T6, T7, T8, TResult> : RuntimeFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<T3> c3; private readonly InvocationCodec<T4> c4; private readonly InvocationCodec<T5> c5; private readonly InvocationCodec<T6> c6; private readonly InvocationCodec<T7> c7; private readonly InvocationCodec<T8> c8; private readonly InvocationCodec<TResult> result;
        public RuntimeFunction(Projection value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<T3> c3, InvocationCodec<T4> c4, InvocationCodec<T5> c5, InvocationCodec<T6> c6, InvocationCodec<T7> c7, InvocationCodec<T8> c8, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.c3 = c3; this.c4 = c4; this.c5 = c5; this.c6 = c6; this.c7 = c7; this.c8 = c8; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2, T3 a3, T4 a4, T5 a5, T6 a6, T7 a7, T8 a8) { var w = new ArgumentWriter(8); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); c3.WriteArgument(w, a3); c4.WriteArgument(w, a4); c5.WriteArgument(w, a5); c6.WriteArgument(w, a6); c7.WriteArgument(w, a7); c8.WriteArgument(w, a8); return result.ReadResult(Owner, InvokeCore(w)); }
    }
    public static class ValueCodecs
    {
        public static T? OptionalValue<T>(Projection value, Func<Projection, T> read) where T : struct => value.IsNone ? (T?)null : read(value);
        public static T? OptionalReference<T>(Projection value, Func<Projection, T> read) where T : class => value.IsNone ? null : read(value);
        public static int Int(Projection value) => value.Int;
        public static float Float(Projection value) => value.Float;
        public static bool Bool(Projection value) => value.Bool;
        public static string String(Projection value) => value.Text;
        public static uint Enum(Projection value) => value.Enum;
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
        // fstring 返回值既可以是纯文本，也可以是带执行环境的原模板。
        public static InvocationCodec<RuntimeTemplate> TemplateInvocation { get; } = new InvocationCodec<RuntimeTemplate>(
            value => new RuntimeTemplate(value), (writer, value) => WriteRuntime(writer, value),
            (runtime, response) => response.Tag == 4
                ? new RuntimeTemplate(Encoding.UTF8.GetString(Native.ReadBuffer(response)))
                : new RuntimeTemplate(ReadRuntime(runtime, response)));
        public static InvocationCodec<T> EnumInvocation<T>(string typeName, Func<uint, T> read, Func<T, uint> write) => new InvocationCodec<T>(
            value => read(value.Enum), (writer, value) => { writer.WriteByte(5); writer.WriteString(typeName); writer.WriteUInt32(write(value)); },
            (_, response) => response.Tag == 5 && Encoding.UTF8.GetString(Native.ReadBuffer(response)) == typeName ? read(checked((uint)response.Integer)) : throw TypeMismatch());
        public static InvocationCodec<T> RuntimeInvocation<T>(Func<Projection, T> read) where T : IRuntimeArgument => new InvocationCodec<T>(
            read, (writer, value) => WriteRuntime(writer, value), (runtime, response) => read(ReadRuntime(runtime, response)));
        public static InvocationCodec<T?> OptionalValueInvocation<T>(InvocationCodec<T> inner) where T : struct => new InvocationCodec<T?>(
            value => value.IsNone ? (T?)null : inner.ReadValue(value),
            (writer, value) => { if (value.HasValue) inner.WriteArgument(writer, value.Value); else writer.WriteByte(0); },
            (runtime, response) => response.Tag == 0 ? (T?)null : inner.ReadResult(runtime, response));
        public static InvocationCodec<T?> OptionalReferenceInvocation<T>(InvocationCodec<T> inner) where T : class => new InvocationCodec<T?>(
            value => value.IsNone ? null : inner.ReadValue(value),
            (writer, value) => { if (value == null) writer.WriteByte(0); else inner.WriteArgument(writer, value); },
            (runtime, response) => response.Tag == 0 ? null : inner.ReadResult(runtime, response));
        private static void WriteRuntime<T>(ArgumentWriter writer, T value) where T : IRuntimeArgument
        {
            if (value == null) throw new ArgumentNullException(nameof(value));
            value.Encode(writer);
        }
        private static Projection ReadRuntime(Runtime runtime, Response response)
        {
            if (response.Tag != 11 || response.Handle != runtime.Handle.Id || response.Length == 0) throw TypeMismatch();
            return new Projection(runtime, response.Length, adopt: true);
        }
        private static CoflowException TypeMismatch() => new CoflowException("Function value type mismatch.");
    }
}
