using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

internal readonly struct CoflowCaptureLayout
{
    public CoflowValueShape Shape { get; init; }
    public int IntegerBase { get; init; }
    public int FloatBase { get; init; }
    public int ReferenceBase { get; init; }

    public CoflowCaptureLayout(CoflowValueShape Shape, int IntegerBase, int FloatBase, int ReferenceBase)
    {
        this.Shape = Shape;
        this.IntegerBase = IntegerBase;
        this.FloatBase = FloatBase;
        this.ReferenceBase = ReferenceBase;
    }
}
}
