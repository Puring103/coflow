using System;
using System.Runtime.InteropServices;
using System.Text;

namespace Coflow
{
    public abstract class HostBinding
    {
        internal string Service { get; }
        protected HostBinding(string service) { Service = service; }
        public abstract string MemberType(string field);
        public abstract object? Read(string field);
        public abstract void Call(string field, HostCall call);
    }
    public sealed class HostCall
    {
        private readonly byte[] data;
        private int position;
        private readonly int count;
        private int consumed;
        internal Response Result { get; private set; }
        internal HostCall(byte[] data, int position)
        {
            this.data = data; this.position = position;
            count = checked((int)ReadUInt32());
        }
        public T Argument<T>(InvocationCodec<T> codec)
        {
            if (codec == null) throw new ArgumentNullException(nameof(codec));
            if (consumed >= count) throw new CoflowException("Host argument count mismatch.");
            consumed++;
            var response = ReadEncodedValue();
            var runtime = response.Tag == 11 ? Runtime.Lookup(response.Handle) : null!;
            return codec.ReadResult(runtime, response);
        }
        public void Return<T>(InvocationCodec<T> codec, T value)
        {
            if (codec == null) throw new ArgumentNullException(nameof(codec));
            if (consumed != count) throw new CoflowException("Host argument count mismatch.");
            var writer = new ArgumentWriter(1);
            codec.WriteArgument(writer, value);
            var encoded = writer.Finish();
            if (encoded[4] == 6 || encoded[4] == 7 || encoded[4] == 8) {
                Result = Native.Call(NativeOperation.CreateBuffer, data: encoded);
                var aggregate = Result; aggregate.Tag = 12; Result = aggregate; return;
            }
            var originalPosition = position;
            position = 4;
            Result = ReadEncodedValue(encoded);
            position = originalPosition;
        }
        private Response ReadEncodedValue() => ReadEncodedValue(data);
        private Response ReadEncodedValue(byte[] source)
        {
            byte tag = ReadByte(source);
            var response = new Response { Tag = tag };
            switch (tag)
            {
                case 0: break;
                case 1: response.Integer = ReadByte(source) == 0 ? 0 : 1; break;
                case 2: response.Integer = unchecked((int)ReadUInt32(source)); break;
                case 3: response.Number = BitConverter.Int32BitsToSingle(unchecked((int)ReadUInt32(source))); break;
                case 4:
                    var text = ReadBytes(source, checked((int)ReadUInt32(source)));
                    response = Native.Call(NativeOperation.CreateBuffer, data: text); response.Tag = 4; break;
                case 5:
                    var type = ReadBytes(source, checked((int)ReadUInt32(source)));
                    response = Native.Call(NativeOperation.CreateBuffer, data: type); response.Tag = 5; response.Integer = ReadUInt32(source); break;
                case 11: response.Handle = ReadUInt64(source); response.Length = ReadUInt64(source); break;
                default: throw new CoflowException("Unknown Host value tag.");
            }
            return response;
        }
        private byte ReadByte(byte[]? source = null)
        {
            var bytes = source ?? data;
            if ((uint)position >= (uint)bytes.Length) throw new CoflowException("Truncated Host arguments.");
            return bytes[position++];
        }
        private uint ReadUInt32(byte[]? source = null)
        {
            var bytes = source ?? data;
            uint value = ReadByte(bytes);
            value |= (uint)ReadByte(bytes) << 8; value |= (uint)ReadByte(bytes) << 16; value |= (uint)ReadByte(bytes) << 24;
            return value;
        }
        private ulong ReadUInt64(byte[]? source = null) => ReadUInt32(source) | ((ulong)ReadUInt32(source) << 32);
        private byte[] ReadBytes(byte[] source, int length)
        {
            if (length < 0 || position > source.Length - length) throw new CoflowException("Truncated Host arguments.");
            var result = new byte[length]; Array.Copy(source, position, result, 0, length); position += length; return result;
        }
    }
    public readonly struct HostEnum
    {
        internal string TypeName { get; }
        internal uint Value { get; }
        public HostEnum(string typeName, uint value) { TypeName = typeName; Value = value; }
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
        internal static void Bind(RuntimeBuilder builder,HostBinding host)
        {
            if(host==null)throw new ArgumentNullException(nameof(host));
            // 强所有权留在托管 builder/runtime；原生只持弱句柄，避免 Host 反向引用
            // Runtime 时形成 GC 无法看见的跨语言强引用环。
            var context=GCHandle.Alloc(host, GCHandleType.Weak);
            bool retained=false;
            bool submitted=false;
            try
            {
                builder.Handle.DangerousAddRef(ref retained);
                var bytes=Encoding.UTF8.GetBytes(host.Service);
                // 原生接口接管 context，绑定失败也会调用 Free。
                uint result=Bind(builder.Handle.Id,bytes,(UIntPtr)bytes.Length,unchecked((ulong)GCHandle.ToIntPtr(context).ToInt64()),ReadCallback,FreeCallback);
                submitted=true;
                if(result!=0)throw new CoflowException("Host binding does not match its declared service.");
            }
            finally
            {
                if(!submitted&&context.IsAllocated)context.Free();
                if(retained)builder.Handle.DangerousRelease();
                GC.KeepAlive(host);
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
                var host=GCHandle.FromIntPtr(new IntPtr(unchecked((long)context))).Target as HostBinding
                    ?? throw new CoflowException("Host owner is no longer available.");
                if(op==2)
                {
                    int position=0;
                    uint fieldLength=ReadUInt32(bytes,ref position);
                    if(fieldLength>int.MaxValue||position>bytes.Length-(int)fieldLength)throw new CoflowException("Invalid Host call payload.");
                    string function=Encoding.UTF8.GetString(bytes,position,(int)fieldLength); position+=(int)fieldLength;
                    var call=new HostCall(bytes,position); host.Call(function,call); result=call.Result; return;
                }
                string name=Encoding.UTF8.GetString(bytes);
                if(op==0){result=Native.Call(NativeOperation.CreateBuffer,data:Encoding.UTF8.GetBytes(host.MemberType(name)));return;}
                switch(host.Read(name))
                {
                    case null: result.Tag=0;break;
                    case bool v: result.Tag=1;result.Integer=v?1:0;break;
                    case int v: result.Tag=2;result.Integer=v;break;
                    case float v: result.Tag=3;result.Number=v;break;
                    case string v: result=Native.Call(NativeOperation.CreateBuffer,data:Encoding.UTF8.GetBytes(v));result.Tag=4;break;
                    case HostEnum v:
                        result=Native.Call(NativeOperation.CreateBuffer,data:Encoding.UTF8.GetBytes(v.TypeName)); result.Tag=5; result.Integer=v.Value; break;
                    case IRuntimeArgument v:
                        var writer = new ArgumentWriter(1); v.Encode(writer);
                        result = Native.Call(NativeOperation.CreateBuffer, data: writer.Finish()); result.Tag = 12; break;
                    default: throw new CoflowException("Host data must be a scalar or a value from the same Runtime.");
                }
            }
            catch(Exception error)
            {
                try{result=Native.Call(NativeOperation.CreateBuffer,data:Encoding.UTF8.GetBytes(error.Message));}catch{result=default;}
                result.Error=1;
            }
        }
        private static uint ReadUInt32(byte[] bytes,ref int position)
        {
            if(position>bytes.Length-4)throw new CoflowException("Invalid Host call payload.");
            uint value=bytes[position]; value|=(uint)bytes[position+1]<<8; value|=(uint)bytes[position+2]<<16; value|=(uint)bytes[position+3]<<24; position+=4; return value;
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
