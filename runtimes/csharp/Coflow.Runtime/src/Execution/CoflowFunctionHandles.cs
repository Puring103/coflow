using System;
using Coflow.Runtime.CompilerServices;

namespace Coflow.Runtime;

internal readonly struct CoflowFunctionId : IEquatable<CoflowFunctionId>
{
    private readonly ulong _packed;

    internal CoflowFunctionId(uint snapshotId, CoflowFunctionKind kind, int targetIndex)
    {
        if (kind == CoflowFunctionKind.Missing || targetIndex < 0 || targetIndex > 0x1fff_ffff)
            throw new ArgumentOutOfRangeException(nameof(targetIndex));
        _packed = ((ulong)snapshotId << 32) | ((uint)kind << 29) | (uint)targetIndex;
    }

    internal uint SnapshotId => (uint)(_packed >> 32);
    internal CoflowFunctionKind Kind => (CoflowFunctionKind)((_packed >> 29) & 0x7);
    internal int TargetIndex => (int)(_packed & 0x1fff_ffff);
    internal long Packed => unchecked((long)_packed);
    internal bool IsValid => _packed != 0;
    internal static CoflowFunctionId FromPacked(ulong packed) => new(packed);

    private CoflowFunctionId(ulong packed) => _packed = packed;
    public bool Equals(CoflowFunctionId other) => _packed == other._packed;
    public override bool Equals(object? obj) => obj is CoflowFunctionId other && Equals(other);
    public override int GetHashCode() => _packed.GetHashCode();
    public static bool operator ==(CoflowFunctionId left, CoflowFunctionId right) => left.Equals(right);
    public static bool operator !=(CoflowFunctionId left, CoflowFunctionId right) => !left.Equals(right);
}

internal enum CoflowFunctionKind : byte { Missing, Program, Native, Closure }

internal interface ICoflowFunctionHandle
{
    CoflowFunctionId FunctionId { get; }
    CoflowValueId EnvironmentId { get; }
}

public readonly struct CoflowFunction<TResult> : ICoflowFunctionHandle
{
    internal CoflowFunction(CoflowFunctionId functionId, CoflowValueId environmentId) { FunctionId = functionId; EnvironmentId = environmentId; }
    CoflowFunctionId ICoflowFunctionHandle.FunctionId => FunctionId;
    CoflowValueId ICoflowFunctionHandle.EnvironmentId => EnvironmentId;
    internal CoflowFunctionId FunctionId { get; }
    internal CoflowValueId EnvironmentId { get; }
    public TResult Invoke(Coflow coflow) => coflow.InvokeFunction<CoflowArguments0, TResult>(FunctionId, EnvironmentId, default);
}

public readonly struct CoflowFunction<T1, TResult> : ICoflowFunctionHandle
{
    internal CoflowFunction(CoflowFunctionId functionId, CoflowValueId environmentId) { FunctionId = functionId; EnvironmentId = environmentId; }
    CoflowFunctionId ICoflowFunctionHandle.FunctionId => FunctionId; CoflowValueId ICoflowFunctionHandle.EnvironmentId => EnvironmentId;
    internal CoflowFunctionId FunctionId { get; } internal CoflowValueId EnvironmentId { get; }
    public TResult Invoke(Coflow coflow, T1 arg1) => coflow.InvokeFunction<CoflowArguments1<T1>, TResult>(FunctionId, EnvironmentId, new(arg1));
}

public readonly struct CoflowFunction<T1, T2, TResult> : ICoflowFunctionHandle
{
    internal CoflowFunction(CoflowFunctionId functionId, CoflowValueId environmentId) { FunctionId = functionId; EnvironmentId = environmentId; }
    CoflowFunctionId ICoflowFunctionHandle.FunctionId => FunctionId; CoflowValueId ICoflowFunctionHandle.EnvironmentId => EnvironmentId;
    internal CoflowFunctionId FunctionId { get; } internal CoflowValueId EnvironmentId { get; }
    public TResult Invoke(Coflow coflow, T1 arg1, T2 arg2) => coflow.InvokeFunction<CoflowArguments2<T1, T2>, TResult>(FunctionId, EnvironmentId, new(arg1, arg2));
}

public readonly struct CoflowFunction<T1, T2, T3, TResult> : ICoflowFunctionHandle
{
    internal CoflowFunction(CoflowFunctionId functionId, CoflowValueId environmentId) { FunctionId = functionId; EnvironmentId = environmentId; }
    CoflowFunctionId ICoflowFunctionHandle.FunctionId => FunctionId; CoflowValueId ICoflowFunctionHandle.EnvironmentId => EnvironmentId;
    internal CoflowFunctionId FunctionId { get; } internal CoflowValueId EnvironmentId { get; }
    public TResult Invoke(Coflow coflow, T1 arg1, T2 arg2, T3 arg3) => coflow.InvokeFunction<CoflowArguments3<T1, T2, T3>, TResult>(FunctionId, EnvironmentId, new(arg1, arg2, arg3));
}

public readonly struct CoflowFunction<T1, T2, T3, T4, TResult> : ICoflowFunctionHandle
{
    internal CoflowFunction(CoflowFunctionId functionId, CoflowValueId environmentId) { FunctionId = functionId; EnvironmentId = environmentId; }
    CoflowFunctionId ICoflowFunctionHandle.FunctionId => FunctionId; CoflowValueId ICoflowFunctionHandle.EnvironmentId => EnvironmentId;
    internal CoflowFunctionId FunctionId { get; } internal CoflowValueId EnvironmentId { get; }
    public TResult Invoke(Coflow coflow, T1 arg1, T2 arg2, T3 arg3, T4 arg4) => coflow.InvokeFunction<CoflowArguments4<T1, T2, T3, T4>, TResult>(FunctionId, EnvironmentId, new(arg1, arg2, arg3, arg4));
}

public readonly struct CoflowFunction<T1, T2, T3, T4, T5, TResult> : ICoflowFunctionHandle
{
    internal CoflowFunction(CoflowFunctionId functionId, CoflowValueId environmentId) { FunctionId = functionId; EnvironmentId = environmentId; }
    CoflowFunctionId ICoflowFunctionHandle.FunctionId => FunctionId; CoflowValueId ICoflowFunctionHandle.EnvironmentId => EnvironmentId;
    internal CoflowFunctionId FunctionId { get; } internal CoflowValueId EnvironmentId { get; }
    public TResult Invoke(Coflow coflow, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5) => coflow.InvokeFunction<CoflowArguments5<T1, T2, T3, T4, T5>, TResult>(FunctionId, EnvironmentId, new(arg1, arg2, arg3, arg4, arg5));
}

public readonly struct CoflowFunction<T1, T2, T3, T4, T5, T6, TResult> : ICoflowFunctionHandle
{
    internal CoflowFunction(CoflowFunctionId functionId, CoflowValueId environmentId) { FunctionId = functionId; EnvironmentId = environmentId; }
    CoflowFunctionId ICoflowFunctionHandle.FunctionId => FunctionId; CoflowValueId ICoflowFunctionHandle.EnvironmentId => EnvironmentId;
    internal CoflowFunctionId FunctionId { get; } internal CoflowValueId EnvironmentId { get; }
    public TResult Invoke(Coflow coflow, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6) => coflow.InvokeFunction<CoflowArguments6<T1, T2, T3, T4, T5, T6>, TResult>(FunctionId, EnvironmentId, new(arg1, arg2, arg3, arg4, arg5, arg6));
}

public readonly struct CoflowFunction<T1, T2, T3, T4, T5, T6, T7, TResult> : ICoflowFunctionHandle
{
    internal CoflowFunction(CoflowFunctionId functionId, CoflowValueId environmentId) { FunctionId = functionId; EnvironmentId = environmentId; }
    CoflowFunctionId ICoflowFunctionHandle.FunctionId => FunctionId; CoflowValueId ICoflowFunctionHandle.EnvironmentId => EnvironmentId;
    internal CoflowFunctionId FunctionId { get; } internal CoflowValueId EnvironmentId { get; }
    public TResult Invoke(Coflow coflow, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7) => coflow.InvokeFunction<CoflowArguments7<T1, T2, T3, T4, T5, T6, T7>, TResult>(FunctionId, EnvironmentId, new(arg1, arg2, arg3, arg4, arg5, arg6, arg7));
}

public readonly struct CoflowFunction<T1, T2, T3, T4, T5, T6, T7, T8, TResult> : ICoflowFunctionHandle
{
    internal CoflowFunction(CoflowFunctionId functionId, CoflowValueId environmentId) { FunctionId = functionId; EnvironmentId = environmentId; }
    CoflowFunctionId ICoflowFunctionHandle.FunctionId => FunctionId; CoflowValueId ICoflowFunctionHandle.EnvironmentId => EnvironmentId;
    internal CoflowFunctionId FunctionId { get; } internal CoflowValueId EnvironmentId { get; }
    public TResult Invoke(Coflow coflow, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7, T8 arg8) => coflow.InvokeFunction<CoflowArguments8<T1, T2, T3, T4, T5, T6, T7, T8>, TResult>(FunctionId, EnvironmentId, new(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8));
}
