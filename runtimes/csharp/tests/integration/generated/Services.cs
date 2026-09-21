#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public sealed class @Services : RuntimeObject {
internal @Services(Projection value) : base(value) { value.RequireContract(global::@Game.@Config.Generated.ContractIdentity); }
public string Id => Read("id", __CoflowCodecs.Id);
public string @environment => Read("environment", __CoflowCodecs.F0);
public RuntimeFunction<int,int> @adjustFunction => Read("adjust", __CoflowCodecs.F1);
public int @adjust(int @a0) => this.@adjustFunction.Invoke(@a0);
public RuntimeFunction<int,Unit> @notifyFunction => Read("notify", __CoflowCodecs.F2);
public Unit @notify(int @a0) => this.@notifyFunction.Invoke(@a0);
private static class __CoflowCodecs {
internal static readonly Func<Projection,string> Id = ValueCodecs.String;
internal static readonly Func<Projection,string> F0 = ValueCodecs.String;
internal static readonly Func<Projection,RuntimeFunction<int,int>> F1 = v0 => new RuntimeFunction<int,int>(v0, ValueCodecs.IntInvocation, ValueCodecs.IntInvocation);
internal static readonly Func<Projection,RuntimeFunction<int,Unit>> F2 = v0 => new RuntimeFunction<int,Unit>(v0, ValueCodecs.IntInvocation, ValueCodecs.UnitInvocation);
}
internal static global::@Game.@Config.@Services Wrap(Projection value) {
value = value.Canonical();
if (value.TryGetProjection<global::@Game.@Config.@Services>(out var projected)) return projected;
switch (value.TypeName) {
case "Services": return new global::@Game.@Config.@Services(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
