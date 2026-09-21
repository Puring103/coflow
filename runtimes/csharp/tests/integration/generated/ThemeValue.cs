#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public readonly struct @ThemeValue : IRuntimeArgument {
private readonly Projection Value;
internal @ThemeValue(Projection value) { value.RequireContract(global::@Game.@Config.Generated.ContractIdentity); Value = value; }
private T Read<T>(string field, Func<Projection,T> codec) => Value.Field(field).ReadProjected(codec);
void IRuntimeArgument.Encode(ArgumentWriter writer) => writer.Write(Value);
public @ThemeValue(int @value) : this(Projection.Data(global::@Game.@Config.Generated.ContractIdentity, "ThemeValue", new string[] { "value" }, new Projection[] { Projection.From(@value) })) { }
public int @value => Read("value", __CoflowCodecs.F0);
private static class __CoflowCodecs {
internal static readonly Func<Projection,int> F0 = ValueCodecs.Int;
}
internal static global::@Game.@Config.@ThemeValue Wrap(Projection value) {
value = value.Canonical();
if (value.TryGetProjection<global::@Game.@Config.@ThemeValue>(out var projected)) return projected;
switch (value.TypeName) {
case "ThemeValue": return new global::@Game.@Config.@ThemeValue(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
