#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public class @Hero : global::@Game.@Config.@Character {
internal @Hero(Projection value) : base(value) { value.RequireContract(global::@Game.@Config.Generated.ContractIdentity); }
public int @level => Read("level", __CoflowCodecs.F0);
private static class __CoflowCodecs {
internal static readonly Func<Projection,int> F0 = ValueCodecs.Int;
}
internal new static global::@Game.@Config.@Hero Wrap(Projection value) {
value = value.Canonical();
if (value.TryGetProjection<global::@Game.@Config.@Hero>(out var projected)) return projected;
switch (value.TypeName) {
case "Hero": return new global::@Game.@Config.@Hero(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
