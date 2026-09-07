using System;
using System.Linq;

namespace Coflow.Runtime.CompilerServices;

internal static class CoflowNativeCallFactory
{
    internal static CoflowNativeCall BindFunction(CoflowFunctionEntry entry, Type receiverType)
    {
        return new CoflowNativeCall(new[] { receiverType }, FunctionDelegateType(entry.Signature), frame =>
        {
            var receiver = frame.ReadRecord(0, receiverType);
            if (!CoflowSchemaRuntimeContext.TryGetTypeCodec(receiver.GetType(), out var descriptor))
                throw new CoflowBoundaryException("Function receiver has no schema codec.");
            var kind = entry.CompiledProgram is null ? CoflowFunctionKind.Native : CoflowFunctionKind.Program;
            frame.WriteFunction(
                new CoflowFunctionId(CoflowInvocationContext.CurrentExecution.SnapshotId, kind, entry.TargetIndex),
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

}
