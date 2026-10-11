using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

internal sealed class CoflowNativeCallSite
{
    internal CoflowNativeCallSite(CoflowNativeCall call, CoflowValueRegister[] arguments, CoflowValueRegister result)
    {
        Call = call;
        Arguments = CoflowFrozenArray<CoflowValueRegister>.CopyOf(arguments);
        Result = result;
    }

    internal CoflowNativeCall Call { get; }

    internal CoflowFrozenArray<CoflowValueRegister> Arguments { get; }

    internal CoflowValueRegister Result { get; }
}
}
