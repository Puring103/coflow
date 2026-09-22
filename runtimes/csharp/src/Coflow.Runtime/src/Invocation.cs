using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Text;

namespace Coflow
{
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
        // 编码后的字节不拥有原生租约；调用完成前必须保留所有投影。
        private readonly List<Projection> roots = new List<Projection>();
        private Projection? captured;
        internal ArgumentWriter(int count) { WriteUInt32(checked((uint)count)); }
        private ArgumentWriter() { }
        public void Write(Projection value)
        {
            if (captured.HasValue) throw new CoflowException("An argument can only be encoded once.");
            if (bytes.Count == 0) { captured = value; return; }
            roots.Add(value);
            value.Write(this);
        }
        internal static Projection Capture(ICoflowValue value)
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
    public sealed class CoflowTemplate : ICoflowValue
    {
        private readonly Projection value;
        void ICoflowValue.Encode(ArgumentWriter writer) => writer.Write(value);
        public CoflowTemplate(Projection value) { this.value = value; }
        public CoflowTemplate(string text) { value = Projection.From(text ?? throw new ArgumentNullException(nameof(text))); }
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
    public abstract class CoflowFunction : ICoflowValue
    {
        private readonly ExecutionTarget target;
        void ICoflowValue.Encode(ArgumentWriter writer) => writer.Write(target.Projection);
        protected Runtime Owner => target.Owner;
        protected CoflowFunction(Projection value) { target = new ExecutionTarget(value); }
        public string Source => target.Source;
        internal TResult InvokeCore<TResult>(ArgumentWriter writer, InvocationCodec<TResult> codec)
        {
            try { return codec.ReadResult(Owner, target.Invoke(writer.Finish())); }
            finally { GC.KeepAlive(writer); GC.KeepAlive(target); }
        }
    }
    public sealed class CoflowFunction<TResult> : CoflowFunction
    {
        private readonly InvocationCodec<TResult> result;
        public CoflowFunction(Projection value, InvocationCodec<TResult> result) : base(value) { this.result = result; }
        public TResult Invoke() => InvokeCore(new ArgumentWriter(0), result);
    }
    public sealed class CoflowFunction<T1, TResult> : CoflowFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<TResult> result;
        public CoflowFunction(Projection value, InvocationCodec<T1> c1, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.result = result; }
        public TResult Invoke(T1 a1) { var w = new ArgumentWriter(1); c1.WriteArgument(w, a1); return InvokeCore(w, result); }
    }
    public sealed class CoflowFunction<T1, T2, TResult> : CoflowFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<TResult> result;
        public CoflowFunction(Projection value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2) { var w = new ArgumentWriter(2); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); return InvokeCore(w, result); }
    }
    public sealed class CoflowFunction<T1, T2, T3, TResult> : CoflowFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<T3> c3; private readonly InvocationCodec<TResult> result;
        public CoflowFunction(Projection value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<T3> c3, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.c3 = c3; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2, T3 a3) { var w = new ArgumentWriter(3); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); c3.WriteArgument(w, a3); return InvokeCore(w, result); }
    }
    public sealed class CoflowFunction<T1, T2, T3, T4, TResult> : CoflowFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<T3> c3; private readonly InvocationCodec<T4> c4; private readonly InvocationCodec<TResult> result;
        public CoflowFunction(Projection value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<T3> c3, InvocationCodec<T4> c4, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.c3 = c3; this.c4 = c4; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2, T3 a3, T4 a4) { var w = new ArgumentWriter(4); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); c3.WriteArgument(w, a3); c4.WriteArgument(w, a4); return InvokeCore(w, result); }
    }
    public sealed class CoflowFunction<T1, T2, T3, T4, T5, TResult> : CoflowFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<T3> c3; private readonly InvocationCodec<T4> c4; private readonly InvocationCodec<T5> c5; private readonly InvocationCodec<TResult> result;
        public CoflowFunction(Projection value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<T3> c3, InvocationCodec<T4> c4, InvocationCodec<T5> c5, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.c3 = c3; this.c4 = c4; this.c5 = c5; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2, T3 a3, T4 a4, T5 a5) { var w = new ArgumentWriter(5); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); c3.WriteArgument(w, a3); c4.WriteArgument(w, a4); c5.WriteArgument(w, a5); return InvokeCore(w, result); }
    }
    public sealed class CoflowFunction<T1, T2, T3, T4, T5, T6, TResult> : CoflowFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<T3> c3; private readonly InvocationCodec<T4> c4; private readonly InvocationCodec<T5> c5; private readonly InvocationCodec<T6> c6; private readonly InvocationCodec<TResult> result;
        public CoflowFunction(Projection value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<T3> c3, InvocationCodec<T4> c4, InvocationCodec<T5> c5, InvocationCodec<T6> c6, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.c3 = c3; this.c4 = c4; this.c5 = c5; this.c6 = c6; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2, T3 a3, T4 a4, T5 a5, T6 a6) { var w = new ArgumentWriter(6); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); c3.WriteArgument(w, a3); c4.WriteArgument(w, a4); c5.WriteArgument(w, a5); c6.WriteArgument(w, a6); return InvokeCore(w, result); }
    }
    public sealed class CoflowFunction<T1, T2, T3, T4, T5, T6, T7, TResult> : CoflowFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<T3> c3; private readonly InvocationCodec<T4> c4; private readonly InvocationCodec<T5> c5; private readonly InvocationCodec<T6> c6; private readonly InvocationCodec<T7> c7; private readonly InvocationCodec<TResult> result;
        public CoflowFunction(Projection value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<T3> c3, InvocationCodec<T4> c4, InvocationCodec<T5> c5, InvocationCodec<T6> c6, InvocationCodec<T7> c7, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.c3 = c3; this.c4 = c4; this.c5 = c5; this.c6 = c6; this.c7 = c7; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2, T3 a3, T4 a4, T5 a5, T6 a6, T7 a7) { var w = new ArgumentWriter(7); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); c3.WriteArgument(w, a3); c4.WriteArgument(w, a4); c5.WriteArgument(w, a5); c6.WriteArgument(w, a6); c7.WriteArgument(w, a7); return InvokeCore(w, result); }
    }
    public sealed class CoflowFunction<T1, T2, T3, T4, T5, T6, T7, T8, TResult> : CoflowFunction
    {
        private readonly InvocationCodec<T1> c1; private readonly InvocationCodec<T2> c2; private readonly InvocationCodec<T3> c3; private readonly InvocationCodec<T4> c4; private readonly InvocationCodec<T5> c5; private readonly InvocationCodec<T6> c6; private readonly InvocationCodec<T7> c7; private readonly InvocationCodec<T8> c8; private readonly InvocationCodec<TResult> result;
        public CoflowFunction(Projection value, InvocationCodec<T1> c1, InvocationCodec<T2> c2, InvocationCodec<T3> c3, InvocationCodec<T4> c4, InvocationCodec<T5> c5, InvocationCodec<T6> c6, InvocationCodec<T7> c7, InvocationCodec<T8> c8, InvocationCodec<TResult> result) : base(value) { this.c1 = c1; this.c2 = c2; this.c3 = c3; this.c4 = c4; this.c5 = c5; this.c6 = c6; this.c7 = c7; this.c8 = c8; this.result = result; }
        public TResult Invoke(T1 a1, T2 a2, T3 a3, T4 a4, T5 a5, T6 a6, T7 a7, T8 a8) { var w = new ArgumentWriter(8); c1.WriteArgument(w, a1); c2.WriteArgument(w, a2); c3.WriteArgument(w, a3); c4.WriteArgument(w, a4); c5.WriteArgument(w, a5); c6.WriteArgument(w, a6); c7.WriteArgument(w, a7); c8.WriteArgument(w, a8); return InvokeCore(w, result); }
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
        public static InvocationCodec<CoflowTemplate> TemplateInvocation { get; } = new InvocationCodec<CoflowTemplate>(
            value => new CoflowTemplate(value), (writer, value) => WriteRuntime(writer, value),
            (runtime, response) => response.Tag == 4
                ? new CoflowTemplate(Encoding.UTF8.GetString(Native.ReadBuffer(response)))
                : new CoflowTemplate(ReadRuntime(runtime, response)));
        public static InvocationCodec<T> EnumInvocation<T>(string typeName, Func<uint, T> read, Func<T, uint> write) => new InvocationCodec<T>(
            value => read(value.Enum), (writer, value) => { writer.WriteByte(5); writer.WriteString(typeName); writer.WriteUInt32(write(value)); },
            (_, response) => response.Tag == 5 && Encoding.UTF8.GetString(Native.ReadBuffer(response)) == typeName ? read(checked((uint)response.Integer)) : throw TypeMismatch());
        public static InvocationCodec<T> CoflowInvocation<T>(Func<Projection, T> read) where T : ICoflowValue => new InvocationCodec<T>(
            read, (writer, value) => WriteRuntime(writer, value), (runtime, response) => read(ReadRuntime(runtime, response)));
        public static InvocationCodec<T?> OptionalValueInvocation<T>(InvocationCodec<T> inner) where T : struct => new InvocationCodec<T?>(
            value => value.IsNone ? (T?)null : inner.ReadValue(value),
            (writer, value) => { if (value.HasValue) inner.WriteArgument(writer, value.Value); else writer.WriteByte(0); },
            (runtime, response) => response.Tag == 0 ? (T?)null : inner.ReadResult(runtime, response));
        public static InvocationCodec<T?> OptionalReferenceInvocation<T>(InvocationCodec<T> inner) where T : class => new InvocationCodec<T?>(
            value => value.IsNone ? null : inner.ReadValue(value),
            (writer, value) => { if (value == null) writer.WriteByte(0); else inner.WriteArgument(writer, value); },
            (runtime, response) => response.Tag == 0 ? null : inner.ReadResult(runtime, response));
        private static void WriteRuntime<T>(ArgumentWriter writer, T value) where T : ICoflowValue
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
