#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public class @DimensionBase : RuntimeObject {
internal @DimensionBase(Projection value) : base(value) { value.RequireContract(global::@Game.@Config.Generated.ContractIdentity); }
public string Id => Read("id", __CoflowCodecs.Id);
public RuntimeDimension<string> @name => Read("name", __CoflowCodecs.F0);
public RuntimeDimension<string> @hint => Read("hint", __CoflowCodecs.F1);
private static class __CoflowCodecs {
internal static readonly Func<Projection,string> Id = ValueCodecs.String;
internal static readonly Func<Projection,RuntimeDimension<string>> F0 = v => new RuntimeDimension<string>(v, ValueCodecs.String);
internal static readonly Func<Projection,RuntimeDimension<string>> F1 = v => new RuntimeDimension<string>(v, ValueCodecs.String);
}
internal static global::@Game.@Config.@DimensionBase Wrap(Projection value) {
value = value.Canonical();
if (value.TryGetProjection<global::@Game.@Config.@DimensionBase>(out var projected)) return projected;
switch (value.TypeName) {
case "DimensionBase": return new global::@Game.@Config.@DimensionBase(value);
case "DimensionChild": return new global::@Game.@Config.@DimensionChild(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
