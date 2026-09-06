namespace Coflow.Runtime.CompilerServices;

internal sealed class CoflowProgramLinkException(string message) : Exception(message);

internal readonly record struct CoflowCallSite(
    CoflowFunctionIdentity Identity,
    CoflowFunctionSignature Signature,
    Type[] VmParameterTypes,
    int ArgumentCount)
{
    internal static CoflowCallSite From(CoflowFunctionEntry entry, int argumentCount)
    {
        // 未链接调用只保留稳定符号和 ABI，不得嵌入当前快照的函数实体或索引。
        return new CoflowCallSite(
            entry.Identity,
            entry.Signature,
            entry.VmParameterTypes.ToArray(),
            argumentCount);
    }
}

internal readonly record struct CoflowFunctionReferenceTemplate(
    CoflowFunctionIdentity Identity,
    Type? ReceiverType)
{
    internal object Link(CoflowProgramLinker linker)
    {
        var entry = linker.Function(Identity);
        return ReceiverType is null
            ? linker.FunctionHandle(entry)
            : CoflowNativeCallFactory.BindFunction(entry, ReceiverType);
    }
}

internal readonly record struct CoflowRecordReferenceTemplate(
    string DeclaredType,
    string RecordKey)
{
    internal object Link(CoflowProgramLinker linker) => linker.Record(DeclaredType, RecordKey);
}

internal readonly record struct CoflowConstantReferenceTemplate(CoflowConstant Constant)
{
    internal object Link(CoflowProgramLinker linker) => linker.Constant(Constant);
}
