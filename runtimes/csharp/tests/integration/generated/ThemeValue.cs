#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public readonly struct ThemeValue : ICoflowValue
{
    private readonly Projection _value;
    public int value { get; }

    internal ThemeValue(Projection projection)
    {
        projection.RequireContract(global::Game.Config.Generated.ContractIdentity);
        _value = projection;
        value = ValueCodecs.Int(projection.Field("value"));
    }

    void ICoflowValue.Encode(ArgumentWriter writer) => writer.Write(_value);

    public ThemeValue(int value) : this(Projection.Data(global::Game.Config.Generated.ContractIdentity, "ThemeValue", new[] { "value" }, new[] { Projection.From(value) })) { }
}
}
