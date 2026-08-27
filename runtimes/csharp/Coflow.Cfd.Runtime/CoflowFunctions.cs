namespace CoflowRuntime;

using System.ComponentModel;

public enum HostBindError
{
    FunctionsNotCompiled,
    AlreadyBound,
    FunctionAlreadyImplemented,
    GenerationRetired,
}

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
    CoflowFunctionSlot Slot,
    Delegate Implementation);

[EditorBrowsable(EditorBrowsableState.Never)]
public readonly record struct CoflowHostFunctionTransfer(
    CoflowFunctionSlot Source,
    CoflowFunctionSlot Target);

internal sealed class CoflowGenerationGate
{
    internal object Sync { get; } = new();
    internal bool IsActive { get; private set; } = true;

    internal void Retire()
    {
        if (!Monitor.IsEntered(Sync))
            throw new SynchronizationLockException("The generation gate must be locked before it is retired.");
        IsActive = false;
    }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CoflowHostSlot
{
    private readonly object _sync = new();
    private readonly CoflowGenerationGate _generationGate;
    private readonly bool _functionsCompiled;
    private bool _bound;

    internal CoflowHostSlot(bool functionsCompiled, CoflowGenerationGate generationGate)
    {
        _functionsCompiled = functionsCompiled;
        _generationGate = generationGate;
    }

    public void EnsureBound()
    {
        lock (_sync)
        {
            if (!_bound) throw new CoflowHostNotBoundException();
        }
    }

    public Result<Unit, HostBindError> Bind(
        Action assignFields,
        params CoflowHostFunctionBinding[] functions)
    {
        if (assignFields is null) throw new ArgumentNullException(nameof(assignFields));
        if (functions is null) throw new ArgumentNullException(nameof(functions));
        lock (_generationGate.Sync)
        {
            lock (_sync)
            {
                if (!_generationGate.IsActive)
                    return Result<Unit, HostBindError>.Err(HostBindError.GenerationRetired);
                if (!_functionsCompiled)
                    return Result<Unit, HostBindError>.Err(HostBindError.FunctionsNotCompiled);
                if (_bound)
                    return Result<Unit, HostBindError>.Err(HostBindError.AlreadyBound);
                if (functions.Any(function => !function.Slot.CanBind))
                    return Result<Unit, HostBindError>.Err(HostBindError.FunctionAlreadyImplemented);
                foreach (var function in functions)
                    function.Slot.BindHost(function.Implementation);
                assignFields();
                _bound = true;
                return Result<Unit, HostBindError>.Ok(Unit.Value);
            }
        }
    }

    public void TransferStateTo(
        CoflowHostSlot target,
        Action copyFields,
        params CoflowHostFunctionTransfer[] functions)
    {
        if (target is null) throw new ArgumentNullException(nameof(target));
        if (copyFields is null) throw new ArgumentNullException(nameof(copyFields));
        if (functions is null) throw new ArgumentNullException(nameof(functions));
        lock (_sync)
        {
            if (!_bound) return;
            lock (target._sync)
            {
                if (!target._functionsCompiled || target._bound)
                    throw new InvalidOperationException("The target Coflow host slot cannot receive binding state.");
                foreach (var function in functions)
                    if (!function.Source.TransferBindingTo(function.Target))
                        throw new InvalidOperationException("A Coflow host function binding cannot be transferred.");
                copyFields();
                target._bound = true;
            }
        }
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
        AdaptedInvoker> Invokers = new();
    private static readonly System.Collections.Concurrent.ConcurrentDictionary<Type,
        Func<CoflowClosure, Delegate>> ClosureFactories = new();
    private static readonly System.Runtime.CompilerServices.ConditionalWeakTable<Delegate, CoflowFunctionSlot>
        Slots = new();
    private static readonly System.Runtime.CompilerServices.ConditionalWeakTable<Delegate, CoflowClosure>
        Closures = new();

    internal static TDelegate Create<TDelegate>(CoflowFunctionSlot slot)
        where TDelegate : Delegate
        => (TDelegate)Create(typeof(TDelegate), slot);

    private static Delegate Create(Type delegateType, CoflowFunctionSlot slot)
    {
        var invoke = delegateType.GetMethod("Invoke") ?? throw new ArgumentException(
            "The requested function value type is not a delegate.", nameof(delegateType));
        var parameters = invoke.GetParameters()
            .Select(parameter => System.Linq.Expressions.Expression.Parameter(
                parameter.ParameterType, parameter.Name))
            .ToArray();
        var arguments = System.Linq.Expressions.Expression.NewArrayInit(
            typeof(object),
            parameters.Select(parameter => System.Linq.Expressions.Expression.Convert(
                parameter, typeof(object))));
        System.Linq.Expressions.Expression body = invoke.ReturnType == typeof(void)
            ? System.Linq.Expressions.Expression.Call(
                System.Linq.Expressions.Expression.Constant(slot),
                typeof(CoflowFunctionSlot).GetMethod(nameof(CoflowFunctionSlot.InvokeVoid))!,
                arguments)
            : System.Linq.Expressions.Expression.Call(
                System.Linq.Expressions.Expression.Constant(slot),
                typeof(CoflowFunctionSlot).GetMethod(nameof(CoflowFunctionSlot.Invoke))!
                    .MakeGenericMethod(invoke.ReturnType),
                arguments);
        var implementation = System.Linq.Expressions.Expression
            .Lambda(delegateType, body, parameters)
            .Compile();
        Slots.Add(implementation, slot);
        return implementation;
    }

    internal static bool TryGetSlot(Delegate implementation, out CoflowFunctionSlot slot) =>
        Slots.TryGetValue(implementation, out slot!);

    internal static bool TryGetClosure(Delegate implementation, out CoflowClosure closure) =>
        Closures.TryGetValue(implementation, out closure!);

    internal static object? Adapt(Type expectedType, object? value)
    {
        if (!typeof(Delegate).IsAssignableFrom(expectedType))
            return value;
        if (value is CoflowFunctionSlot slot) return Create(expectedType, slot);
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
        var arguments = System.Linq.Expressions.Expression.NewArrayInit(
            typeof(object),
            parameters.Select(parameter => System.Linq.Expressions.Expression.Convert(
                parameter, typeof(object))));
        System.Linq.Expressions.Expression body = invoke.ReturnType == typeof(void)
            ? System.Linq.Expressions.Expression.Call(
                closure,
                typeof(CoflowClosure).GetMethod(nameof(CoflowClosure.InvokeVoid))!,
                arguments)
            : System.Linq.Expressions.Expression.Call(
                closure,
                typeof(CoflowClosure).GetMethod(nameof(CoflowClosure.Invoke))!
                    .MakeGenericMethod(invoke.ReturnType),
                arguments);
        return System.Linq.Expressions.Expression.Lambda<Func<CoflowClosure, Delegate>>(
            System.Linq.Expressions.Expression.Convert(
                System.Linq.Expressions.Expression.Lambda(delegateType, body, parameters),
                typeof(Delegate)),
            closure).Compile();
    }

    internal static T Adapt<T>(object? value) => (T)Adapt(typeof(T), value)!;

    internal static object? InvokeAdapted(Delegate implementation, object?[] arguments)
    {
        var invoker = Invokers.GetOrAdd(implementation.GetType(), CreateInvoker);
        object?[]? adaptedArguments = null;
        foreach (var index in invoker.DelegateParameterIndexes)
        {
            var adapted = Adapt(invoker.ParameterTypes[index], arguments[index]);
            if (ReferenceEquals(adapted, arguments[index])) continue;
            adaptedArguments ??= (object?[])arguments.Clone();
            adaptedArguments[index] = adapted;
        }
        return invoker.Invoke(implementation, adaptedArguments ?? arguments);
    }

    private static AdaptedInvoker CreateInvoker(Type delegateType)
    {
        var implementation = System.Linq.Expressions.Expression.Parameter(typeof(Delegate), "implementation");
        var arguments = System.Linq.Expressions.Expression.Parameter(typeof(object[]), "arguments");
        var invoke = delegateType.GetMethod("Invoke")!;
        var parameters = invoke.GetParameters();
        var call = System.Linq.Expressions.Expression.Invoke(
            System.Linq.Expressions.Expression.Convert(implementation, delegateType),
            parameters.Select((parameter, index) =>
                System.Linq.Expressions.Expression.Convert(
                    System.Linq.Expressions.Expression.ArrayIndex(arguments,
                        System.Linq.Expressions.Expression.Constant(index)),
                    parameter.ParameterType)));
        System.Linq.Expressions.Expression body = invoke.ReturnType == typeof(void)
            ? System.Linq.Expressions.Expression.Block(call,
                System.Linq.Expressions.Expression.Convert(
                    System.Linq.Expressions.Expression.Constant(Unit.Value), typeof(object)))
            : System.Linq.Expressions.Expression.Convert(call, typeof(object));
        return new AdaptedInvoker(
            System.Linq.Expressions.Expression.Lambda<Func<Delegate, object?[], object?>>(
                body, implementation, arguments).Compile(),
            parameters.Select(parameter => parameter.ParameterType).ToArray(),
            parameters.Select((parameter, index) => (parameter, index))
                .Where(item => typeof(Delegate).IsAssignableFrom(item.parameter.ParameterType))
                .Select(item => item.index)
                .ToArray());
    }

    private sealed record AdaptedInvoker(
        Func<Delegate, object?[], object?> Invoke,
        Type[] ParameterTypes,
        int[] DelegateParameterIndexes);
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

[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CoflowFunctionSlot
{
    private readonly object _sync = new();
    private Delegate? _implementation;
    private CoflowProgram? _compiled;
    private bool _functionsCompiled;

    internal CoflowFunctionSlot(
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
    internal bool CanBind
    {
        get
        {
            lock (_sync) return _functionsCompiled && _compiled is null && _implementation is null;
        }
    }

    internal void BindHost(Delegate implementation)
    {
        if (implementation is null) throw new ArgumentNullException(nameof(implementation));
        lock (_sync)
        {
            if (!_functionsCompiled || _compiled is not null || _implementation is not null)
                throw new InvalidOperationException("The Coflow host function slot cannot be bound.");
            System.Threading.Volatile.Write(ref _implementation, implementation);
        }
    }

    internal void PublishCompiled(CoflowProgram? implementation)
    {
        lock (_sync)
        {
            System.Threading.Volatile.Write(ref _compiled, implementation);
            System.Threading.Volatile.Write(ref _functionsCompiled, true);
        }
    }

    public TResult Invoke<TResult>(params object?[] arguments)
    {
        var value = InvokeCore(arguments);
        try { return CoflowFunctionDelegates.Adapt<TResult>(value); }
        catch (InvalidCastException)
        {
            throw new InvalidOperationException(
                $"Coflow function returned `{value?.GetType()}` instead of `{typeof(TResult)}`.");
        }
    }

    public void InvokeVoid(params object?[] arguments) => InvokeCore(arguments);

    private object? InvokeCore(object?[] arguments)
    {
        if (!System.Threading.Volatile.Read(ref _functionsCompiled))
            throw new CoflowFunctionNotCompiledException();
        var compiled = System.Threading.Volatile.Read(ref _compiled);
        var implementation = System.Threading.Volatile.Read(ref _implementation);
        if (compiled is not null) return CoflowVm.Execute(compiled, arguments);
        if (implementation is not null)
        {
            try
            {
                return CoflowFunctionDelegates.InvokeAdapted(implementation, arguments);
            }
            catch (Exception error)
            {
                var inner = error is System.Reflection.TargetInvocationException { InnerException: { } target }
                    ? target
                    : error;
                if (inner is CoflowFaultException fault)
                    throw fault.WithCallers(new[] { Identity }, SourcePath, SourceSpan);
                throw new CoflowFaultException(
                    Identity,
                    SourcePath,
                    SourceSpan,
                    new[] { Identity },
                    inner.Message,
                    inner);
            }
        }
        throw new CoflowFunctionNotBoundException();
    }

    internal CoflowProgram? CompiledProgram
    {
        get => System.Threading.Volatile.Read(ref _compiled);
    }
    internal bool HasBoundImplementation
    {
        get => System.Threading.Volatile.Read(ref _implementation) is not null;
    }

    internal bool TransferBindingTo(CoflowFunctionSlot target)
    {
        Delegate? implementation;
        lock (_sync) implementation = _implementation;
        if (implementation is null) return true;
        if (Identity != target.Identity ||
            Signature.ResultType != target.Signature.ResultType ||
            !Signature.ParameterTypes.SequenceEqual(target.Signature.ParameterTypes))
            return false;
        lock (target._sync)
        {
            if (!target._functionsCompiled || target._compiled is not null || target._implementation is not null)
                return false;
            System.Threading.Volatile.Write(ref target._implementation, implementation);
            return true;
        }
    }

    internal object? InvokeBoundFromVm(object?[] arguments)
    {
        var implementation = System.Threading.Volatile.Read(ref _implementation);
        if (implementation is not null)
        {
            try
            {
                return CoflowFunctionDelegates.InvokeAdapted(implementation, arguments);
            }
            catch (Exception error)
            {
                var inner = error is System.Reflection.TargetInvocationException { InnerException: { } target }
                    ? target
                    : error;
                if (inner is CoflowFaultException fault)
                    throw fault.WithCallers(new[] { Identity }, SourcePath, SourceSpan);
                throw new CoflowFaultException(
                    Identity,
                    SourcePath,
                    SourceSpan,
                    new[] { Identity },
                    inner.Message,
                    inner);
            }
        }
        throw new CoflowFunctionNotBoundException();
    }
}
