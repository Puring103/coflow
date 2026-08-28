namespace CoflowRuntime;

using System.ComponentModel;

public sealed class CoflowFunctionNotCompiledException : InvalidOperationException
{
    public CoflowFunctionNotCompiledException()
        : base("Coflow functions are unavailable because the data was loaded with Coflow.LoadData.") { }
}

public sealed class CoflowFunctionNotBoundException : InvalidOperationException
{
    public CoflowFunctionNotBoundException()
        : base("The Coflow function has no CFD body or bound C# implementation.") { }
}

public sealed class CoflowHostNotBoundException : InvalidOperationException
{
    public CoflowHostNotBoundException()
        : base("The generated @Host singleton has not been bound.") { }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public readonly record struct CoflowHostFunctionBinding(
    CoflowFunctionEntry Entry,
    Delegate Implementation);

[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CoflowHostState
{
    private readonly bool _functionsCompiled;
    private bool _configured;

    internal CoflowHostState(bool functionsCompiled)
    {
        _functionsCompiled = functionsCompiled;
    }

    public void EnsureBound()
    {
        if (!_configured) throw new CoflowHostNotBoundException();
    }

    public void Configure(
        Action assignFields,
        params CoflowHostFunctionBinding[] functions)
    {
        if (assignFields is null) throw new ArgumentNullException(nameof(assignFields));
        if (functions is null) throw new ArgumentNullException(nameof(functions));
        if (!_functionsCompiled) throw new CoflowFunctionNotCompiledException();
        foreach (var function in functions) function.Entry.ConfigureHost(function.Implementation);
        assignFields();
        _configured = true;
    }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public readonly record struct CoflowFunctionIdentity(
    string DeclaredType,
    string RecordKey,
    string FieldName,
    string ValuePath = "");

internal static class CoflowFunctionDelegates
{
    private static readonly System.Collections.Concurrent.ConcurrentDictionary<Type,
        Func<CoflowClosure, Delegate>> ClosureFactories = new();
    private static readonly System.Runtime.CompilerServices.ConditionalWeakTable<Delegate, CoflowFunctionEntry>
        Entries = new();
    private static readonly System.Runtime.CompilerServices.ConditionalWeakTable<Delegate, CoflowClosure>
        Closures = new();
    private static readonly System.Runtime.CompilerServices.ConditionalWeakTable<Delegate, CoflowNativeCall>
        NativeCalls = new();

    internal static TDelegate Create<TDelegate>(CoflowFunctionEntry entry)
        where TDelegate : Delegate
        => (TDelegate)Create(typeof(TDelegate), entry);

    private static Delegate Create(Type delegateType, CoflowFunctionEntry entry)
    {
        var invoke = delegateType.GetMethod("Invoke") ?? throw new ArgumentException(
            "The requested function value type is not a delegate.", nameof(delegateType));
        var parameters = invoke.GetParameters()
            .Select(parameter => System.Linq.Expressions.Expression.Parameter(
                parameter.ParameterType, parameter.Name))
            .ToArray();
        var method = TypedInvokeMethod(typeof(CoflowFunctionEntry), invoke);
        var genericTypes = invoke.GetParameters().Select(parameter => parameter.ParameterType)
            .Concat(invoke.ReturnType == typeof(void) ? Type.EmptyTypes : new[] { invoke.ReturnType })
            .ToArray();
        if (method.IsGenericMethodDefinition) method = method.MakeGenericMethod(genericTypes);
        var body = System.Linq.Expressions.Expression.Call(
            System.Linq.Expressions.Expression.Constant(entry), method, parameters);
        var implementation = System.Linq.Expressions.Expression
            .Lambda(delegateType, body, parameters)
            .Compile();
        Entries.Add(implementation, entry);
        return implementation;
    }

    internal static bool TryGetEntry(Delegate implementation, out CoflowFunctionEntry entry) =>
        Entries.TryGetValue(implementation, out entry!);

    internal static bool TryGetClosure(Delegate implementation, out CoflowClosure closure) =>
        Closures.TryGetValue(implementation, out closure!);

    internal static CoflowNativeCall NativeCall(Delegate implementation) =>
        NativeCalls.GetValue(implementation, static value => new CoflowNativeCall(value));

    internal static object? Adapt(Type expectedType, object? value)
    {
        if (!typeof(Delegate).IsAssignableFrom(expectedType))
            return value;
        if (value is CoflowFunctionEntry entry) return Create(expectedType, entry);
        if (value is not CoflowClosure closure) return value;
        var implementation = ClosureFactories.GetOrAdd(expectedType, CreateClosureFactory)(closure);
        Closures.Add(implementation, closure);
        return implementation;
    }

    private static Func<CoflowClosure, Delegate> CreateClosureFactory(Type delegateType)
    {
        var invoke = delegateType.GetMethod("Invoke") ?? throw new ArgumentException(
            "The requested function value type is not a delegate.", nameof(delegateType));
        var closure = System.Linq.Expressions.Expression.Parameter(typeof(CoflowClosure), "closure");
        var parameters = invoke.GetParameters()
            .Select(parameter => System.Linq.Expressions.Expression.Parameter(
                parameter.ParameterType, parameter.Name))
            .ToArray();
        var method = TypedInvokeMethod(typeof(CoflowClosure), invoke);
        var genericTypes = invoke.GetParameters().Select(parameter => parameter.ParameterType)
            .Concat(invoke.ReturnType == typeof(void) ? Type.EmptyTypes : new[] { invoke.ReturnType })
            .ToArray();
        if (method.IsGenericMethodDefinition) method = method.MakeGenericMethod(genericTypes);
        var body = System.Linq.Expressions.Expression.Call(closure, method, parameters);
        return System.Linq.Expressions.Expression.Lambda<Func<CoflowClosure, Delegate>>(
            System.Linq.Expressions.Expression.Convert(
                System.Linq.Expressions.Expression.Lambda(delegateType, body, parameters),
                typeof(Delegate)),
            closure).Compile();
    }

    internal static T Adapt<T>(object? value) => (T)Adapt(typeof(T), value)!;

    private static System.Reflection.MethodInfo TypedInvokeMethod(Type owner, System.Reflection.MethodInfo signature)
    {
        var name = signature.ReturnType == typeof(void) ? "InvokeVoid" : "Invoke";
        var genericCount = signature.GetParameters().Length + (signature.ReturnType == typeof(void) ? 0 : 1);
        return owner.GetMethods(System.Reflection.BindingFlags.Instance |
                System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.NonPublic)
            .Single(method => method.Name == name && method.GetParameters().Length == signature.GetParameters().Length &&
                (method.IsGenericMethodDefinition ? method.GetGenericArguments().Length : 0) == genericCount);
    }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CoflowFunctionSignature
{
    public CoflowFunctionSignature(Type resultType, IReadOnlyList<Type> parameterTypes)
    {
        ResultType = resultType ?? throw new ArgumentNullException(nameof(resultType));
        ParameterTypes = parameterTypes ?? throw new ArgumentNullException(nameof(parameterTypes));
    }

    public Type ResultType { get; }
    public IReadOnlyList<Type> ParameterTypes { get; }
}

public sealed class CoflowFunctionEntry<TDelegate> where TDelegate : Delegate
{
    [EditorBrowsable(EditorBrowsableState.Never)]
    public CoflowFunctionEntry(CoflowFunctionEntry entry, TDelegate function)
    {
        RuntimeEntry = entry ?? throw new ArgumentNullException(nameof(entry));
        Function = function ?? throw new ArgumentNullException(nameof(function));
    }

    [EditorBrowsable(EditorBrowsableState.Never)]
    public static CoflowFunctionEntry<TDelegate> Create(
        CoflowFunctionEntry entry,
        Func<CoflowFunctionEntry, TDelegate> factory) =>
        new(entry, (factory ?? throw new ArgumentNullException(nameof(factory)))(entry));

    public TDelegate Function { get; }

    [EditorBrowsable(EditorBrowsableState.Never)]
    public CoflowFunctionEntry RuntimeEntry { get; }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CoflowFunctionEntry
{
    private Delegate? _implementation;
    private CoflowNativeCall? _hostCall;
    private CoflowProgram? _compiled;
    private bool _functionsCompiled;

    internal CoflowFunctionEntry(
        CoflowFunctionIdentity identity,
        CoflowFunctionSignature signature,
        CfdFunctionValue? source,
        CfdNameResolver names,
        string sourcePath,
        CfdSpan? sourceSpan,
        bool requiresCfdBody = false)
    {
        Identity = identity;
        Signature = signature;
        Source = source;
        Names = names;
        SourcePath = sourcePath;
        SourceSpan = sourceSpan;
        RequiresCfdBody = requiresCfdBody;
    }

    internal CoflowFunctionIdentity Identity { get; }
    internal CoflowFunctionSignature Signature { get; }
    internal CfdFunctionValue? Source { get; }
    internal CfdNameResolver Names { get; }
    internal string SourcePath { get; }
    internal CfdSpan? SourceSpan { get; }
    internal bool RequiresCfdBody { get; }
    internal void ConfigureHost(Delegate implementation)
    {
        if (implementation is null) throw new ArgumentNullException(nameof(implementation));
        if (!_functionsCompiled || _compiled is not null)
            throw new InvalidOperationException("A compiled Coflow function cannot be configured as a host function.");
        _implementation = implementation;
        _hostCall = new CoflowNativeCall(implementation);
    }

    internal void PublishCompiled(CoflowProgram? implementation)
    {
        _compiled = implementation;
        _functionsCompiled = true;
    }

    public TResult Invoke<TResult>() => _compiled is { } program ? CoflowVm.Execute<TResult>(program) : Host<Func<TResult>>()();
    public TResult Invoke<T1, TResult>(T1 arg1) => _compiled is { } program ? CoflowVm.Execute<T1, TResult>(program, arg1) : Host<Func<T1, TResult>>()(arg1);
    public TResult Invoke<T1, T2, TResult>(T1 arg1, T2 arg2) => _compiled is { } program ? CoflowVm.Execute<T1, T2, TResult>(program, arg1, arg2) : Host<Func<T1, T2, TResult>>()(arg1, arg2);
    public TResult Invoke<T1, T2, T3, TResult>(T1 arg1, T2 arg2, T3 arg3) => _compiled is { } program ? CoflowVm.Execute<T1, T2, T3, TResult>(program, arg1, arg2, arg3) : Host<Func<T1, T2, T3, TResult>>()(arg1, arg2, arg3);
    public TResult Invoke<T1, T2, T3, T4, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4) => _compiled is { } program ? CoflowVm.Execute<T1, T2, T3, T4, TResult>(program, arg1, arg2, arg3, arg4) : Host<Func<T1, T2, T3, T4, TResult>>()(arg1, arg2, arg3, arg4);
    public TResult Invoke<T1, T2, T3, T4, T5, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5) => _compiled is { } program ? CoflowVm.Execute<T1, T2, T3, T4, T5, TResult>(program, arg1, arg2, arg3, arg4, arg5) : Host<Func<T1, T2, T3, T4, T5, TResult>>()(arg1, arg2, arg3, arg4, arg5);
    public TResult Invoke<T1, T2, T3, T4, T5, T6, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6) => _compiled is { } program ? CoflowVm.Execute<T1, T2, T3, T4, T5, T6, TResult>(program, arg1, arg2, arg3, arg4, arg5, arg6) : Host<Func<T1, T2, T3, T4, T5, T6, TResult>>()(arg1, arg2, arg3, arg4, arg5, arg6);
    public TResult Invoke<T1, T2, T3, T4, T5, T6, T7, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7) => _compiled is { } program ? CoflowVm.Execute<T1, T2, T3, T4, T5, T6, T7, TResult>(program, arg1, arg2, arg3, arg4, arg5, arg6, arg7) : Host<Func<T1, T2, T3, T4, T5, T6, T7, TResult>>()(arg1, arg2, arg3, arg4, arg5, arg6, arg7);
    public TResult Invoke<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7, T8 arg8) => _compiled is { } program ? CoflowVm.Execute<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(program, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8) : Host<Func<T1, T2, T3, T4, T5, T6, T7, T8, TResult>>()(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8);

    public void InvokeVoid()
    {
        if (_compiled is { } program) { CoflowVm.Execute<Unit>(program); return; }
        Host<Action>()();
    }
    public void InvokeVoid<T1>(T1 arg1)
    {
        if (_compiled is { } program) { CoflowVm.Execute<T1, Unit>(program, arg1); return; }
        Host<Action<T1>>()(arg1);
    }
    public void InvokeVoid<T1, T2>(T1 arg1, T2 arg2)
    {
        if (_compiled is { } program) { CoflowVm.Execute<T1, T2, Unit>(program, arg1, arg2); return; }
        Host<Action<T1, T2>>()(arg1, arg2);
    }
    public void InvokeVoid<T1, T2, T3>(T1 arg1, T2 arg2, T3 arg3)
    {
        if (_compiled is { } program) { CoflowVm.Execute<T1, T2, T3, Unit>(program, arg1, arg2, arg3); return; }
        Host<Action<T1, T2, T3>>()(arg1, arg2, arg3);
    }
    public void InvokeVoid<T1, T2, T3, T4>(T1 arg1, T2 arg2, T3 arg3, T4 arg4)
    {
        if (_compiled is { } program) { CoflowVm.Execute<T1, T2, T3, T4, Unit>(program, arg1, arg2, arg3, arg4); return; }
        Host<Action<T1, T2, T3, T4>>()(arg1, arg2, arg3, arg4);
    }
    public void InvokeVoid<T1, T2, T3, T4, T5>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5)
    {
        if (_compiled is { } program) { CoflowVm.Execute<T1, T2, T3, T4, T5, Unit>(program, arg1, arg2, arg3, arg4, arg5); return; }
        Host<Action<T1, T2, T3, T4, T5>>()(arg1, arg2, arg3, arg4, arg5);
    }
    public void InvokeVoid<T1, T2, T3, T4, T5, T6>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6)
    {
        if (_compiled is { } program) { CoflowVm.Execute<T1, T2, T3, T4, T5, T6, Unit>(program, arg1, arg2, arg3, arg4, arg5, arg6); return; }
        Host<Action<T1, T2, T3, T4, T5, T6>>()(arg1, arg2, arg3, arg4, arg5, arg6);
    }
    public void InvokeVoid<T1, T2, T3, T4, T5, T6, T7>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7)
    {
        if (_compiled is { } program) { CoflowVm.Execute<T1, T2, T3, T4, T5, T6, T7, Unit>(program, arg1, arg2, arg3, arg4, arg5, arg6, arg7); return; }
        Host<Action<T1, T2, T3, T4, T5, T6, T7>>()(arg1, arg2, arg3, arg4, arg5, arg6, arg7);
    }
    public void InvokeVoid<T1, T2, T3, T4, T5, T6, T7, T8>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7, T8 arg8)
    {
        if (_compiled is { } program) { CoflowVm.Execute<T1, T2, T3, T4, T5, T6, T7, T8, Unit>(program, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8); return; }
        Host<Action<T1, T2, T3, T4, T5, T6, T7, T8>>()(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8);
    }

    private TDelegate Host<TDelegate>() where TDelegate : Delegate
    {
        if (!_functionsCompiled) throw new CoflowFunctionNotCompiledException();
        if (_implementation is TDelegate implementation) return implementation;
        if (_implementation is null) throw new CoflowFunctionNotBoundException();
        throw new InvalidOperationException($"Host function `{Identity}` has an incompatible delegate signature.");
    }

    internal CoflowProgram? CompiledProgram
    {
        get => _compiled;
    }
    internal bool HasBoundImplementation
    {
        get => _implementation is not null;
    }

    internal void InvokeBoundFromVm(CoflowNativeFrame frame)
    {
        var call = _hostCall;
        if (call is null) throw new CoflowFunctionNotBoundException();
        try { call.Invoke(frame); }
        catch (Exception error)
        {
            if (error is CoflowFaultException fault)
                throw fault.WithCallers(new[] { Identity }, SourcePath, SourceSpan);
            throw new CoflowFaultException(
                Identity, SourcePath, SourceSpan, new[] { Identity }, error.Message, error);
        }
    }

}
