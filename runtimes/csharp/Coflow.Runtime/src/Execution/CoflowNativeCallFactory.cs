using System;
using System.Linq;
using System.Linq.Expressions;
using System.Reflection;

namespace Coflow.Runtime.CompilerServices;

internal static class CoflowNativeCallFactory
{
    private static readonly MethodInfo BindFunctionMethod = Method("BindFunctionCore");
    private static readonly System.Collections.Concurrent.ConcurrentDictionary<
        Type, Func<CoflowFunctionEntry, CoflowNativeCall>> BindFunctionFactories = new();

    internal static CoflowNativeCall BindFunction(CoflowFunctionEntry entry, Type receiverType) =>
        BindFunctionFactories.GetOrAdd(receiverType, static type => BuildBindFunction(type))(entry);

    private static Func<CoflowFunctionEntry, CoflowNativeCall> BuildBindFunction(Type receiverType)
    {
        // 每种 receiver 只关闭一次泛型方法；后续快照链接直接调用强类型工厂。
        var entry = Expression.Parameter(typeof(CoflowFunctionEntry), "entry");
        return CoflowExpressionCompiler.Compile(
            Expression.Lambda<Func<CoflowFunctionEntry, CoflowNativeCall>>(
                Expression.Call(BindFunctionMethod.MakeGenericMethod(receiverType), entry), entry));
    }

    private static CoflowNativeCall BindFunctionCore<TReceiver>(CoflowFunctionEntry entry)
    {
        return new CoflowNativeCall(new[] { typeof(TReceiver) }, FunctionDelegateType(entry.Signature), frame =>
        {
            var receiver = frame.Read<TReceiver>(0);
            if (receiver is null)
                throw new CoflowBoundaryException("Function receiver cannot be null.");
            if (!CoflowTypeCodecs.TryGet(receiver.GetType(), out var descriptor))
                throw new CoflowBoundaryException("Function receiver has no schema codec.");
            var kind = entry.CompiledProgram is null ? CoflowFunctionKind.Native : CoflowFunctionKind.Program;
            frame.WriteFunction(
                new CoflowFunctionId(CoflowInvocationContext.SnapshotId, kind, entry.TargetIndex),
                descriptor.GetValueIdObject(receiver));
        });
    }

    private static Type FunctionDelegateType(CoflowFunctionSignature signature)
    {
        var parameters = signature.ParameterTypes.ToArray();
        var arguments = parameters.Append(signature.ResultType).ToArray();
        var definition = arguments.Length switch
        {
            1 => typeof(CoflowFunction<>), 2 => typeof(CoflowFunction<,>),
            3 => typeof(CoflowFunction<,,>), 4 => typeof(CoflowFunction<,,,>),
            5 => typeof(CoflowFunction<,,,,>), 6 => typeof(CoflowFunction<,,,,,>),
            7 => typeof(CoflowFunction<,,,,,,>), 8 => typeof(CoflowFunction<,,,,,,,>),
            9 => typeof(CoflowFunction<,,,,,,,,>),
            _ => throw new InvalidOperationException("Coflow functions support at most eight parameters."),
        };
        return definition.MakeGenericType(arguments);
    }

    private static MethodInfo Method(string name)
    {
        return typeof(CoflowNativeCallFactory).GetMethod(
            name, BindingFlags.Static | BindingFlags.NonPublic)!;
    }
}
