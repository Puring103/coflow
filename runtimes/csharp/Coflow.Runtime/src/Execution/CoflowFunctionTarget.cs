namespace Coflow.Runtime.CompilerServices;

internal readonly record struct CoflowFunctionTarget
{
    private readonly CoflowFunctionEntry? _entry;
    private readonly object? _receiver;
    private readonly CoflowClosure? _closure;
    internal CoflowFunctionEntry? Entry => _entry;
    internal object? Receiver => _receiver;
    internal CoflowClosure? Closure => _closure;
    internal CoflowFunctionTarget(CoflowFunctionEntry entry, object? receiver) { _entry = entry; _receiver = receiver; _closure = null; }
    internal CoflowFunctionTarget(CoflowClosure closure) { _entry = null; _receiver = null; _closure = closure; }

    internal TResult Invoke<TResult>() => _closure is { } closure ? CoflowVm.ExecuteClosure<TResult>(closure) : _entry!.CompiledProgram is { } program
        ? CoflowVm.ExecuteBound<TResult>(program, _receiver!) : _entry.Invoke<TResult>();
    internal TResult Invoke<T1, TResult>(T1 a1) => _closure is { } closure ? CoflowVm.ExecuteClosure<T1, TResult>(closure, a1) : _entry!.CompiledProgram is { } program
        ? CoflowVm.ExecuteBound<T1, TResult>(program, _receiver!, a1) : _entry.Invoke<T1, TResult>(a1);
    internal TResult Invoke<T1, T2, TResult>(T1 a1, T2 a2) => _closure is { } closure ? CoflowVm.ExecuteClosure<T1, T2, TResult>(closure, a1, a2) : _entry!.CompiledProgram is { } program
        ? CoflowVm.ExecuteBound<T1, T2, TResult>(program, _receiver!, a1, a2) : _entry.Invoke<T1, T2, TResult>(a1, a2);
    internal TResult Invoke<T1, T2, T3, TResult>(T1 a1, T2 a2, T3 a3) => _closure is { } closure ? CoflowVm.ExecuteClosure<T1, T2, T3, TResult>(closure, a1, a2, a3) : _entry!.CompiledProgram is { } program
        ? CoflowVm.ExecuteBound<T1, T2, T3, TResult>(program, _receiver!, a1, a2, a3) : _entry.Invoke<T1, T2, T3, TResult>(a1, a2, a3);
    internal TResult Invoke<T1, T2, T3, T4, TResult>(T1 a1, T2 a2, T3 a3, T4 a4) => _closure is { } closure ? CoflowVm.ExecuteClosure<T1, T2, T3, T4, TResult>(closure, a1, a2, a3, a4) : _entry!.CompiledProgram is { } program
        ? CoflowVm.ExecuteBound<T1, T2, T3, T4, TResult>(program, _receiver!, a1, a2, a3, a4) : _entry.Invoke<T1, T2, T3, T4, TResult>(a1, a2, a3, a4);
    internal TResult Invoke<T1, T2, T3, T4, T5, TResult>(T1 a1, T2 a2, T3 a3, T4 a4, T5 a5) => _closure is { } closure ? CoflowVm.ExecuteClosure<T1, T2, T3, T4, T5, TResult>(closure, a1, a2, a3, a4, a5) : _entry!.CompiledProgram is { } program
        ? CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, TResult>(program, _receiver!, a1, a2, a3, a4, a5) : _entry.Invoke<T1, T2, T3, T4, T5, TResult>(a1, a2, a3, a4, a5);
    internal TResult Invoke<T1, T2, T3, T4, T5, T6, TResult>(T1 a1, T2 a2, T3 a3, T4 a4, T5 a5, T6 a6) => _closure is { } closure ? CoflowVm.ExecuteClosure<T1, T2, T3, T4, T5, T6, TResult>(closure, a1, a2, a3, a4, a5, a6) : _entry!.CompiledProgram is { } program
        ? CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, T6, TResult>(program, _receiver!, a1, a2, a3, a4, a5, a6) : _entry.Invoke<T1, T2, T3, T4, T5, T6, TResult>(a1, a2, a3, a4, a5, a6);
    internal TResult Invoke<T1, T2, T3, T4, T5, T6, T7, TResult>(T1 a1, T2 a2, T3 a3, T4 a4, T5 a5, T6 a6, T7 a7) => _closure is { } closure ? CoflowVm.ExecuteClosure<T1, T2, T3, T4, T5, T6, T7, TResult>(closure, a1, a2, a3, a4, a5, a6, a7) : _entry!.CompiledProgram is { } program
        ? CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, T6, T7, TResult>(program, _receiver!, a1, a2, a3, a4, a5, a6, a7) : _entry.Invoke<T1, T2, T3, T4, T5, T6, T7, TResult>(a1, a2, a3, a4, a5, a6, a7);
    internal TResult Invoke<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(T1 a1, T2 a2, T3 a3, T4 a4, T5 a5, T6 a6, T7 a7, T8 a8) => _closure is { } closure ? CoflowVm.ExecuteClosure<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(closure, a1, a2, a3, a4, a5, a6, a7, a8) : _entry!.CompiledProgram is { } program
        ? CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(program, _receiver!, a1, a2, a3, a4, a5, a6, a7, a8) : _entry.Invoke<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(a1, a2, a3, a4, a5, a6, a7, a8);
}
