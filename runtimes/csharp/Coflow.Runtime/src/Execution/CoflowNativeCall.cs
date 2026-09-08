using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;

namespace Coflow.Runtime.CompilerServices
{

internal sealed class CoflowNativeCall
{
    internal int ArgumentCount => ParameterTypes.Length;

    internal Type[] ParameterTypes { get; }

    internal Type ResultType { get; }

    internal CoflowNativeInvoker Invoke { get; }

    internal CoflowNativeCall(Type[] parameterTypes, Type resultType, CoflowNativeInvoker invoke)
    {
        ParameterTypes = parameterTypes ?? throw new ArgumentNullException("parameterTypes");
        ResultType = resultType ?? throw new ArgumentNullException("resultType");
        Invoke = invoke ?? throw new ArgumentNullException("invoke");
    }

    internal static CoflowNativeCall Create<TRecord, TValue>(Func<TRecord, TValue> implementation)
    {
        if (implementation == null)
        {
            throw new ArgumentNullException("implementation");
        }
        return new CoflowNativeCall(new Type[1] { typeof(TRecord) }, typeof(TValue), delegate (CoflowNativeFrame frame)
        {
            frame.Write(implementation(frame.Read<TRecord>(0)));
        });
    }
}
}
