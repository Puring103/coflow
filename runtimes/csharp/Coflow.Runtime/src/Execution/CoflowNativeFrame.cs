using System;

namespace Coflow.Runtime.CompilerServices;

internal readonly struct CoflowNativeFrame
{
    private readonly CoflowVm.CoflowExecutionContext _context;

    private readonly CoflowValueRegister[] _arguments;

    private readonly CoflowValueRegister _result;

    private readonly Type _resultType;

    internal CoflowNativeFrame(CoflowVm.CoflowExecutionContext context, CoflowNativeCallSite site)
        : this(context, site.Arguments, site.Result, site.Call.ResultType)
    {
    }

    internal CoflowNativeFrame(CoflowVm.CoflowExecutionContext context, CoflowValueRegister[] arguments, CoflowValueRegister result, Type resultType)
    {
        _context = context;
        _arguments = arguments;
        _result = result;
        _resultType = resultType;
    }

    public T Read<T>(int index)
    {
        return CoflowBoundaryCodec<T>.ReadRelative(_context, _arguments[index]);
    }

    public void Write<T>(T value)
    {
        if (typeof(T) != _resultType)
        {
            throw new InvalidOperationException($"native result `{typeof(T)}` does not match `{_resultType}`");
        }
        CoflowBoundaryCodec<T>.WriteRelative(_context, _result, value);
    }

    public void WriteImported<T>(T value)
    {
        if (typeof(T) != _resultType)
        {
            throw new InvalidOperationException($"native result `{typeof(T)}` does not match `{_resultType}`");
        }
        CoflowBoundaryCodec<T>.WriteImportedRelative(_context, _result, value);
    }

    internal void WriteFunction(CoflowFunctionId functionId, CoflowValueId environmentId)
    {
        _context.WriteIntegerRelative(_result.IntegerBase, functionId.Packed);
        _context.WriteIntegerRelative(_result.IntegerBase + 1, unchecked((long)environmentId.Packed));
    }
}
