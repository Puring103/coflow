using System;
using System.Runtime.InteropServices;
using System.Text;

namespace Coflow.Runtime
{
    public interface ICoflowHost
    {
        string MemberType(string field);
        object? Read(string field);
    }
    internal static class HostBridge
    {
#if (UNITY_IOS || UNITY_WEBGL) && !UNITY_EDITOR
        private const string Library = "__Internal";
#else
        private const string Library = "coflow_ffi";
#endif
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate void Callback(ulong context, uint op, IntPtr field, UIntPtr length, out Response result);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate void ReleaseCallback(ulong context);
        private static readonly Callback ReadCallback = Read;
        private static readonly ReleaseCallback FreeCallback = Free;
        [DllImport(Library,CallingConvention=CallingConvention.Cdecl,EntryPoint="coflow_bind_host")]
        private static extern uint Bind(ulong builder,byte[] service,UIntPtr length,ulong context,Callback read,ReleaseCallback free);
        internal static void Bind(CoflowBuilder builder,string service,ICoflowHost host)
        {
            if(host==null)throw new ArgumentNullException(nameof(host));
            var context=GCHandle.Alloc(host);
            bool retained=false;
            bool submitted=false;
            try
            {
                builder.Handle.DangerousAddRef(ref retained);
                var bytes=Encoding.UTF8.GetBytes(service);
                // 原生接口接管 context，绑定失败也会调用 Free。
                uint result=Bind(builder.Handle.Id,bytes,(UIntPtr)bytes.Length,unchecked((ulong)GCHandle.ToIntPtr(context).ToInt64()),ReadCallback,FreeCallback);
                submitted=true;
                if(result!=0)throw new CoflowException("Host binding does not match its declared service.");
            }
            finally
            {
                if(!submitted&&context.IsAllocated)context.Free();
                if(retained)builder.Handle.DangerousRelease();
            }
        }
#if UNITY_2022_1_OR_NEWER
        [AOT.MonoPInvokeCallback(typeof(Callback))]
#endif
        private static void Read(ulong context,uint op,IntPtr field,UIntPtr length,out Response result)
        {
            result=default;
            try
            {
                var bytes=new byte[checked((int)length.ToUInt64())];Marshal.Copy(field,bytes,0,bytes.Length);
                string name=Encoding.UTF8.GetString(bytes);
                var host=(ICoflowHost)GCHandle.FromIntPtr(new IntPtr(unchecked((long)context))).Target!;
                if(op==0){result=Native.Call(41,data:Encoding.UTF8.GetBytes(host.MemberType(name)));return;}
                switch(host.Read(name))
                {
                    case null: result.Tag=0;break;
                    case bool v: result.Tag=1;result.Integer=v?1:0;break;
                    case int v: result.Tag=2;result.Integer=v;break;
                    case float v: result.Tag=3;result.Number=v;break;
                    case string v: result=Native.Call(41,data:Encoding.UTF8.GetBytes(v));result.Tag=4;break;
                    case ICoflowValue v:
                        using (var value = v.RetainValue()) { result = Native.Call(33, value.Handle); result.Tag = 11; } break;
                    default: throw new CoflowException("Host data must be a scalar or a value from the same Runtime.");
                }
            }
            catch(Exception error)
            {
                try{result=Native.Call(41,data:Encoding.UTF8.GetBytes(error.Message));}catch{result=default;}
                result.Error=1;
            }
        }
#if UNITY_2022_1_OR_NEWER
        [AOT.MonoPInvokeCallback(typeof(ReleaseCallback))]
#endif
        private static void Free(ulong context)
        {
            // 终结线程只解除托管保活，不调用 Unity API。
            try{GCHandle.FromIntPtr(new IntPtr(unchecked((long)context))).Free();}catch{ }
        }
    }
}
