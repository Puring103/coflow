using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

using System.ComponentModel;

[EditorBrowsable(EditorBrowsableState.Never)]
public readonly struct CoflowTypeId
{
    public int Value { get; init; }

    public CoflowTypeId(int Value)
    {
        this.Value = Value;
    }

    public bool IsValid => Value > 0;
}

[EditorBrowsable(EditorBrowsableState.Never)]
public readonly struct CoflowFieldId
{
    public int Value { get; init; }

    public CoflowFieldId(int Value)
    {
        this.Value = Value;
    }

    public bool IsValid => Value >= 0;
}
}
