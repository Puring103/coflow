#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public readonly struct @ItemStats : IRuntimeArgument {
private readonly Projection Value;
internal @ItemStats(Projection value) { value.RequireContract(global::@Game.@Config.Generated.ContractIdentity); Value = value; }
private T Read<T>(string field, Func<Projection,T> codec) => Value.Field(field).ReadProjected(codec);
void IRuntimeArgument.Encode(ArgumentWriter writer) => writer.Write(Value);
public @ItemStats(int @value, RuntimeFunction<int,int> @transform) : this(Projection.Data(global::@Game.@Config.Generated.ContractIdentity, "ItemStats", new string[] { "value", "transform" }, new Projection[] { Projection.From(@value), Projection.From(@transform) })) { }
public int @value => Read("value", __CoflowCodecs.F0);
public RuntimeFunction<int,int> @transformFunction => Read("transform", __CoflowCodecs.F1);
public int @transform(int @a0) => this.@transformFunction.Invoke(@a0);
private static class __CoflowCodecs {
internal static readonly Func<Projection,int> F0 = ValueCodecs.Int;
internal static readonly Func<Projection,RuntimeFunction<int,int>> F1 = v0 => new RuntimeFunction<int,int>(v0, ValueCodecs.IntInvocation, ValueCodecs.IntInvocation);
}
internal static global::@Game.@Config.@ItemStats Wrap(Projection value) {
value = value.Canonical();
if (value.TryGetProjection<global::@Game.@Config.@ItemStats>(out var projected)) return projected;
switch (value.TypeName) {
case "ItemStats": return new global::@Game.@Config.@ItemStats(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
