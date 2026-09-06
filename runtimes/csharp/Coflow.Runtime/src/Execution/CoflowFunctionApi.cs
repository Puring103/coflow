namespace Coflow.Runtime;

public sealed class CoflowStaleValueException : InvalidOperationException
{
    public CoflowStaleValueException()
        : base("The value does not belong to the current Coflow snapshot.") { }
}

public sealed class CoflowBoundaryException : InvalidOperationException
{
    public CoflowBoundaryException(string message) : base(message) { }
}

public sealed class CoflowFunctionNotBoundException : InvalidOperationException
{
    public CoflowFunctionNotBoundException()
        : base("The Coflow function has no CFD body or bound C# implementation.") { }
}

public sealed class CoflowHostNotBoundException : InvalidOperationException
{
    public CoflowHostNotBoundException()
        : base("The @Host singleton has not been bound.") { }
}

public readonly record struct CoflowFunctionIdentity(
    string DeclaredType,
    string RecordKey,
    string FieldName,
    string ValuePath = "");
