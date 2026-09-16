using System;
using System.Runtime.InteropServices;
using System.Text;

namespace Coflow
{
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
    }
    internal static class Native
    {
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
        internal static Response Call(uint op, NativeHandle? handle = null, string key = "", byte[]? data = null, ulong index = 0, ulong value = 0)
        {
            var encoded = Encoding.UTF8.GetBytes(key);
            data ??= Array.Empty<byte>();
            bool retained = false;
            try
            {
                if (handle != null) handle.DangerousAddRef(ref retained);
                uint status = Request(op, handle?.Id ?? 0, value, encoded, (UIntPtr)encoded.Length,
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
        internal static string ReadString(uint op, NativeHandle handle) => Encoding.UTF8.GetString(ReadBuffer(Call(op, handle)));
    }
    public class CoflowException : Exception { public CoflowException(string message) : base(message) { } }
}
