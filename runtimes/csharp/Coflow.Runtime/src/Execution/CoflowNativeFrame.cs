using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;

namespace Coflow.Runtime.CompilerServices
{

internal readonly struct CoflowNativeFrame
{
    private readonly CoflowExecutionSession _context;

    private readonly IReadOnlyList<CoflowValueRegister> _arguments;

    private readonly CoflowValueRegister _result;

    private readonly Type _resultType;

    internal CoflowNativeFrame(CoflowExecutionSession context, CoflowNativeCallSite site)
        : this(context, site.Arguments, site.Result, site.Call.ResultType)
    {
    }

    internal CoflowNativeFrame(CoflowExecutionSession context, IReadOnlyList<CoflowValueRegister> arguments, CoflowValueRegister result, Type resultType)
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

    internal bool Compare(CoflowEquality.Comparer compare) => compare(_context,
        _context.Registers.View(_context.Registers.Offset(_arguments[0])),
        _context.Registers.View(_context.Registers.Offset(_arguments[1])));

    internal object ReadRecord(int index, Type expectedType)
    {
        var register = _arguments[index];
        var id = CoflowValueId.FromPacked(unchecked((ulong)
            _context.Registers.ReadIntegerRelative(register.IntegerBase)));
        return _context.ApiValue(id, expectedType);
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
        _context.Registers.WriteIntegerRelative(_result.IntegerBase, functionId.Packed);
        _context.Registers.WriteIntegerRelative(_result.IntegerBase + 1, unchecked((long)environmentId.Packed));
    }
}
}
