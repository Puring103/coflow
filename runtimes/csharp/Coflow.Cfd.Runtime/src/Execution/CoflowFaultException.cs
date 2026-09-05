namespace CoflowRuntime;

public sealed class CoflowFaultException : Exception
{
    internal CoflowFaultException(
        CoflowFunctionIdentity function,
        string sourcePath,
        CfdSpan? sourceSpan,
        IReadOnlyList<CoflowFunctionIdentity> callStack,
        string message,
        Exception? inner = null,
        bool preserveSourceLocation = false) : base(message, inner)
    {
        Function = function;
        SourcePath = sourcePath;
        SourceSpan = sourceSpan;
        CallStack = callStack;
        PreserveSourceLocation = preserveSourceLocation;
    }
    public CoflowFunctionIdentity Function { get; }
    public string SourcePath { get; }
    public CfdSpan? SourceSpan { get; }
    public IReadOnlyList<CoflowFunctionIdentity> CallStack { get; }
    internal bool PreserveSourceLocation { get; }
    internal CoflowFaultException WithCallers(
        IEnumerable<CoflowFunctionIdentity> callers,
        string? callerSourcePath = null,
        CfdSpan? callerSourceSpan = null) => new(
            Function,
            callerSourcePath ?? SourcePath,
            callerSourceSpan ?? SourceSpan,
            CallStack.Concat(callers).Distinct().Take(32).ToArray(),
            Message,
            InnerException,
            PreserveSourceLocation);
}
