using System;
using System.Runtime.InteropServices;

namespace Coflow.Runtime.CompilerServices;

/// <summary>统一调用参数协议；值类型参数包避免在预热调用路径创建数组或装箱参数。</summary>
internal interface ICoflowArgumentPack
{
    int Count { get; }
    void Write(CoflowExecutionSession session);
    TResult InvokeHost<TResult>(Delegate implementation);
    void InvokeHostVoid(Delegate implementation);
}

[StructLayout(LayoutKind.Sequential, Size = 1)]
internal readonly struct CoflowArguments0 : ICoflowArgumentPack
{
    public int Count => 0;
    public void Write(CoflowExecutionSession session) { }
    public TResult InvokeHost<TResult>(Delegate implementation) => ((Func<TResult>)implementation)();
    public void InvokeHostVoid(Delegate implementation) => ((Action)implementation)();
}

internal readonly record struct CoflowArguments1<T1>(T1 Arg1) : ICoflowArgumentPack
{
    public int Count => 1;
    public void Write(CoflowExecutionSession session) => session.Write(session.Parameter(0), Arg1);
    public TResult InvokeHost<TResult>(Delegate implementation) => ((Func<T1, TResult>)implementation)(Arg1);
    public void InvokeHostVoid(Delegate implementation) => ((Action<T1>)implementation)(Arg1);
}

internal readonly record struct CoflowArguments2<T1, T2>(T1 Arg1, T2 Arg2) : ICoflowArgumentPack
{
    public int Count => 2;
    public void Write(CoflowExecutionSession session) { session.Write(session.Parameter(0), Arg1); session.Write(session.Parameter(1), Arg2); }
    public TResult InvokeHost<TResult>(Delegate implementation) => ((Func<T1, T2, TResult>)implementation)(Arg1, Arg2);
    public void InvokeHostVoid(Delegate implementation) => ((Action<T1, T2>)implementation)(Arg1, Arg2);
}

internal readonly record struct CoflowArguments3<T1, T2, T3>(T1 Arg1, T2 Arg2, T3 Arg3) : ICoflowArgumentPack
{
    public int Count => 3;
    public void Write(CoflowExecutionSession session) { session.Write(session.Parameter(0), Arg1); session.Write(session.Parameter(1), Arg2); session.Write(session.Parameter(2), Arg3); }
    public TResult InvokeHost<TResult>(Delegate implementation) => ((Func<T1, T2, T3, TResult>)implementation)(Arg1, Arg2, Arg3);
    public void InvokeHostVoid(Delegate implementation) => ((Action<T1, T2, T3>)implementation)(Arg1, Arg2, Arg3);
}

internal readonly record struct CoflowArguments4<T1, T2, T3, T4>(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4) : ICoflowArgumentPack
{
    public int Count => 4;
    public void Write(CoflowExecutionSession session) { session.Write(session.Parameter(0), Arg1); session.Write(session.Parameter(1), Arg2); session.Write(session.Parameter(2), Arg3); session.Write(session.Parameter(3), Arg4); }
    public TResult InvokeHost<TResult>(Delegate implementation) => ((Func<T1, T2, T3, T4, TResult>)implementation)(Arg1, Arg2, Arg3, Arg4);
    public void InvokeHostVoid(Delegate implementation) => ((Action<T1, T2, T3, T4>)implementation)(Arg1, Arg2, Arg3, Arg4);
}

internal readonly record struct CoflowArguments5<T1, T2, T3, T4, T5>(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4, T5 Arg5) : ICoflowArgumentPack
{
    public int Count => 5;
    public void Write(CoflowExecutionSession session) { session.Write(session.Parameter(0), Arg1); session.Write(session.Parameter(1), Arg2); session.Write(session.Parameter(2), Arg3); session.Write(session.Parameter(3), Arg4); session.Write(session.Parameter(4), Arg5); }
    public TResult InvokeHost<TResult>(Delegate implementation) => ((Func<T1, T2, T3, T4, T5, TResult>)implementation)(Arg1, Arg2, Arg3, Arg4, Arg5);
    public void InvokeHostVoid(Delegate implementation) => ((Action<T1, T2, T3, T4, T5>)implementation)(Arg1, Arg2, Arg3, Arg4, Arg5);
}

internal readonly record struct CoflowArguments6<T1, T2, T3, T4, T5, T6>(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4, T5 Arg5, T6 Arg6) : ICoflowArgumentPack
{
    public int Count => 6;
    public void Write(CoflowExecutionSession session) { session.Write(session.Parameter(0), Arg1); session.Write(session.Parameter(1), Arg2); session.Write(session.Parameter(2), Arg3); session.Write(session.Parameter(3), Arg4); session.Write(session.Parameter(4), Arg5); session.Write(session.Parameter(5), Arg6); }
    public TResult InvokeHost<TResult>(Delegate implementation) => ((Func<T1, T2, T3, T4, T5, T6, TResult>)implementation)(Arg1, Arg2, Arg3, Arg4, Arg5, Arg6);
    public void InvokeHostVoid(Delegate implementation) => ((Action<T1, T2, T3, T4, T5, T6>)implementation)(Arg1, Arg2, Arg3, Arg4, Arg5, Arg6);
}

internal readonly record struct CoflowArguments7<T1, T2, T3, T4, T5, T6, T7>(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4, T5 Arg5, T6 Arg6, T7 Arg7) : ICoflowArgumentPack
{
    public int Count => 7;
    public void Write(CoflowExecutionSession session) { session.Write(session.Parameter(0), Arg1); session.Write(session.Parameter(1), Arg2); session.Write(session.Parameter(2), Arg3); session.Write(session.Parameter(3), Arg4); session.Write(session.Parameter(4), Arg5); session.Write(session.Parameter(5), Arg6); session.Write(session.Parameter(6), Arg7); }
    public TResult InvokeHost<TResult>(Delegate implementation) => ((Func<T1, T2, T3, T4, T5, T6, T7, TResult>)implementation)(Arg1, Arg2, Arg3, Arg4, Arg5, Arg6, Arg7);
    public void InvokeHostVoid(Delegate implementation) => ((Action<T1, T2, T3, T4, T5, T6, T7>)implementation)(Arg1, Arg2, Arg3, Arg4, Arg5, Arg6, Arg7);
}

internal readonly record struct CoflowArguments8<T1, T2, T3, T4, T5, T6, T7, T8>(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4, T5 Arg5, T6 Arg6, T7 Arg7, T8 Arg8) : ICoflowArgumentPack
{
    public int Count => 8;
    public void Write(CoflowExecutionSession session) { session.Write(session.Parameter(0), Arg1); session.Write(session.Parameter(1), Arg2); session.Write(session.Parameter(2), Arg3); session.Write(session.Parameter(3), Arg4); session.Write(session.Parameter(4), Arg5); session.Write(session.Parameter(5), Arg6); session.Write(session.Parameter(6), Arg7); session.Write(session.Parameter(7), Arg8); }
    public TResult InvokeHost<TResult>(Delegate implementation) => ((Func<T1, T2, T3, T4, T5, T6, T7, T8, TResult>)implementation)(Arg1, Arg2, Arg3, Arg4, Arg5, Arg6, Arg7, Arg8);
    public void InvokeHostVoid(Delegate implementation) => ((Action<T1, T2, T3, T4, T5, T6, T7, T8>)implementation)(Arg1, Arg2, Arg3, Arg4, Arg5, Arg6, Arg7, Arg8);
}

internal readonly record struct CoflowRawArguments1<T1>(T1 Arg1) : ICoflowArgumentPack
{
    public int Count => 1;
    public void Write(CoflowExecutionSession session) =>
        CoflowBoundaryCodec<T1>.WriteRelative(session, session.Program.RegisterProgram.Parameters[0], Arg1);
    public TResult InvokeHost<TResult>(Delegate implementation) => throw new InvalidOperationException("Raw arguments cannot invoke a Host delegate.");
    public void InvokeHostVoid(Delegate implementation) => throw new InvalidOperationException("Raw arguments cannot invoke a Host delegate.");
}

internal readonly record struct CoflowReceiverArguments<TReceiver, TArguments>(TReceiver Receiver, TArguments Arguments) : ICoflowArgumentPack
    where TArguments : struct, ICoflowArgumentPack
{
    public int Count => Arguments.Count + 1;
    public void Write(CoflowExecutionSession session) { Arguments.Write(session); session.Write(session.Parameter(Arguments.Count), Receiver); }
    public TResult InvokeHost<TResult>(Delegate implementation) => throw new InvalidOperationException("Receiver arguments cannot invoke a Host delegate.");
    public void InvokeHostVoid(Delegate implementation) => throw new InvalidOperationException("Receiver arguments cannot invoke a Host delegate.");
}

internal readonly record struct CoflowBoxedReceiverArguments<TArguments>(object Receiver, TArguments Arguments) : ICoflowArgumentPack
    where TArguments : struct, ICoflowArgumentPack
{
    public int Count => Arguments.Count + 1;
    public void Write(CoflowExecutionSession session)
    {
        Arguments.Write(session);
        session.WriteBoxed(session.Parameter(Arguments.Count), Receiver);
    }
    public TResult InvokeHost<TResult>(Delegate implementation) => throw new InvalidOperationException("Receiver arguments cannot invoke a Host delegate.");
    public void InvokeHostVoid(Delegate implementation) => throw new InvalidOperationException("Receiver arguments cannot invoke a Host delegate.");
}

internal readonly record struct CoflowClosureArguments<TArguments>(CoflowClosure Closure, TArguments Arguments) : ICoflowArgumentPack
    where TArguments : struct, ICoflowArgumentPack
{
    public int Count => Arguments.Count + Closure.Captures.Count;
    public void Write(CoflowExecutionSession session) { Arguments.Write(session); session.WriteCaptures(Closure, Arguments.Count); }
    public TResult InvokeHost<TResult>(Delegate implementation) => throw new InvalidOperationException("Closure arguments cannot invoke a Host delegate.");
    public void InvokeHostVoid(Delegate implementation) => throw new InvalidOperationException("Closure arguments cannot invoke a Host delegate.");
}
