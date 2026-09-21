using System;
using System.Runtime.InteropServices;
using System.Text;

namespace Coflow
{
    // 与 coflow_ffi 的 Operation 一一对应，托管代码不直接使用协议数字。
    internal enum NativeOperation : uint
    {
        LoadContract = 1,
        ContractIdentity = 8,
        CreateBuilder = 10,
        AddDataSource = 11,
        BuildRuntime = 12,
        TableLength = 21,
        TableValue = 22,
        ReadField = 23,
        InspectValue = 24,
        ReadText = 25,
        ArrayValue = 26,
        DictionaryKey = 27,
        DictionaryValue = 28,
        Invoke = 29,
        TypeName = 30,
        ProgramSource = 31,
        TryFindRecord = 32,
        DimensionVariant = 34,
        ValueEquals = 35,
        DimensionDefault = 36,
        Singleton = 37,
        DictionaryFind = 38,
        CanonicalValue = 39,
        CreateBuffer = 41,
        Collect = 43,
        RunChecks = 45,
        DimensionVariantKey = 46,
        ReadDynamicValue = 47,
        CreateValueLease = 48,
        ReadRecord = 49,
    }
    [StructLayout(LayoutKind.Sequential)]
    internal struct Response
    {
        internal ulong Handle;
        internal long Integer;
        internal double Number;
        internal ulong Length;
        internal uint Tag;
        internal uint Error;
    }
    // ulong 保存完整身份，SafeHandle 的 IntPtr 只标记存活，兼容 32 位 IL2CPP。
    internal sealed class NativeHandle : SafeHandle
    {
        internal ulong Id { get; }
        internal NativeHandle(ulong id) : base(IntPtr.Zero, true)
        {
            if (id == 0) throw new ArgumentException("Invalid native handle.", nameof(id));
            Id = id;
            SetHandle(new IntPtr(1));
        }
        public override bool IsInvalid => handle == IntPtr.Zero;
        protected override bool ReleaseHandle() { Native.Release(Id); handle = IntPtr.Zero; return true; }
        internal bool DisposeExplicit()
        {
            if (IsClosed || IsInvalid) return true;
            if (!Native.Dispose(Id)) return false;
            SetHandleAsInvalid();
            return true;
        }
    }
    internal static class Native
    {
        [ThreadStatic] internal static long RequestCount;
#if (UNITY_IOS || UNITY_WEBGL) && !UNITY_EDITOR
        private const string Library = "__Internal";
#else
        private const string Library = "coflow_ffi";
#endif
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl, EntryPoint = "coflow_request")]
        private static extern uint Request(uint op, ulong handle, ulong value, byte[] key, UIntPtr keyLength,
            byte[] data, UIntPtr dataLength, ulong index, out Response response);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl, EntryPoint = "coflow_buffer_copy")]
        private static extern uint Copy(ulong handle, byte[] destination, UIntPtr capacity);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl, EntryPoint = "coflow_release")]
        internal static extern void Release(ulong handle);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl, EntryPoint = "coflow_dispose")]
        [return: MarshalAs(UnmanagedType.I4)]
        private static extern uint DisposeNative(ulong handle);
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl, EntryPoint = "coflow_thread_drain")]
        internal static extern ulong ThreadDrain();
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl, EntryPoint = "coflow_thread_shutdown")]
        [return: MarshalAs(UnmanagedType.I4)]
        internal static extern uint ThreadShutdown();
        internal static bool Dispose(ulong handle) => DisposeNative(handle) == 0;
        internal static Response Call(NativeOperation op, NativeHandle? handle = null, string key = "", byte[]? data = null, ulong index = 0, ulong value = 0)
        {
            var encoded = Encoding.UTF8.GetBytes(key);
            data ??= Array.Empty<byte>();
            bool retained = false;
            try
            {
                if (handle != null) handle.DangerousAddRef(ref retained);
                System.Threading.Interlocked.Increment(ref RequestCount);
                uint status = Request((uint)op, handle?.Id ?? 0, value, encoded, (UIntPtr)encoded.Length,
                    data, (UIntPtr)data.Length, index, out var result);
                if (status == 2) throw BuildException.Decode(ReadBuffer(result));
                if (status != 0) throw new CoflowException(result.Handle == 0 ? "Native operation failed." : Encoding.UTF8.GetString(ReadBuffer(result)));
                return result;
            }
            finally { if (retained) handle!.DangerousRelease(); }
        }
        internal static byte[] ReadBuffer(Response response)
        {
            using var buffer = new NativeHandle(response.Handle);
            if (response.Length > int.MaxValue) throw new CoflowException("Native buffer is too large.");
            var bytes = new byte[(int)response.Length];
            if (Copy(buffer.Id, bytes, (UIntPtr)bytes.Length) != 0) throw new CoflowException("Native buffer read failed.");
            return bytes;
        }
        internal static string ReadString(NativeOperation op, NativeHandle handle) => Encoding.UTF8.GetString(ReadBuffer(Call(op, handle)));
    }
    public class CoflowException : Exception { public CoflowException(string message) : base(message) { } }
}
