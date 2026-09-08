using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;

namespace Coflow.Runtime.CompilerServices
{

internal readonly struct CoflowHigherOrderOperation
{
    public string Name { get; init; }
    public Type ElementType { get; init; }
    public Type OutputElementType { get; init; }
    public Type ResultType { get; init; }

    public CoflowHigherOrderOperation(string Name, Type ElementType, Type OutputElementType, Type ResultType)
    {
        this.Name = Name;
        this.ElementType = ElementType;
        this.OutputElementType = OutputElementType;
        this.ResultType = ResultType;
    }
}
}
