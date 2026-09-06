namespace Coflow.Runtime.CompilerServices;

using System.ComponentModel;

[EditorBrowsable(EditorBrowsableState.Never)]
public readonly record struct CoflowTypeId(int Value)
{
    public bool IsValid => Value > 0;
}

[EditorBrowsable(EditorBrowsableState.Never)]
public readonly record struct CoflowFieldId(int Value)
{
    public bool IsValid => Value >= 0;
}
