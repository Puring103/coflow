namespace Coflow.Runtime.CompilerServices;

using System.ComponentModel;

[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CoflowFunctionEntry
{
    private Delegate? _implementation;
    private CoflowNativeCall? _hostCall;
    private CoflowProgram? _compiled;
    private int _programIndex = -1;
    private int _targetIndex = -1;
    private readonly Type[] _vmParameterTypes;

    internal CoflowFunctionEntry(
        CoflowFunctionIdentity identity,
        CoflowFunctionSignature signature,
        Type receiverType,
        CfdFunctionValue? source,
        string sourcePath,
        CfdSpan? sourceSpan,
        bool requiresCfdBody = false,
        bool isDefault = false,
        long moduleId = 0)
    {
        Identity = identity;
        Signature = signature ?? throw new ArgumentNullException(nameof(signature));
        ReceiverType = receiverType ?? throw new ArgumentNullException(nameof(receiverType));
        Source = source;
        SourcePath = sourcePath ?? throw new ArgumentNullException(nameof(sourcePath));
        SourceSpan = sourceSpan;
        RequiresCfdBody = requiresCfdBody;
        IsDefault = isDefault;
        ModuleId = moduleId;
        _vmParameterTypes = source is null && !requiresCfdBody
            ? signature.ParameterTypes.ToArray()
            : signature.ParameterTypes.Concat(new[] { receiverType }).ToArray();
    }

    internal CoflowFunctionIdentity Identity { get; }
    internal CoflowFunctionSignature Signature { get; }
    internal Type ReceiverType { get; }
    internal IReadOnlyList<Type> VmParameterTypes => _vmParameterTypes;
    internal CfdFunctionValue? Source { get; }
    internal string SourcePath { get; }
    internal CfdSpan? SourceSpan { get; }
    internal bool RequiresCfdBody { get; }
    internal bool IsDefault { get; }
    internal long ModuleId { get; }
    internal object? Owner { get; set; }
    internal CoflowProgram? CompiledProgram => _compiled;
    internal bool HasBoundImplementation => _implementation is not null;
    internal int ProgramIndex => _programIndex >= 0
        ? _programIndex
        : throw new InvalidOperationException("The function has not been assigned a snapshot program index.");
    internal int TargetIndex => _targetIndex >= 0
        ? _targetIndex
        : throw new InvalidOperationException("The function has not been assigned a snapshot target index.");

    internal void AssignTargetIndex(int targetIndex)
    {
        if (targetIndex < 0) throw new ArgumentOutOfRangeException(nameof(targetIndex));
        if (_targetIndex >= 0) throw new InvalidOperationException("The function already has a snapshot target index.");
        _targetIndex = targetIndex;
    }

    internal void AssignProgramIndex(int programIndex)
    {
        if (programIndex < 0) throw new ArgumentOutOfRangeException(nameof(programIndex));
        if (_programIndex >= 0) throw new InvalidOperationException("The function already has a snapshot program index.");
        _programIndex = programIndex;
    }

    internal void ConfigureHost(Delegate implementation, CoflowNativeCall? call = null)
    {
        if (implementation is null) throw new ArgumentNullException(nameof(implementation));
        if (Source is not null || _hostCall is not null)
            throw new InvalidOperationException("Only an unbound Host function can receive a native implementation.");
        _implementation = implementation;
        _hostCall = call ?? new CoflowNativeCall(implementation);
    }

    internal void PublishCompiled(CoflowProgram? implementation) => _compiled = implementation;

    public TResult Invoke<TResult>() => _compiled is { } program
        ? CoflowVm.ExecuteBound<TResult>(program, RequiredOwner())
        : InvokeHost(() => Host<Func<TResult>>()());
    public TResult Invoke<T1, TResult>(T1 arg1) => _compiled is { } program
        ? CoflowVm.ExecuteBound<T1, TResult>(program, RequiredOwner(), arg1)
        : InvokeHost(() => Host<Func<T1, TResult>>()(arg1));
    public TResult Invoke<T1, T2, TResult>(T1 arg1, T2 arg2) => _compiled is { } program
        ? CoflowVm.ExecuteBound<T1, T2, TResult>(program, RequiredOwner(), arg1, arg2)
        : InvokeHost(() => Host<Func<T1, T2, TResult>>()(arg1, arg2));
    public TResult Invoke<T1, T2, T3, TResult>(T1 arg1, T2 arg2, T3 arg3) => _compiled is { } program
        ? CoflowVm.ExecuteBound<T1, T2, T3, TResult>(program, RequiredOwner(), arg1, arg2, arg3)
        : InvokeHost(() => Host<Func<T1, T2, T3, TResult>>()(arg1, arg2, arg3));
    public TResult Invoke<T1, T2, T3, T4, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4) => _compiled is { } program
        ? CoflowVm.ExecuteBound<T1, T2, T3, T4, TResult>(program, RequiredOwner(), arg1, arg2, arg3, arg4)
        : InvokeHost(() => Host<Func<T1, T2, T3, T4, TResult>>()(arg1, arg2, arg3, arg4));
    public TResult Invoke<T1, T2, T3, T4, T5, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5) => _compiled is { } program
        ? CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, TResult>(program, RequiredOwner(), arg1, arg2, arg3, arg4, arg5)
        : InvokeHost(() => Host<Func<T1, T2, T3, T4, T5, TResult>>()(arg1, arg2, arg3, arg4, arg5));
    public TResult Invoke<T1, T2, T3, T4, T5, T6, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6) => _compiled is { } program
        ? CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, T6, TResult>(program, RequiredOwner(), arg1, arg2, arg3, arg4, arg5, arg6)
        : InvokeHost(() => Host<Func<T1, T2, T3, T4, T5, T6, TResult>>()(arg1, arg2, arg3, arg4, arg5, arg6));
    public TResult Invoke<T1, T2, T3, T4, T5, T6, T7, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7) => _compiled is { } program
        ? CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, T6, T7, TResult>(program, RequiredOwner(), arg1, arg2, arg3, arg4, arg5, arg6, arg7)
        : InvokeHost(() => Host<Func<T1, T2, T3, T4, T5, T6, T7, TResult>>()(arg1, arg2, arg3, arg4, arg5, arg6, arg7));
    public TResult Invoke<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7, T8 arg8) => _compiled is { } program
        ? CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(program, RequiredOwner(), arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8)
        : InvokeHost(() => Host<Func<T1, T2, T3, T4, T5, T6, T7, T8, TResult>>()(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8));

    public void InvokeVoid() { if (_compiled is { } program) _ = CoflowVm.ExecuteBound<Unit>(program, RequiredOwner()); else InvokeHost(() => Host<Action>()()); }
    public void InvokeVoid<T1>(T1 arg1) { if (_compiled is { } program) _ = CoflowVm.ExecuteBound<T1, Unit>(program, RequiredOwner(), arg1); else InvokeHost(() => Host<Action<T1>>()(arg1)); }
    public void InvokeVoid<T1, T2>(T1 arg1, T2 arg2) { if (_compiled is { } program) _ = CoflowVm.ExecuteBound<T1, T2, Unit>(program, RequiredOwner(), arg1, arg2); else InvokeHost(() => Host<Action<T1, T2>>()(arg1, arg2)); }
    public void InvokeVoid<T1, T2, T3>(T1 arg1, T2 arg2, T3 arg3) { if (_compiled is { } program) _ = CoflowVm.ExecuteBound<T1, T2, T3, Unit>(program, RequiredOwner(), arg1, arg2, arg3); else InvokeHost(() => Host<Action<T1, T2, T3>>()(arg1, arg2, arg3)); }
    public void InvokeVoid<T1, T2, T3, T4>(T1 arg1, T2 arg2, T3 arg3, T4 arg4) { if (_compiled is { } program) _ = CoflowVm.ExecuteBound<T1, T2, T3, T4, Unit>(program, RequiredOwner(), arg1, arg2, arg3, arg4); else InvokeHost(() => Host<Action<T1, T2, T3, T4>>()(arg1, arg2, arg3, arg4)); }
    public void InvokeVoid<T1, T2, T3, T4, T5>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5) { if (_compiled is { } program) _ = CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, Unit>(program, RequiredOwner(), arg1, arg2, arg3, arg4, arg5); else InvokeHost(() => Host<Action<T1, T2, T3, T4, T5>>()(arg1, arg2, arg3, arg4, arg5)); }
    public void InvokeVoid<T1, T2, T3, T4, T5, T6>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6) { if (_compiled is { } program) _ = CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, T6, Unit>(program, RequiredOwner(), arg1, arg2, arg3, arg4, arg5, arg6); else InvokeHost(() => Host<Action<T1, T2, T3, T4, T5, T6>>()(arg1, arg2, arg3, arg4, arg5, arg6)); }
    public void InvokeVoid<T1, T2, T3, T4, T5, T6, T7>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7) { if (_compiled is { } program) _ = CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, T6, T7, Unit>(program, RequiredOwner(), arg1, arg2, arg3, arg4, arg5, arg6, arg7); else InvokeHost(() => Host<Action<T1, T2, T3, T4, T5, T6, T7>>()(arg1, arg2, arg3, arg4, arg5, arg6, arg7)); }
    public void InvokeVoid<T1, T2, T3, T4, T5, T6, T7, T8>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7, T8 arg8) { if (_compiled is { } program) _ = CoflowVm.ExecuteBound<T1, T2, T3, T4, T5, T6, T7, T8, Unit>(program, RequiredOwner(), arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8); else InvokeHost(() => Host<Action<T1, T2, T3, T4, T5, T6, T7, T8>>()(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8)); }

    private object RequiredOwner() => Owner ?? throw new CoflowFunctionNotBoundException();

    private TDelegate Host<TDelegate>() where TDelegate : Delegate
    {
        if (_implementation is TDelegate implementation)
        {
            CoflowInvocationContext.Budget.HostCall(HostBoundaryLanes());
            return implementation;
        }
        if (_implementation is null) throw new CoflowFunctionNotBoundException();
        throw new InvalidOperationException($"Host function `{Identity}` has an incompatible delegate signature.");
    }

    private TResult InvokeHost<TResult>(Func<TResult> call)
    {
        try { return call(); }
        catch (Exception error) { throw HostFault(error); }
    }

    private void InvokeHost(Action call)
    {
        try { call(); }
        catch (Exception error) { throw HostFault(error); }
    }

    private Exception HostFault(Exception error) => error is CoflowFaultException fault
        ? fault.WithCallers(new[] { Identity }, SourcePath, SourceSpan)
        : new CoflowFaultException(Identity, SourcePath, SourceSpan,
            new[] { Identity }, error.Message, error);

    private long HostBoundaryLanes()
    {
        var result = CoflowValueShape.Of(Signature.ResultType);
        long lanes = (long)result.IntegerCount + result.FloatCount + result.ReferenceCount;
        foreach (var type in Signature.ParameterTypes)
        {
            var layout = CoflowValueShape.Of(type);
            lanes += (long)layout.IntegerCount + layout.FloatCount + layout.ReferenceCount;
        }
        return lanes;
    }

    internal void InvokeBoundFromVm(CoflowNativeFrame frame)
    {
        var call = _hostCall;
        if (call is null) throw new CoflowFunctionNotBoundException();
        try { call.Invoke(frame); }
        catch (Exception error) { throw HostFault(error); }
    }
}
