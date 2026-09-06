namespace Coflow.Runtime.CompilerServices;

using System.ComponentModel;

[EditorBrowsable(EditorBrowsableState.Never)]
public readonly struct CoflowValueId : IEquatable<CoflowValueId>
{
    private readonly ulong _value;

    public CoflowValueId(uint generation, uint index)
    {
        _value = ((ulong)generation << 32) | index;
    }

    public static CoflowValueId Invalid => default;
    public bool IsValid => _value != 0;
    internal uint Generation => (uint)(_value >> 32);
    internal uint Index => (uint)_value;
    internal ulong Packed => _value;
    internal static CoflowValueId FromPacked(ulong value) => new((uint)(value >> 32), (uint)value);
    public bool Equals(CoflowValueId other) => _value == other._value;
    public override bool Equals(object? obj) => obj is CoflowValueId other && Equals(other);
    public override int GetHashCode() => _value.GetHashCode();
    public static bool operator ==(CoflowValueId left, CoflowValueId right) => left.Equals(right);
    public static bool operator !=(CoflowValueId left, CoflowValueId right) => !left.Equals(right);
}
