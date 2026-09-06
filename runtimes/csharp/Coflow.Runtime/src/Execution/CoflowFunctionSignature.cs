namespace Coflow.Runtime.CompilerServices;

internal sealed class CoflowFunctionSignature
{
    public CoflowFunctionSignature(Type resultType, IReadOnlyList<Type> parameterTypes)
    {
        ResultType = resultType ?? throw new ArgumentNullException(nameof(resultType));
        ParameterTypes = parameterTypes ?? throw new ArgumentNullException(nameof(parameterTypes));
    }

    public Type ResultType { get; }
    public IReadOnlyList<Type> ParameterTypes { get; }
}
