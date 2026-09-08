using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime
{

public sealed class CoflowStaleValueException : InvalidOperationException
{
    public CoflowStaleValueException()
        : base("The value does not belong to the current Coflow snapshot.") { }

    internal CoflowStaleValueException(string details)
        : base($"The value does not belong to the current Coflow snapshot. {details}") { }
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

public readonly struct CoflowFunctionIdentity
{
    public string DeclaredType { get; init; }
    public string RecordKey { get; init; }
    public string FieldName { get; init; }
    public string ValuePath { get; init; }

    public CoflowFunctionIdentity(string DeclaredType, string RecordKey, string FieldName, string ValuePath = "")
    {
        this.DeclaredType = DeclaredType;
        this.RecordKey = RecordKey;
        this.FieldName = FieldName;
        this.ValuePath = ValuePath;
    }
}
}
