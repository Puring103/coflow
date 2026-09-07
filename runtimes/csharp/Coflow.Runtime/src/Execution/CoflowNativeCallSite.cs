namespace Coflow.Runtime.CompilerServices;

internal sealed class CoflowNativeCallSite(CoflowNativeCall call, CoflowValueRegister[] arguments, CoflowValueRegister result)
{
    internal CoflowNativeCall Call { get; } = call;

    internal CoflowFrozenArray<CoflowValueRegister> Arguments { get; } =
        CoflowFrozenArray<CoflowValueRegister>.CopyOf(arguments);

    internal CoflowValueRegister Result { get; } = result;
}
