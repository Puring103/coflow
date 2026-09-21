#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public class @DimensionChild : global::@Game.@Config.@DimensionBase {
internal @DimensionChild(Projection value) : base(value) { value.RequireContract(global::@Game.@Config.Generated.ContractIdentity); }
private static class __CoflowCodecs {
}
internal new static global::@Game.@Config.@DimensionChild Wrap(Projection value) {
value = value.Canonical();
if (value.TryGetProjection<global::@Game.@Config.@DimensionChild>(out var projected)) return projected;
switch (value.TypeName) {
case "DimensionChild": return new global::@Game.@Config.@DimensionChild(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
