#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public readonly struct Stats : IRuntimeArgument
{
    private readonly Projection _value;
    public int health { get; }
    public RuntimeArray<float> weights { get; }
    public int? bonus { get; }

    internal Stats(Projection projection)
    {
        projection.RequireContract(global::Game.Config.Generated.ContractIdentity);
        _value = projection;
        health = ValueCodecs.Int(projection.Field("health"));
        weights = new RuntimeArray<float>(projection.Field("weights"), ValueCodecs.Float);
        bonus = ValueCodecs.OptionalValue(projection.Field("bonus"), ValueCodecs.Int);
    }

    void IRuntimeArgument.Encode(ArgumentWriter writer) => writer.Write(_value);

    public Stats(int health, RuntimeArray<float> weights, int? bonus) : this(Projection.Data(global::Game.Config.Generated.ContractIdentity, "Stats", new[] { "health", "weights", "bonus" }, new[] { Projection.From(health), Projection.From(weights), Projection.From(bonus) })) { }
}
}
