#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public readonly struct Stats : ICoflowValue
{
    private readonly Projection _value;
    public int health { get; }
    public CoflowArray<float> weights { get; }
    public int? bonus { get; }

    internal Stats(Projection projection)
    {
        projection.RequireContract(global::Game.Config.Generated.ContractIdentity);
        _value = projection;
        health = ValueCodecs.Int(projection.Field("health"));
        weights = new CoflowArray<float>(projection.Field("weights"), ValueCodecs.Float);
        bonus = ValueCodecs.OptionalValue(projection.Field("bonus"), ValueCodecs.Int);
    }

    void ICoflowValue.Encode(ArgumentWriter writer) => writer.Write(_value);

    public Stats(int health, CoflowArray<float> weights, int? bonus) : this(Projection.Data(global::Game.Config.Generated.ContractIdentity, "Stats", new[] { "health", "weights", "bonus" }, new[] { Projection.From(health), Projection.From(weights), Projection.From(bonus) })) { }
}
}
