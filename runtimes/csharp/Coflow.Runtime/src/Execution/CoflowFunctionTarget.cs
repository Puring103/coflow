using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

internal readonly struct CoflowFunctionTarget
{
    private readonly CoflowFunctionEntry? _entry;
    private readonly object? _receiver;
    private readonly CoflowClosure? _closure;

    internal CoflowFunctionTarget(CoflowFunctionEntry entry, object? receiver)
    {
        _entry = entry;
        _receiver = receiver;
        _closure = null;
    }

    internal CoflowFunctionTarget(CoflowClosure closure)
    {
        _entry = null;
        _receiver = null;
        _closure = closure;
    }

    internal CoflowFunctionEntry Entry => _entry ??
        throw new InvalidOperationException("A closure target has no function entry.");
    internal object? Receiver => _receiver;
    internal CoflowClosure? Closure => _closure;

    /// <summary>所有调用目标只在这里分派一次，参数数量不会继续传播到执行层。</summary>
    internal TResult Invoke<TArguments, TResult>(TArguments arguments)
        where TArguments : struct, ICoflowArgumentPack
    {
        if (_closure is { } closure)
            return CoflowVm.ExecuteClosure<TArguments, TResult>(closure, arguments);
        if (_entry!.CompiledProgram is { } program)
            return CoflowVm.ExecuteBound<TArguments, TResult>(program, _receiver!, arguments);
        return _entry.InvokeHost<TArguments, TResult>(arguments);
    }

    internal void InvokeVoid<TArguments>(TArguments arguments)
        where TArguments : struct, ICoflowArgumentPack
    {
        if (_closure is { } closure)
        {
            _ = CoflowVm.ExecuteClosure<TArguments, Unit>(closure, arguments);
            return;
        }
        if (_entry!.CompiledProgram is { } program)
        {
            _ = CoflowVm.ExecuteBound<TArguments, Unit>(program, _receiver!, arguments);
            return;
        }
        _entry.InvokeHostVoid(arguments);
    }
}
}
