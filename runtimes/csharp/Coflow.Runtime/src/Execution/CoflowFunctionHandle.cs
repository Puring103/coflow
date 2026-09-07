using System;
using System.Linq.Expressions;
using Coflow.Runtime;

namespace Coflow.Runtime.CompilerServices;

internal static class CoflowFunctionHandle
{
    internal static bool IsFunctionType(Type type) => type.IsGenericType &&
        type.GetGenericTypeDefinition().FullName is { } name &&
        name.StartsWith("Coflow.Runtime.CoflowFunction`", StringComparison.Ordinal);

    internal static T Create<T>(CoflowFunctionId functionId, CoflowValueId environmentId) =>
        CoflowFunctionAccess<T>.Create(functionId, environmentId);

    internal static CoflowRawFunctionHandle Resolve(
        CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId) =>
        CoflowInvocationContext.CurrentExecution.FunctionHandle(id, typeId, fieldId);
}

internal static class CoflowFunctionAccess<T>
{
    internal static readonly Func<T, CoflowFunctionId> FunctionId;
    internal static readonly Func<T, CoflowValueId> EnvironmentId;
    internal static readonly Func<CoflowFunctionId, CoflowValueId, T> Create;

    static CoflowFunctionAccess()
    {
        var type = typeof(T);
        if (!CoflowFunctionHandle.IsFunctionType(type))
            throw new InvalidOperationException($"`{type}` is not a Coflow function handle.");
        var value = Expression.Parameter(type, "value");
        FunctionId = CoflowExpressionCompiler.Compile(Expression.Lambda<Func<T, CoflowFunctionId>>(
            Expression.Property(value, "FunctionId"), value));
        EnvironmentId = CoflowExpressionCompiler.Compile(Expression.Lambda<Func<T, CoflowValueId>>(
            Expression.Property(value, "EnvironmentId"), value));
        var functionId = Expression.Parameter(typeof(CoflowFunctionId), "functionId");
        var environmentId = Expression.Parameter(typeof(CoflowValueId), "environmentId");
        var constructor = type.GetConstructor(System.Reflection.BindingFlags.Instance |
            System.Reflection.BindingFlags.NonPublic, null,
            new[] { typeof(CoflowFunctionId), typeof(CoflowValueId) }, null)!;
        Create = CoflowExpressionCompiler.Compile(Expression.Lambda<Func<CoflowFunctionId, CoflowValueId, T>>(
            Expression.New(constructor, functionId, environmentId), functionId, environmentId));
    }
}

internal readonly record struct CoflowRawFunctionHandle(
    CoflowFunctionId FunctionId,
    CoflowValueId EnvironmentId) : ICoflowFunctionHandle;
