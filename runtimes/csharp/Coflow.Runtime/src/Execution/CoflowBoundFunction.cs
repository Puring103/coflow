namespace Coflow.Runtime.CompilerServices;

internal readonly record struct CoflowBoundFunction(CoflowFunctionEntry Entry, object Receiver)
{
    // 生成方法统一落到这里；Program 进入 VM，Host 调用快照绑定的强类型 native delegate。
    internal TResult Invoke<TResult>()
    {
        return Entry.CompiledProgram is { } program
            ? CoflowVm.ExecuteBound<TResult>(program, Receiver)
            : Entry.Invoke<TResult>();
    }

    internal TResult Invoke<T1, TResult>(T1 arg1)
    {
        return Entry.CompiledProgram is { } program ? CoflowVm.ExecuteBound<T1, TResult>(program, Receiver, arg1) : Entry.Invoke<T1, TResult>(arg1);
    }

    internal TResult Invoke<T1, T2, TResult>(T1 arg1, T2 arg2)
    {
        return Entry.CompiledProgram is { } program ? CoflowVm.ExecuteBound<T1, T2, TResult>(program, Receiver, arg1, arg2) : Entry.Invoke<T1, T2, TResult>(arg1, arg2);
    }

    internal TResult Invoke<T1, T2, T3, TResult>(T1 arg1, T2 arg2, T3 arg3)
    {
        return Entry.CompiledProgram is { } program ? CoflowVm.ExecuteBound<T1, T2, T3, TResult>(program, Receiver, arg1, arg2, arg3) : Entry.Invoke<T1, T2, T3, TResult>(arg1, arg2, arg3);
    }

    internal TResult Invoke<T1, T2, T3, T4, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4)
    {
        return Entry.CompiledProgram is { } program ? CoflowVm.ExecuteBound<T1, T2, T3, T4, TResult>(program, Receiver, arg1, arg2, arg3, arg4) : Entry.Invoke<T1, T2, T3, T4, TResult>(arg1, arg2, arg3, arg4);
    }

    internal TResult Invoke<T1, T2, T3, T4, T5, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5)
    {
        return Entry.CompiledProgram is { } program ? CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, TResult>(program, Receiver, arg1, arg2, arg3, arg4, arg5) : Entry.Invoke<T1, T2, T3, T4, T5, TResult>(arg1, arg2, arg3, arg4, arg5);
    }

    internal TResult Invoke<T1, T2, T3, T4, T5, T6, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6)
    {
        return Entry.CompiledProgram is { } program ? CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, T6, TResult>(program, Receiver, arg1, arg2, arg3, arg4, arg5, arg6) : Entry.Invoke<T1, T2, T3, T4, T5, T6, TResult>(arg1, arg2, arg3, arg4, arg5, arg6);
    }

    internal TResult Invoke<T1, T2, T3, T4, T5, T6, T7, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7)
    {
        return Entry.CompiledProgram is { } program ? CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, T6, T7, TResult>(program, Receiver, arg1, arg2, arg3, arg4, arg5, arg6, arg7) : Entry.Invoke<T1, T2, T3, T4, T5, T6, T7, TResult>(arg1, arg2, arg3, arg4, arg5, arg6, arg7);
    }

    internal TResult Invoke<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7, T8 arg8)
    {
        return Entry.CompiledProgram is { } program ? CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(program, Receiver, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8) : Entry.Invoke<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8);
    }

    internal void InvokeVoid()
    {
        if (Entry.CompiledProgram is { } program) _ = CoflowVm.ExecuteBound<Unit>(program, Receiver); else Entry.InvokeVoid();
    }

    internal void InvokeVoid<T1>(T1 arg1)
    {
        if (Entry.CompiledProgram is { } program) _ = CoflowVm.ExecuteBound<T1, Unit>(program, Receiver, arg1); else Entry.InvokeVoid(arg1);
    }

    internal void InvokeVoid<T1, T2>(T1 arg1, T2 arg2)
    {
        if (Entry.CompiledProgram is { } program) _ = CoflowVm.ExecuteBound<T1, T2, Unit>(program, Receiver, arg1, arg2); else Entry.InvokeVoid(arg1, arg2);
    }

    internal void InvokeVoid<T1, T2, T3>(T1 arg1, T2 arg2, T3 arg3)
    {
        if (Entry.CompiledProgram is { } program) _ = CoflowVm.ExecuteBound<T1, T2, T3, Unit>(program, Receiver, arg1, arg2, arg3); else Entry.InvokeVoid(arg1, arg2, arg3);
    }

    internal void InvokeVoid<T1, T2, T3, T4>(T1 arg1, T2 arg2, T3 arg3, T4 arg4)
    {
        if (Entry.CompiledProgram is { } program) _ = CoflowVm.ExecuteBound<T1, T2, T3, T4, Unit>(program, Receiver, arg1, arg2, arg3, arg4); else Entry.InvokeVoid(arg1, arg2, arg3, arg4);
    }

    internal void InvokeVoid<T1, T2, T3, T4, T5>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5)
    {
        if (Entry.CompiledProgram is { } program) _ = CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, Unit>(program, Receiver, arg1, arg2, arg3, arg4, arg5); else Entry.InvokeVoid(arg1, arg2, arg3, arg4, arg5);
    }

    internal void InvokeVoid<T1, T2, T3, T4, T5, T6>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6)
    {
        if (Entry.CompiledProgram is { } program) _ = CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, T6, Unit>(program, Receiver, arg1, arg2, arg3, arg4, arg5, arg6); else Entry.InvokeVoid(arg1, arg2, arg3, arg4, arg5, arg6);
    }

    internal void InvokeVoid<T1, T2, T3, T4, T5, T6, T7>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7)
    {
        if (Entry.CompiledProgram is { } program) _ = CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, T6, T7, Unit>(program, Receiver, arg1, arg2, arg3, arg4, arg5, arg6, arg7); else Entry.InvokeVoid(arg1, arg2, arg3, arg4, arg5, arg6, arg7);
    }

    internal void InvokeVoid<T1, T2, T3, T4, T5, T6, T7, T8>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7, T8 arg8)
    {
        if (Entry.CompiledProgram is { } program) _ = CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, T6, T7, T8, Unit>(program, Receiver, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8); else Entry.InvokeVoid(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8);
    }
}
