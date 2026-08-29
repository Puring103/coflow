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
public readonly record struct CoflowHostFunctionBinding
{
    public CoflowHostFunctionBinding(CoflowFunctionEntry entry, Delegate implementation)
        : this(entry, implementation, null) { }

    private CoflowHostFunctionBinding(
        CoflowFunctionEntry entry,
        Delegate implementation,
        CoflowNativeCall? call)
    {
        Entry = entry ?? throw new ArgumentNullException(nameof(entry));
        Implementation = implementation ?? throw new ArgumentNullException(nameof(implementation));
        Call = call;
    }

    public CoflowFunctionEntry Entry { get; }
    public Delegate Implementation { get; }
    internal CoflowNativeCall? Call { get; }

    public static CoflowHostFunctionBinding Create<TResult>(CoflowFunctionEntry entry, Func<TResult> implementation) =>
        new(entry, implementation, new(Type.EmptyTypes, typeof(TResult), frame => frame.Write(implementation())));
    public static CoflowHostFunctionBinding Create<T1, TResult>(CoflowFunctionEntry entry, Func<T1, TResult> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1) }, typeof(TResult), frame => frame.Write(implementation(frame.Read<T1>(0)))));
    public static CoflowHostFunctionBinding Create<T1, T2, TResult>(CoflowFunctionEntry entry, Func<T1, T2, TResult> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2) }, typeof(TResult), frame => frame.Write(implementation(frame.Read<T1>(0), frame.Read<T2>(1)))));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, TResult>(CoflowFunctionEntry entry, Func<T1, T2, T3, TResult> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3) }, typeof(TResult), frame => frame.Write(implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2)))));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4, TResult>(CoflowFunctionEntry entry, Func<T1, T2, T3, T4, TResult> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4) }, typeof(TResult), frame => frame.Write(implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3)))));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4, T5, TResult>(CoflowFunctionEntry entry, Func<T1, T2, T3, T4, T5, TResult> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4), typeof(T5) }, typeof(TResult), frame => frame.Write(implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3), frame.Read<T5>(4)))));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4, T5, T6, TResult>(CoflowFunctionEntry entry, Func<T1, T2, T3, T4, T5, T6, TResult> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4), typeof(T5), typeof(T6) }, typeof(TResult), frame => frame.Write(implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3), frame.Read<T5>(4), frame.Read<T6>(5)))));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4, T5, T6, T7, TResult>(CoflowFunctionEntry entry, Func<T1, T2, T3, T4, T5, T6, T7, TResult> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4), typeof(T5), typeof(T6), typeof(T7) }, typeof(TResult), frame => frame.Write(implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3), frame.Read<T5>(4), frame.Read<T6>(5), frame.Read<T7>(6)))));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(CoflowFunctionEntry entry, Func<T1, T2, T3, T4, T5, T6, T7, T8, TResult> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4), typeof(T5), typeof(T6), typeof(T7), typeof(T8) }, typeof(TResult), frame => frame.Write(implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3), frame.Read<T5>(4), frame.Read<T6>(5), frame.Read<T7>(6), frame.Read<T8>(7)))));

    public static CoflowHostFunctionBinding Create(CoflowFunctionEntry entry, Action implementation) =>
        new(entry, implementation, new(Type.EmptyTypes, typeof(Unit), frame => { implementation(); frame.Write(Unit.Value); }));
    public static CoflowHostFunctionBinding Create<T1>(CoflowFunctionEntry entry, Action<T1> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1) }, typeof(Unit), frame => { implementation(frame.Read<T1>(0)); frame.Write(Unit.Value); }));
    public static CoflowHostFunctionBinding Create<T1, T2>(CoflowFunctionEntry entry, Action<T1, T2> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2) }, typeof(Unit), frame => { implementation(frame.Read<T1>(0), frame.Read<T2>(1)); frame.Write(Unit.Value); }));
    public static CoflowHostFunctionBinding Create<T1, T2, T3>(CoflowFunctionEntry entry, Action<T1, T2, T3> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3) }, typeof(Unit), frame => { implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2)); frame.Write(Unit.Value); }));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4>(CoflowFunctionEntry entry, Action<T1, T2, T3, T4> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4) }, typeof(Unit), frame => { implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3)); frame.Write(Unit.Value); }));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4, T5>(CoflowFunctionEntry entry, Action<T1, T2, T3, T4, T5> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4), typeof(T5) }, typeof(Unit), frame => { implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3), frame.Read<T5>(4)); frame.Write(Unit.Value); }));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4, T5, T6>(CoflowFunctionEntry entry, Action<T1, T2, T3, T4, T5, T6> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4), typeof(T5), typeof(T6) }, typeof(Unit), frame => { implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3), frame.Read<T5>(4), frame.Read<T6>(5)); frame.Write(Unit.Value); }));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4, T5, T6, T7>(CoflowFunctionEntry entry, Action<T1, T2, T3, T4, T5, T6, T7> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4), typeof(T5), typeof(T6), typeof(T7) }, typeof(Unit), frame => { implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3), frame.Read<T5>(4), frame.Read<T6>(5), frame.Read<T7>(6)); frame.Write(Unit.Value); }));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4, T5, T6, T7, T8>(CoflowFunctionEntry entry, Action<T1, T2, T3, T4, T5, T6, T7, T8> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4), typeof(T5), typeof(T6), typeof(T7), typeof(T8) }, typeof(Unit), frame => { implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3), frame.Read<T5>(4), frame.Read<T6>(5), frame.Read<T7>(6), frame.Read<T8>(7)); frame.Write(Unit.Value); }));
}

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
        foreach (var function in functions)
            function.Entry.ConfigureHost(function.Implementation, function.Call);
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

[EditorBrowsable(EditorBrowsableState.Never)]
public readonly struct CoflowCallable
{
    private readonly object _target;
    private CoflowFunctionEntry? _entry => _target as CoflowFunctionEntry;
    private CoflowClosure? _closure => _target as CoflowClosure;

    internal CoflowCallable(CoflowFunctionEntry entry) => _target = entry;
    internal CoflowCallable(CoflowClosure closure) => _target = closure;

    public TResult Invoke<TResult>() => _entry is { } entry ? entry.Invoke<TResult>() : CoflowVm.ExecuteClosure<TResult>(_closure!);
    public TResult Invoke<T1, TResult>(T1 arg1) => _entry is { } entry ? entry.Invoke<T1, TResult>(arg1) : CoflowVm.ExecuteClosure<T1, TResult>(_closure!, arg1);
    public TResult Invoke<T1, T2, TResult>(T1 arg1, T2 arg2) => _entry is { } entry ? entry.Invoke<T1, T2, TResult>(arg1, arg2) : CoflowVm.ExecuteClosure<T1, T2, TResult>(_closure!, arg1, arg2);
    public TResult Invoke<T1, T2, T3, TResult>(T1 arg1, T2 arg2, T3 arg3) => _entry is { } entry ? entry.Invoke<T1, T2, T3, TResult>(arg1, arg2, arg3) : CoflowVm.ExecuteClosure<T1, T2, T3, TResult>(_closure!, arg1, arg2, arg3);
    public TResult Invoke<T1, T2, T3, T4, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4) => _entry is { } entry ? entry.Invoke<T1, T2, T3, T4, TResult>(arg1, arg2, arg3, arg4) : CoflowVm.ExecuteClosure<T1, T2, T3, T4, TResult>(_closure!, arg1, arg2, arg3, arg4);
    public TResult Invoke<T1, T2, T3, T4, T5, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5) => _entry is { } entry ? entry.Invoke<T1, T2, T3, T4, T5, TResult>(arg1, arg2, arg3, arg4, arg5) : CoflowVm.ExecuteClosure<T1, T2, T3, T4, T5, TResult>(_closure!, arg1, arg2, arg3, arg4, arg5);
    public TResult Invoke<T1, T2, T3, T4, T5, T6, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6) => _entry is { } entry ? entry.Invoke<T1, T2, T3, T4, T5, T6, TResult>(arg1, arg2, arg3, arg4, arg5, arg6) : CoflowVm.ExecuteClosure<T1, T2, T3, T4, T5, T6, TResult>(_closure!, arg1, arg2, arg3, arg4, arg5, arg6);
    public TResult Invoke<T1, T2, T3, T4, T5, T6, T7, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7) => _entry is { } entry ? entry.Invoke<T1, T2, T3, T4, T5, T6, T7, TResult>(arg1, arg2, arg3, arg4, arg5, arg6, arg7) : CoflowVm.ExecuteClosure<T1, T2, T3, T4, T5, T6, T7, TResult>(_closure!, arg1, arg2, arg3, arg4, arg5, arg6, arg7);
    public TResult Invoke<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7, T8 arg8) => _entry is { } entry ? entry.Invoke<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8) : CoflowVm.ExecuteClosure<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(_closure!, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8);

    public void InvokeVoid() { if (_entry is { } entry) entry.InvokeVoid(); else CoflowVm.ExecuteClosure<Unit>(_closure!); }
    public void InvokeVoid<T1>(T1 arg1) { if (_entry is { } entry) entry.InvokeVoid(arg1); else CoflowVm.ExecuteClosure<T1, Unit>(_closure!, arg1); }
    public void InvokeVoid<T1, T2>(T1 arg1, T2 arg2) { if (_entry is { } entry) entry.InvokeVoid(arg1, arg2); else CoflowVm.ExecuteClosure<T1, T2, Unit>(_closure!, arg1, arg2); }
    public void InvokeVoid<T1, T2, T3>(T1 arg1, T2 arg2, T3 arg3) { if (_entry is { } entry) entry.InvokeVoid(arg1, arg2, arg3); else CoflowVm.ExecuteClosure<T1, T2, T3, Unit>(_closure!, arg1, arg2, arg3); }
    public void InvokeVoid<T1, T2, T3, T4>(T1 arg1, T2 arg2, T3 arg3, T4 arg4) { if (_entry is { } entry) entry.InvokeVoid(arg1, arg2, arg3, arg4); else CoflowVm.ExecuteClosure<T1, T2, T3, T4, Unit>(_closure!, arg1, arg2, arg3, arg4); }
    public void InvokeVoid<T1, T2, T3, T4, T5>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5) { if (_entry is { } entry) entry.InvokeVoid(arg1, arg2, arg3, arg4, arg5); else CoflowVm.ExecuteClosure<T1, T2, T3, T4, T5, Unit>(_closure!, arg1, arg2, arg3, arg4, arg5); }
    public void InvokeVoid<T1, T2, T3, T4, T5, T6>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6) { if (_entry is { } entry) entry.InvokeVoid(arg1, arg2, arg3, arg4, arg5, arg6); else CoflowVm.ExecuteClosure<T1, T2, T3, T4, T5, T6, Unit>(_closure!, arg1, arg2, arg3, arg4, arg5, arg6); }
    public void InvokeVoid<T1, T2, T3, T4, T5, T6, T7>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7) { if (_entry is { } entry) entry.InvokeVoid(arg1, arg2, arg3, arg4, arg5, arg6, arg7); else CoflowVm.ExecuteClosure<T1, T2, T3, T4, T5, T6, T7, Unit>(_closure!, arg1, arg2, arg3, arg4, arg5, arg6, arg7); }
    public void InvokeVoid<T1, T2, T3, T4, T5, T6, T7, T8>(T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7, T8 arg8) { if (_entry is { } entry) entry.InvokeVoid(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8); else CoflowVm.ExecuteClosure<T1, T2, T3, T4, T5, T6, T7, T8, Unit>(_closure!, arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8); }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public static class CoflowDelegateAdapter
{
    public static void Register<TDelegate>(Func<CoflowCallable, TDelegate> factory)
        where TDelegate : Delegate => CoflowFunctionDelegates.Register(factory);
}

internal static class CoflowFunctionDelegates
{
    internal sealed class CallableDescriptor(
        CoflowFunctionEntry? entry,
        CoflowClosure? closure,
        CoflowNativeCall? nativeCall)
    {
        internal CoflowFunctionEntry? Entry { get; } = entry;
        internal CoflowClosure? Closure { get; } = closure;
        internal CoflowNativeCall? NativeCall { get; } = nativeCall;
    }

    private static readonly System.Collections.Concurrent.ConcurrentDictionary<Type,
        Func<CoflowClosure, Delegate>> ClosureFactories = new();
    private static readonly System.Runtime.CompilerServices.ConditionalWeakTable<Delegate, CoflowFunctionEntry>
        Entries = new();
    private static readonly System.Runtime.CompilerServices.ConditionalWeakTable<Delegate, CoflowClosure>
        Closures = new();
    private static readonly System.Runtime.CompilerServices.ConditionalWeakTable<Delegate, CoflowNativeCall>
        NativeCalls = new();
    private static readonly System.Runtime.CompilerServices.ConditionalWeakTable<Delegate, CallableDescriptor>
        Callables = new();
    private static readonly System.Collections.Concurrent.ConcurrentDictionary<Type, byte>
        GeneratedAdapterTypes = new();

    internal static TDelegate Create<TDelegate>(CoflowFunctionEntry entry)
        where TDelegate : Delegate
        => (TDelegate)Create(typeof(TDelegate), entry);

    internal static void Register<TDelegate>(Func<CoflowCallable, TDelegate> factory)
        where TDelegate : Delegate
    {
        if (factory is null) throw new ArgumentNullException(nameof(factory));
        ClosureFactories[typeof(TDelegate)] = closure => factory(new CoflowCallable(closure));
        GeneratedAdapterTypes[typeof(TDelegate)] = 0;
    }

    internal static bool HasGeneratedAdapter<TDelegate>() where TDelegate : Delegate =>
        GeneratedAdapterTypes.ContainsKey(typeof(TDelegate));

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
        var implementation = CoflowExpressionCompiler.Compile(
            System.Linq.Expressions.Expression.Lambda(delegateType, body, parameters));
        Entries.Add(implementation, entry);
        return implementation;
    }

    private static CoflowNativeCall NativeCall(Delegate implementation) =>
        NativeCalls.GetValue(implementation, static value => new CoflowNativeCall(value));

    internal static CallableDescriptor Callable(Delegate implementation) =>
        Callables.GetValue(implementation, static value =>
        {
            if (Entries.TryGetValue(value, out var entry))
                return new CallableDescriptor(entry, null, null);
            if (Closures.TryGetValue(value, out var closure))
                return new CallableDescriptor(null, closure, null);
            return new CallableDescriptor(null, null, NativeCall(value));
        });

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
        return CoflowExpressionCompiler.Compile(
            System.Linq.Expressions.Expression.Lambda<Func<CoflowClosure, Delegate>>(
            System.Linq.Expressions.Expression.Convert(
                System.Linq.Expressions.Expression.Lambda(delegateType, body, parameters),
                typeof(Delegate)),
            closure));
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

    [EditorBrowsable(EditorBrowsableState.Never)]
    public static CoflowFunctionEntry<TDelegate> CreateAot(
        CoflowFunctionEntry entry,
        Func<CoflowCallable, TDelegate> factory)
    {
        if (factory is null) throw new ArgumentNullException(nameof(factory));
        CoflowFunctionDelegates.Register(factory);
        return new(entry, factory(new CoflowCallable(entry)));
    }

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
    internal void ConfigureHost(Delegate implementation, CoflowNativeCall? call = null)
    {
        if (implementation is null) throw new ArgumentNullException(nameof(implementation));
        if (!_functionsCompiled || _compiled is not null)
            throw new InvalidOperationException("A compiled Coflow function cannot be configured as a host function.");
        _implementation = implementation;
        _hostCall = call ?? new CoflowNativeCall(implementation);
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
