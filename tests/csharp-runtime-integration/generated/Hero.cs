#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public class @Hero : global::@Game.@Config.@Character {
public @Hero(RuntimeValue value) : base(value) { value.RequireContract(global::@Game.@Config.Generated.ContractIdentity); }
public int @level => Read("level", __CoflowCodecs.F0);
private static class __CoflowCodecs {
internal static readonly Func<RuntimeValue,int> F0 = ValueCodecs.Int;
}
public new static global::@Game.@Config.@Hero Wrap(RuntimeValue value) {
value = value.Canonical();
if (value.TryGetProjection<global::@Game.@Config.@Hero>(out var projected)) return projected;
switch (value.TypeName) {
case "Hero": return new global::@Game.@Config.@Hero(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
