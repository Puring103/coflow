using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

internal readonly struct CoflowLinkedFunction
{
    public CoflowProgram? Program { get; init; }
    public CoflowFunctionEntry Entry { get; init; }

    public CoflowLinkedFunction(CoflowProgram? Program, CoflowFunctionEntry Entry)
    {
        this.Program = Program;
        this.Entry = Entry;
    }
}
}
