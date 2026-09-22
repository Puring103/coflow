using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Text;

namespace Coflow
{
    public enum ValueKind : uint { None, Bool, Int, Float, String, Enum, Object, Array, Dictionary, Function, Template, Dimension }
    [EditorBrowsable(EditorBrowsableState.Never)]
    public interface ICoflowValue { void Encode(ArgumentWriter writer); }

    // 值 ID 只在所属 Runtime 内有效，不分配独立原生句柄，也不要求单独释放。
    [EditorBrowsable(EditorBrowsableState.Never)]
    public readonly struct Projection : IEquatable<Projection>, ICoflowValue
    {
        internal Runtime Owner { get; }
        internal ulong Id { get; }
        private readonly ValueImage snapshot;
        private readonly ValueImage.Node? detached;
        internal Projection(Runtime owner, ulong id, bool adopt = false)
        {
            Owner = owner; Id = id; detached = null;
            // 先取得拥有型根，投影或解码失败也必须释放；纯内容图投影后立即解除保活。
            NativeHandle? lease = new NativeHandle(Native.Call(NativeOperation.CreateValueLease, owner.Handle, value: id, index: adopt ? 1UL : 0UL).Handle);
            try {
                snapshot = ValueImage.Read(Native.ReadBuffer(Native.Call(NativeOperation.ReadDynamicValue, owner.Handle, value: id)));
                if (snapshot.NeedsLease()) { owner.RetainProjection(snapshot, lease); lease = null; }
            }
            finally { lease?.Dispose(); }
        }
        internal Projection(Runtime owner, ulong id, ValueImage snapshot) { Owner = owner; Id = id; this.snapshot = snapshot; detached = null; }
        private Projection(ValueImage.Node node) { Owner = null!; Id = 0; snapshot = null!; detached = node; }
        private ValueImage.Node Node => detached ?? (snapshot ?? throw new CoflowException("Uninitialized runtime value.")).Get(Id);
        public T ReadProjected<T>(Func<Projection, T> read)
        {
            if (Node.Kind == 12) {
                // 一个公开 getter 只读取一次 Host，再对具体结果做 None/类型/字段转换。
                return Canonical().ReadProjected(read);
            }
            return read(this);
        }
        [EditorBrowsable(EditorBrowsableState.Never)]
        public T Resolve<T>()
        {
            var value = Canonical();
            if (value.Owner == null || (value.Node.Kind != 6 && value.Node.Kind != 13))
                throw new CoflowException("Value is not a runtime record.");
            return value.Node.Kind == 6
                ? value.Owner.ResolveEmbedded<T>(value.Id, value.snapshot)
                : value.Owner.ResolveRecord<T>(value.Id);
        }
        internal void Publish(object value)
        {
            // C# 主动构造的 data 没有 Runtime 身份；只有 Rust 记录参与对象缓存和循环引用发布。
            if (Owner != null) Owner.PublishRecord(Id, value, snapshot);
        }
        public static Projection From(object? value)
        {
            switch (value)
            {
                case null: return new Projection(new ValueImage.Node { Kind = 0 });
                case bool v: return new Projection(new ValueImage.Node { Kind = 1, Bits = v ? 1U : 0U });
                case int v: return new Projection(new ValueImage.Node { Kind = 2, Bits = unchecked((uint)v) });
                case float v: return new Projection(new ValueImage.Node { Kind = 3, Bits = unchecked((uint)BitConverter.SingleToInt32Bits(v)) });
                case string v: return new Projection(new ValueImage.Node { Kind = 4, Text = v });
                case ICoflowValue v: return ArgumentWriter.Capture(v);
                default: throw new CoflowException("Unsupported managed value.");
            }
        }
        public static Projection EnumValue(string type, uint value) => new Projection(new ValueImage.Node { Kind = 5, Type = type, Bits = value });
        public static Projection Data(byte[] identity, string type, string[] names, Projection[] values)
        {
            if (names.Length != values.Length) throw new ArgumentException("Field count mismatch.");
            var fields = new Dictionary<string, Projection>(StringComparer.Ordinal);
            for (int i = 0; i < names.Length; ++i) fields.Add(names[i], values[i]);
            return new Projection(new ValueImage.Node { Kind = 6, Type = type, Identity = (byte[])identity.Clone(), Members = fields });
        }
        internal static Projection Collection(byte kind, Projection[] values)
        {
            var node = new ValueImage.Node { Kind = kind, Children = (Projection[])values.Clone() };
            if (kind == 8) {
                node.Lookup = new Dictionary<ValueKey, int>();
                for (int i = 0; i < values.Length; i += 2) node.Lookup.Add(values[i].Node.ScalarKey, i / 2);
            }
            return new Projection(node);
        }
        private Projection Child(ulong id) => new Projection(Owner, id, snapshot);
        void ICoflowValue.Encode(ArgumentWriter writer) => writer.Write(this);
        internal Response Request(NativeOperation op, string key = "", byte[]? data = null, ulong index = 0)
        {
            if (Owner == null || Id == 0) throw new CoflowException("Invalid value.");
            try { return Owner.Execute(op, key, data, index, Id); }
            finally { GC.KeepAlive(snapshot); }
        }
        private Projection Child(Response result) => new Projection(Owner, result.Handle);
        public ValueKind Kind => Node.Kind == 12 ? (ValueKind)Request(NativeOperation.InspectValue).Tag : Node.Kind == 13 ? ValueKind.Object : (ValueKind)Node.Kind;
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
            if (node.Kind == 6 && node.Key != null || node.Kind == 9 || node.Kind == 10 || node.Kind == 12 || node.Kind == 13) {
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
                if (a.Kind == 13 || b.Kind == 13) { if (!left.Equals(right)) return false; continue; }
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
    public abstract class CoflowObject : ICoflowValue
    {
        protected Projection Value { get; }
        protected CoflowObject(Record record) { Value = record.Value; Value.Publish(this); }
        void ICoflowValue.Encode(ArgumentWriter writer) => writer.Write(Value);
        internal Projection ArgumentProjection => Value;
        public string ActualType => Value.TypeName;
        protected T Read<T>(string field, Func<Projection, T> read) => Value.Field(field).ReadProjected(read);
        public bool ValueEquals(CoflowObject other) => other != null && Value.ValueEquals(other.Value);
        public override bool Equals(object? other) => other is CoflowObject value && Value.Equals(value.Value);
        public override int GetHashCode() => Value.GetHashCode();
        public static bool operator ==(CoflowObject? left, CoflowObject? right) => ReferenceEquals(left, right) || (!(left is null) && left.Equals(right));
        public static bool operator !=(CoflowObject? left, CoflowObject? right) => !(left == right);
    }
    [EditorBrowsable(EditorBrowsableState.Never)]
    public readonly struct Record
    {
        internal Projection Value { get; }
        public Record(Projection value) { Value = value; }
        public Projection Field(string name) => Value.Field(name);
    }

}
