namespace CoflowRuntime;

public sealed class CoflowFunctionNotCompiledException : InvalidOperationException
{
    public CoflowFunctionNotCompiledException()
        : base("Coflow functions are unavailable because the data was loaded with CoflowData.Load.") { }
}

public sealed class CoflowFunctionNotBoundException : InvalidOperationException
{
    public CoflowFunctionNotBoundException()
        : base("The Coflow function has no CFD body or bound C# implementation.") { }
}

public sealed class CoflowHostNotBoundException : InvalidOperationException
{
    public CoflowHostNotBoundException()
        : base("The generated @Host singleton has not been configured.") { }
}

public readonly record struct CoflowFunctionIdentity(
    string DeclaredType,
    string RecordKey,
    string FieldName,
    string ValuePath = "");
