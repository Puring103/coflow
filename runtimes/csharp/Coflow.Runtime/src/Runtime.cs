using System;
using System.Collections.Generic;
using System.Text;

namespace Coflow.Runtime
{
    public sealed class CoflowContract : IDisposable
    {
        internal NativeHandle Handle { get; }
        internal CoflowContract(ulong handle) { Handle = new NativeHandle(handle); }
        public static CoflowContract Load(byte[] bytes) => new CoflowContract(Native.Call(1, data: bytes).Handle);
        public byte[] Identity => Native.ReadBuffer(Native.Call(8, Handle));
        public byte[] ToBytes() => Native.ReadBuffer(Native.Call(7, Handle));
        public CoflowBuilder CreateBuilder() => new CoflowBuilder(Native.Call(10, Handle).Handle);
        public void Dispose() => Handle.Dispose();
    }
    public sealed class CoflowBuilder : IDisposable
    {
        internal NativeHandle Handle { get; }
        internal CoflowBuilder(ulong handle) { Handle = new NativeHandle(handle); }
        public void AddSource(string logicalPath, string source) => Native.Call(11, Handle, logicalPath, Encoding.UTF8.GetBytes(source));
        public void BindHost(string service, ICoflowHost host) => HostBridge.Bind(this, service, host);
        public CoflowRuntime Build() => new CoflowRuntime(Native.Call(12, Handle).Handle);
        public void Dispose() => Handle.Dispose();
    }
    public sealed class CoflowRuntime : IDisposable
    {
        internal NativeHandle Handle { get; }
        internal CoflowRuntime(ulong handle) { Handle = new NativeHandle(handle); }
        public CoflowValue Record(string type, string key) => new CoflowValue(Native.Call(20, Handle, type, Encoding.UTF8.GetBytes(key)).Handle, this);
        public IReadOnlyList<T> Records<T>(string type, Func<CoflowValue, T> wrap)
        {
            int count = checked((int)Native.Call(21, Handle, type).Length);
            var records = new List<T>(count);
            for (int i = 0; i < count; ++i) records.Add(wrap(new CoflowValue(Native.Call(22, Handle, type, index: (ulong)i).Handle, this)));
            return records;
        }
        public void Dispose() => Handle.Dispose();
    }
    public enum CoflowValueKind : uint { None, Bool, Int, Float, String, Enum, Object, Array, Dictionary, Function, Template }
    public interface ICoflowValue { CoflowValue RetainValue(); }
    public sealed class CoflowValue : IDisposable, ICoflowValue
    {
        internal NativeHandle Handle { get; }
        // 值包装保活所属 Runtime；显式 Dispose 仍使所有依赖值失效。
        internal CoflowRuntime Owner { get; }
        internal CoflowValue(ulong handle, CoflowRuntime owner) { Handle = new NativeHandle(handle); Owner = owner; }
        private Response Describe() => Native.Call(24, Handle);
        public CoflowValueKind Kind => (CoflowValueKind)Describe().Tag;
        public int Count => checked((int)Describe().Length);
        public bool IsNone => Kind == CoflowValueKind.None;
        public bool Bool { get { var r = Describe(); Require(r.Tag, 1); return r.Integer != 0; } }
        public int Int { get { var r = Describe(); Require(r.Tag, 2); return checked((int)r.Integer); } }
        public float Float { get { var r = Describe(); Require(r.Tag, 3); return (float)r.Number; } }
        public uint Enum { get { var r = Describe(); Require(r.Tag, 5); return checked((uint)r.Integer); } }
        public string Text => Native.ReadString(25, Handle);
        public string TypeName => Native.ReadString(30, Handle);
        public string ProgramSource => Native.ReadString(31, Handle);
        public void RequireContract(byte[] expected)
        {
            var actual = Native.ReadBuffer(Native.Call(9, Handle));
            bool matches = expected != null && expected.Length == actual.Length;
            if (matches && expected != null) for (int i = 0; i < actual.Length; ++i) matches &= actual[i] == expected[i];
            if (!matches) { Dispose(); throw new CoflowException("Generated types do not match the runtime contract."); }
        }
        public CoflowValue Field(string name) => new CoflowValue(Native.Call(23, Handle, name).Handle, Owner);
        public CoflowValue DimensionDefault() => new CoflowValue(Native.Call(36, Handle).Handle, Owner);
        public CoflowValue DimensionValue(string variant) => new CoflowValue(Native.Call(34, Handle, variant).Handle, Owner);
        public CoflowValue RetainValue() => Retain();
        public CoflowValue Retain() => new CoflowValue(Native.Call(33, Handle).Handle, Owner);
        public bool ValueEquals(CoflowValue other)
        {
            if (other == null) throw new ArgumentNullException(nameof(other));
            bool retained = false;
            try
            {
                other.Handle.DangerousAddRef(ref retained);
                return Native.Call(35, Handle, index: other.Handle.Id).Integer != 0;
            }
            finally { if (retained) other.Handle.DangerousRelease(); }
        }
        public CoflowValue At(int index) => Element(26, index);
        public CoflowValue KeyAt(int index) => Element(27, index);
        public CoflowValue ValueAt(int index) => Element(28, index);
        private CoflowValue Element(uint op, int index)
        {
            if (index < 0) throw new ArgumentOutOfRangeException(nameof(index));
            return new CoflowValue(Native.Call(op, Handle, index: (ulong)index).Handle, Owner);
        }
        public void Call() => Native.Call(29, Handle);
        private static void Require(uint actual, uint expected) { if (actual != expected) throw new CoflowException("Value type mismatch."); }
        public void Dispose() => Handle.Dispose();
    }
    public abstract class CoflowObject : IDisposable, ICoflowValue
    {
        protected CoflowValue Value { get; }
        protected CoflowObject(CoflowValue value) { Value = value; }
        public string ActualType => Value.TypeName;
        public CoflowValue RetainValue() => Value.Retain();
        public bool ValueEquals(CoflowObject other) => other != null && Value.ValueEquals(other.Value);
        protected T Read<T>(string field, Func<CoflowValue, T> codec) => codec(Value.Field(field));
        public void Dispose() => Value.Dispose();
    }
    public sealed class CoflowFunction : IDisposable, ICoflowValue
    {
        private readonly CoflowValue value;
        public CoflowFunction(CoflowValue value) { this.value = value; }
        public CoflowValue RetainValue() => value.Retain();
        public string Source => value.ProgramSource;
        public void Call() => value.Call();
        public void Dispose() => value.Dispose();
    }
    public static class CoflowCodecs
    {
        public static int Int(CoflowValue value) { using (value) return value.Int; }
        public static float Float(CoflowValue value) { using (value) return value.Float; }
        public static bool Bool(CoflowValue value) { using (value) return value.Bool; }
        public static string String(CoflowValue value) { using (value) return value.Text; }
        public static uint Enum(CoflowValue value) { using (value) return value.Enum; }
    }
}
