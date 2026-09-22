using System;
using System.Collections.Generic;
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
    // 原生句柄只在创建线程显式释放；托管终结线程不访问原生资源。
    internal sealed class NativeHandle : IDisposable
    {
        private readonly int ownerThread = Environment.CurrentManagedThreadId;
        internal ulong Id { get; }
        private bool disposed;
        private readonly ulong domainVersion = Native.DomainVersion;
        internal bool IsClosed => disposed || (Environment.CurrentManagedThreadId == ownerThread && domainVersion != Native.DomainVersion);
        internal bool IsInvalid => IsClosed;
        internal NativeHandle(ulong id)
        {
            if (id == 0) throw new ArgumentException("Invalid native handle.", nameof(id));
            Id = id;
        }
        internal void RequireAlive()
        {
            if (Environment.CurrentManagedThreadId != ownerThread) throw new CoflowException("Native handle must be used on its creating thread.");
            if (IsClosed) throw new ObjectDisposedException(nameof(NativeHandle));
        }
        internal bool DisposeExplicit()
        {
            RequireThread();
            if (IsClosed) return true;
            if (!Native.Dispose(Id)) return false;
            disposed = true;
            return true;
        }
        private void RequireThread()
        {
            if (Environment.CurrentManagedThreadId != ownerThread) throw new CoflowException("Native handle must be disposed on its creating thread.");
        }
        public void Dispose()
        {
            if (!DisposeExplicit()) throw new CoflowException("Native handle is busy or has already been released by thread shutdown.");
        }
    }
    internal static class Native
    {
        // 原生请求保活回调返回值；同步重入的内层作用域不会撤销外层的根。
        [ThreadStatic] private static List<object>? callbackRoots;
        internal static void RetainCallbackResult(object value) =>
            (callbackRoots ??= new List<object>()).Add(value);
        [ThreadStatic] internal static long RequestCount;
        [ThreadStatic] internal static ulong DomainVersion;
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
        [DllImport(Library, CallingConvention = CallingConvention.Cdecl, EntryPoint = "coflow_thread_shutdown")]
        [return: MarshalAs(UnmanagedType.I4)]
        internal static extern uint ThreadShutdown();
        internal static bool Dispose(ulong handle) => DisposeNative(handle) == 0;
        internal static Response Call(NativeOperation op, NativeHandle? handle = null, string key = "", byte[]? data = null, ulong index = 0, ulong value = 0)
        {
            var encoded = Encoding.UTF8.GetBytes(key);
            data ??= Array.Empty<byte>();
            handle?.RequireAlive();
            var previousRoots = callbackRoots;
            callbackRoots = null;
            try
            {
                ++RequestCount;
                uint status = Request((uint)op, handle?.Id ?? 0, value, encoded, (UIntPtr)encoded.Length,
                    data, (UIntPtr)data.Length, index, out var result);
                if (status == 2) throw BuildException.Decode(ReadBuffer(result));
                if (status != 0) throw new CoflowException(result.Handle == 0 ? "Native operation failed." : Encoding.UTF8.GetString(ReadBuffer(result)));
                return result;
            }
            finally
            {
                var roots = callbackRoots;
                callbackRoots = previousRoots;
                GC.KeepAlive(roots);
                GC.KeepAlive(handle);
            }
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
