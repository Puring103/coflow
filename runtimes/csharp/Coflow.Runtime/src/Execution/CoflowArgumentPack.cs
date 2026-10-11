using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
using System.Runtime.InteropServices;

namespace Coflow.Runtime.CompilerServices
{

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

internal readonly struct CoflowArguments1<T1> : ICoflowArgumentPack
{
    public T1 Arg1 { get; init; }

    public CoflowArguments1(T1 Arg1)
    {
        this.Arg1 = Arg1;
    }

    public int Count => 1;
    public void Write(CoflowExecutionSession session) => session.Write(session.Parameter(0), Arg1);
    public TResult InvokeHost<TResult>(Delegate implementation) => ((Func<T1, TResult>)implementation)(Arg1);
    public void InvokeHostVoid(Delegate implementation) => ((Action<T1>)implementation)(Arg1);
}

internal readonly struct CoflowArguments2<T1, T2> : ICoflowArgumentPack
{
    public T1 Arg1 { get; init; }
    public T2 Arg2 { get; init; }

    public CoflowArguments2(T1 Arg1, T2 Arg2)
    {
        this.Arg1 = Arg1;
        this.Arg2 = Arg2;
    }

    public int Count => 2;
    public void Write(CoflowExecutionSession session) { session.Write(session.Parameter(0), Arg1); session.Write(session.Parameter(1), Arg2); }
    public TResult InvokeHost<TResult>(Delegate implementation) => ((Func<T1, T2, TResult>)implementation)(Arg1, Arg2);
    public void InvokeHostVoid(Delegate implementation) => ((Action<T1, T2>)implementation)(Arg1, Arg2);
}

internal readonly struct CoflowArguments3<T1, T2, T3> : ICoflowArgumentPack
{
    public T1 Arg1 { get; init; }
    public T2 Arg2 { get; init; }
    public T3 Arg3 { get; init; }

    public CoflowArguments3(T1 Arg1, T2 Arg2, T3 Arg3)
    {
        this.Arg1 = Arg1;
        this.Arg2 = Arg2;
        this.Arg3 = Arg3;
    }

    public int Count => 3;
    public void Write(CoflowExecutionSession session) { session.Write(session.Parameter(0), Arg1); session.Write(session.Parameter(1), Arg2); session.Write(session.Parameter(2), Arg3); }
    public TResult InvokeHost<TResult>(Delegate implementation) => ((Func<T1, T2, T3, TResult>)implementation)(Arg1, Arg2, Arg3);
    public void InvokeHostVoid(Delegate implementation) => ((Action<T1, T2, T3>)implementation)(Arg1, Arg2, Arg3);
}

internal readonly struct CoflowArguments4<T1, T2, T3, T4> : ICoflowArgumentPack
{
    public T1 Arg1 { get; init; }
    public T2 Arg2 { get; init; }
    public T3 Arg3 { get; init; }
    public T4 Arg4 { get; init; }

    public CoflowArguments4(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4)
    {
        this.Arg1 = Arg1;
        this.Arg2 = Arg2;
        this.Arg3 = Arg3;
        this.Arg4 = Arg4;
    }

    public int Count => 4;
    public void Write(CoflowExecutionSession session) { session.Write(session.Parameter(0), Arg1); session.Write(session.Parameter(1), Arg2); session.Write(session.Parameter(2), Arg3); session.Write(session.Parameter(3), Arg4); }
    public TResult InvokeHost<TResult>(Delegate implementation) => ((Func<T1, T2, T3, T4, TResult>)implementation)(Arg1, Arg2, Arg3, Arg4);
    public void InvokeHostVoid(Delegate implementation) => ((Action<T1, T2, T3, T4>)implementation)(Arg1, Arg2, Arg3, Arg4);
}

internal readonly struct CoflowArguments5<T1, T2, T3, T4, T5> : ICoflowArgumentPack
{
    public T1 Arg1 { get; init; }
    public T2 Arg2 { get; init; }
    public T3 Arg3 { get; init; }
    public T4 Arg4 { get; init; }
    public T5 Arg5 { get; init; }

    public CoflowArguments5(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4, T5 Arg5)
    {
        this.Arg1 = Arg1;
        this.Arg2 = Arg2;
        this.Arg3 = Arg3;
        this.Arg4 = Arg4;
        this.Arg5 = Arg5;
    }

    public int Count => 5;
    public void Write(CoflowExecutionSession session) { session.Write(session.Parameter(0), Arg1); session.Write(session.Parameter(1), Arg2); session.Write(session.Parameter(2), Arg3); session.Write(session.Parameter(3), Arg4); session.Write(session.Parameter(4), Arg5); }
    public TResult InvokeHost<TResult>(Delegate implementation) => ((Func<T1, T2, T3, T4, T5, TResult>)implementation)(Arg1, Arg2, Arg3, Arg4, Arg5);
    public void InvokeHostVoid(Delegate implementation) => ((Action<T1, T2, T3, T4, T5>)implementation)(Arg1, Arg2, Arg3, Arg4, Arg5);
}

internal readonly struct CoflowArguments6<T1, T2, T3, T4, T5, T6> : ICoflowArgumentPack
{
    public T1 Arg1 { get; init; }
    public T2 Arg2 { get; init; }
    public T3 Arg3 { get; init; }
    public T4 Arg4 { get; init; }
    public T5 Arg5 { get; init; }
    public T6 Arg6 { get; init; }

    public CoflowArguments6(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4, T5 Arg5, T6 Arg6)
    {
        this.Arg1 = Arg1;
        this.Arg2 = Arg2;
        this.Arg3 = Arg3;
        this.Arg4 = Arg4;
        this.Arg5 = Arg5;
        this.Arg6 = Arg6;
    }

    public int Count => 6;
    public void Write(CoflowExecutionSession session) { session.Write(session.Parameter(0), Arg1); session.Write(session.Parameter(1), Arg2); session.Write(session.Parameter(2), Arg3); session.Write(session.Parameter(3), Arg4); session.Write(session.Parameter(4), Arg5); session.Write(session.Parameter(5), Arg6); }
    public TResult InvokeHost<TResult>(Delegate implementation) => ((Func<T1, T2, T3, T4, T5, T6, TResult>)implementation)(Arg1, Arg2, Arg3, Arg4, Arg5, Arg6);
    public void InvokeHostVoid(Delegate implementation) => ((Action<T1, T2, T3, T4, T5, T6>)implementation)(Arg1, Arg2, Arg3, Arg4, Arg5, Arg6);
}

internal readonly struct CoflowArguments7<T1, T2, T3, T4, T5, T6, T7> : ICoflowArgumentPack
{
    public T1 Arg1 { get; init; }
    public T2 Arg2 { get; init; }
    public T3 Arg3 { get; init; }
    public T4 Arg4 { get; init; }
    public T5 Arg5 { get; init; }
    public T6 Arg6 { get; init; }
    public T7 Arg7 { get; init; }

    public CoflowArguments7(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4, T5 Arg5, T6 Arg6, T7 Arg7)
    {
        this.Arg1 = Arg1;
        this.Arg2 = Arg2;
        this.Arg3 = Arg3;
        this.Arg4 = Arg4;
        this.Arg5 = Arg5;
        this.Arg6 = Arg6;
        this.Arg7 = Arg7;
    }

    public int Count => 7;
    public void Write(CoflowExecutionSession session) { session.Write(session.Parameter(0), Arg1); session.Write(session.Parameter(1), Arg2); session.Write(session.Parameter(2), Arg3); session.Write(session.Parameter(3), Arg4); session.Write(session.Parameter(4), Arg5); session.Write(session.Parameter(5), Arg6); session.Write(session.Parameter(6), Arg7); }
    public TResult InvokeHost<TResult>(Delegate implementation) => ((Func<T1, T2, T3, T4, T5, T6, T7, TResult>)implementation)(Arg1, Arg2, Arg3, Arg4, Arg5, Arg6, Arg7);
    public void InvokeHostVoid(Delegate implementation) => ((Action<T1, T2, T3, T4, T5, T6, T7>)implementation)(Arg1, Arg2, Arg3, Arg4, Arg5, Arg6, Arg7);
}

internal readonly struct CoflowArguments8<T1, T2, T3, T4, T5, T6, T7, T8> : ICoflowArgumentPack
{
    public T1 Arg1 { get; init; }
    public T2 Arg2 { get; init; }
    public T3 Arg3 { get; init; }
    public T4 Arg4 { get; init; }
    public T5 Arg5 { get; init; }
    public T6 Arg6 { get; init; }
    public T7 Arg7 { get; init; }
    public T8 Arg8 { get; init; }

    public CoflowArguments8(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4, T5 Arg5, T6 Arg6, T7 Arg7, T8 Arg8)
    {
        this.Arg1 = Arg1;
        this.Arg2 = Arg2;
        this.Arg3 = Arg3;
        this.Arg4 = Arg4;
        this.Arg5 = Arg5;
        this.Arg6 = Arg6;
        this.Arg7 = Arg7;
        this.Arg8 = Arg8;
    }

    public int Count => 8;
    public void Write(CoflowExecutionSession session) { session.Write(session.Parameter(0), Arg1); session.Write(session.Parameter(1), Arg2); session.Write(session.Parameter(2), Arg3); session.Write(session.Parameter(3), Arg4); session.Write(session.Parameter(4), Arg5); session.Write(session.Parameter(5), Arg6); session.Write(session.Parameter(6), Arg7); session.Write(session.Parameter(7), Arg8); }
    public TResult InvokeHost<TResult>(Delegate implementation) => ((Func<T1, T2, T3, T4, T5, T6, T7, T8, TResult>)implementation)(Arg1, Arg2, Arg3, Arg4, Arg5, Arg6, Arg7, Arg8);
    public void InvokeHostVoid(Delegate implementation) => ((Action<T1, T2, T3, T4, T5, T6, T7, T8>)implementation)(Arg1, Arg2, Arg3, Arg4, Arg5, Arg6, Arg7, Arg8);
}

internal readonly struct CoflowRawArguments1<T1> : ICoflowArgumentPack
{
    public T1 Arg1 { get; init; }

    public CoflowRawArguments1(T1 Arg1)
    {
        this.Arg1 = Arg1;
    }

    public int Count => 1;
    public void Write(CoflowExecutionSession session) =>
        CoflowBoundaryCodec<T1>.WriteRelative(session, session.Program.RegisterProgram.Parameters[0], Arg1);
    public TResult InvokeHost<TResult>(Delegate implementation) => throw new InvalidOperationException("Raw arguments cannot invoke a Host delegate.");
    public void InvokeHostVoid(Delegate implementation) => throw new InvalidOperationException("Raw arguments cannot invoke a Host delegate.");
}

internal readonly struct CoflowReceiverArguments<TReceiver, TArguments> : ICoflowArgumentPack
    where TArguments : struct, ICoflowArgumentPack
{
    public TReceiver Receiver { get; init; }
    public TArguments Arguments { get; init; }

    public CoflowReceiverArguments(TReceiver Receiver, TArguments Arguments)
    {
        this.Receiver = Receiver;
        this.Arguments = Arguments;
    }

    public int Count => Arguments.Count + 1;
    public void Write(CoflowExecutionSession session) { Arguments.Write(session); session.Write(session.Parameter(Arguments.Count), Receiver); }
    public TResult InvokeHost<TResult>(Delegate implementation) => throw new InvalidOperationException("Receiver arguments cannot invoke a Host delegate.");
    public void InvokeHostVoid(Delegate implementation) => throw new InvalidOperationException("Receiver arguments cannot invoke a Host delegate.");
}

internal readonly struct CoflowBoxedReceiverArguments<TArguments> : ICoflowArgumentPack
    where TArguments : struct, ICoflowArgumentPack
{
    public object Receiver { get; init; }
    public TArguments Arguments { get; init; }

    public CoflowBoxedReceiverArguments(object Receiver, TArguments Arguments)
    {
        this.Receiver = Receiver;
        this.Arguments = Arguments;
    }

    public int Count => Arguments.Count + 1;
    public void Write(CoflowExecutionSession session)
    {
        Arguments.Write(session);
        session.WriteBoxed(session.Parameter(Arguments.Count), Receiver);
    }
    public TResult InvokeHost<TResult>(Delegate implementation) => throw new InvalidOperationException("Receiver arguments cannot invoke a Host delegate.");
    public void InvokeHostVoid(Delegate implementation) => throw new InvalidOperationException("Receiver arguments cannot invoke a Host delegate.");
}

internal readonly struct CoflowClosureArguments<TArguments> : ICoflowArgumentPack
    where TArguments : struct, ICoflowArgumentPack
{
    public CoflowClosure Closure { get; init; }
    public TArguments Arguments { get; init; }

    public CoflowClosureArguments(CoflowClosure Closure, TArguments Arguments)
    {
        this.Closure = Closure;
        this.Arguments = Arguments;
    }

    public int Count => Arguments.Count + Closure.Captures.Count;
    public void Write(CoflowExecutionSession session) { Arguments.Write(session); session.WriteCaptures(Closure, Arguments.Count); }
    public TResult InvokeHost<TResult>(Delegate implementation) => throw new InvalidOperationException("Closure arguments cannot invoke a Host delegate.");
    public void InvokeHostVoid(Delegate implementation) => throw new InvalidOperationException("Closure arguments cannot invoke a Host delegate.");
}
}
