using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

internal sealed class CoflowProgramLinkException : Exception
{
    internal CoflowProgramLinkException(string message) : base(message) { }
}

internal readonly struct CoflowCallSite
{
    public CoflowFunctionIdentity Identity { get; init; }
    public CoflowFunctionSignature Signature { get; init; }
    public Type[] VmParameterTypes { get; init; }
    public int ArgumentCount { get; init; }

    public CoflowCallSite(CoflowFunctionIdentity Identity, CoflowFunctionSignature Signature, Type[] VmParameterTypes, int ArgumentCount)
    {
        this.Identity = Identity;
        this.Signature = Signature;
        this.VmParameterTypes = VmParameterTypes;
        this.ArgumentCount = ArgumentCount;
    }

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

internal readonly struct CoflowFunctionReferenceTemplate
{
    public CoflowFunctionIdentity Identity { get; init; }
    public Type? ReceiverType { get; init; }

    public CoflowFunctionReferenceTemplate(CoflowFunctionIdentity Identity, Type? ReceiverType)
    {
        this.Identity = Identity;
        this.ReceiverType = ReceiverType;
    }

    internal object Link(CoflowProgramLinker linker)
    {
        var entry = linker.Function(Identity);
        return ReceiverType is null
            ? linker.FunctionHandle(entry)
            : CoflowNativeCallFactory.BindFunction(entry, ReceiverType);
    }
}

internal readonly struct CoflowRecordReferenceTemplate
{
    public string DeclaredType { get; init; }
    public string RecordKey { get; init; }

    public CoflowRecordReferenceTemplate(string DeclaredType, string RecordKey)
    {
        this.DeclaredType = DeclaredType;
        this.RecordKey = RecordKey;
    }

    internal object Link(CoflowProgramLinker linker) => linker.Record(DeclaredType, RecordKey);
}

internal readonly struct CoflowConstantReferenceTemplate
{
    public CoflowConstant Constant { get; init; }

    public CoflowConstantReferenceTemplate(CoflowConstant Constant)
    {
        this.Constant = Constant;
    }

    internal object Link(CoflowProgramLinker linker) => linker.Constant(Constant);
}
}
