namespace Coflow.Runtime.CompilerServices;

internal sealed class CoflowNativeCallSite(CoflowNativeCall call, CoflowValueRegister[] arguments, CoflowValueRegister result)
{
    internal CoflowNativeCall Call { get; } = call;

    internal CoflowValueRegister[] Arguments { get; } = arguments;

    internal CoflowValueRegister Result { get; } = result;
}
