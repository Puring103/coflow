#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public readonly struct ItemStats : ICoflowValue
{
    private readonly Projection _value;
    public int value { get; }
    public CoflowFunction<int, int> transformFunction { get; }
    public int transform(int a0) => transformFunction.Invoke(a0);

    internal ItemStats(Projection projection)
    {
        projection.RequireContract(global::Game.Config.Generated.ContractIdentity);
        _value = projection;
        value = ValueCodecs.Int(projection.Field("value"));
        transformFunction = new CoflowFunction<int, int>(projection.Field("transform"), ValueCodecs.IntInvocation, ValueCodecs.IntInvocation);
    }

    void ICoflowValue.Encode(ArgumentWriter writer) => writer.Write(_value);

    public ItemStats(int value, CoflowFunction<int, int> transform) : this(Projection.Data(global::Game.Config.Generated.ContractIdentity, "ItemStats", new[] { "value", "transform" }, new[] { Projection.From(value), Projection.From(transform) })) { }
}
}
