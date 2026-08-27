namespace CoflowRuntime;

using System.ComponentModel;

public enum HostBindError
{
    FunctionsNotCompiled,
    AlreadyBound,
    FunctionAlreadyImplemented,
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
public sealed class CoflowHostSlot
{
    private readonly object _sync = new();
    private readonly bool _functionsCompiled;
    private bool _bound;

    internal CoflowHostSlot(bool functionsCompiled) => _functionsCompiled = functionsCompiled;

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
        lock (_sync)
        {
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

    public void TransferStateTo(CoflowHostSlot target, Action copyFields)
    {
        if (target is null) throw new ArgumentNullException(nameof(target));
        if (copyFields is null) throw new ArgumentNullException(nameof(copyFields));
        lock (_sync)
        {
            if (!_bound) return;
            lock (target._sync)
            {
                if (!target._functionsCompiled || target._bound)
                    throw new InvalidOperationException("The target Coflow host slot cannot receive binding state.");
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
        var invoke = expectedType.GetMethod("Invoke") ?? throw new ArgumentException(
            "The requested function value type is not a delegate.", nameof(expectedType));
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
                System.Linq.Expressions.Expression.Constant(closure),
                typeof(CoflowClosure).GetMethod(nameof(CoflowClosure.InvokeVoid))!,
                arguments)
            : System.Linq.Expressions.Expression.Call(
                System.Linq.Expressions.Expression.Constant(closure),
                typeof(CoflowClosure).GetMethod(nameof(CoflowClosure.Invoke))!
                    .MakeGenericMethod(invoke.ReturnType),
                arguments);
        var implementation = System.Linq.Expressions.Expression
            .Lambda(expectedType, body, parameters)
            .Compile();
        Closures.Add(implementation, closure);
        return implementation;
    }

    internal static T Adapt<T>(object? value) => (T)Adapt(typeof(T), value)!;
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
        CfdSpan? sourceSpan)
    {
        Identity = identity;
        Signature = signature;
        Source = source;
        Names = names;
        SourcePath = sourcePath;
        SourceSpan = sourceSpan;
    }

    internal CoflowFunctionIdentity Identity { get; }
    internal CoflowFunctionSignature Signature { get; }
    internal CfdFunctionValue? Source { get; }
    internal CfdNameResolver Names { get; }
    internal string SourcePath { get; }
    internal CfdSpan? SourceSpan { get; }
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
            _implementation = implementation;
        }
    }

    internal void PublishCompiled(CoflowProgram? implementation)
    {
        lock (_sync)
        {
            _compiled = implementation;
            _functionsCompiled = true;
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

    private static object? InvokeAdapted(Delegate implementation, object?[] arguments)
    {
        var parameterTypes = implementation.GetType().GetMethod("Invoke")!.GetParameters();
        var adaptedArguments = new object?[arguments.Length];
        for (var index = 0; index < arguments.Length; index++)
            adaptedArguments[index] = CoflowFunctionDelegates.Adapt(
                parameterTypes[index].ParameterType, arguments[index]);
        return implementation.DynamicInvoke(adaptedArguments);
    }

    private object? InvokeCore(object?[] arguments)
    {
        CoflowProgram? compiled;
        Delegate? implementation;
        lock (_sync)
        {
            if (!_functionsCompiled) throw new CoflowFunctionNotCompiledException();
            compiled = _compiled;
            implementation = _implementation;
        }
        if (compiled is not null) return CoflowVm.Execute(compiled, arguments);
        if (implementation is not null)
        {
            try
            {
                return InvokeAdapted(implementation, arguments);
            }
            catch (Exception error)
            {
                var inner = error is System.Reflection.TargetInvocationException { InnerException: { } target }
                    ? target
                    : error;
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
        get { lock (_sync) return _compiled; }
    }
    internal bool HasBoundImplementation
    {
        get { lock (_sync) return _implementation is not null; }
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
            target._implementation = implementation;
            return true;
        }
    }

    internal object? InvokeBoundFromVm(object?[] arguments)
    {
        Delegate? implementation;
        lock (_sync) implementation = _implementation;
        if (implementation is not null)
        {
            try
            {
                return InvokeAdapted(implementation, arguments);
            }
            catch (Exception error)
            {
                var inner = error is System.Reflection.TargetInvocationException { InnerException: { } target }
                    ? target
                    : error;
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
