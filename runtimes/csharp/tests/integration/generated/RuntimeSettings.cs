#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public sealed class @RuntimeSettings : RuntimeObject {
internal @RuntimeSettings(Projection value) : base(value) { value.RequireContract(global::@Game.@Config.Generated.ContractIdentity); }
public string Id => Read("id", __CoflowCodecs.Id);
public bool @enabled => Read("enabled", __CoflowCodecs.F0);
private static class __CoflowCodecs {
internal static readonly Func<Projection,string> Id = ValueCodecs.String;
internal static readonly Func<Projection,bool> F0 = ValueCodecs.Bool;
}
internal static global::@Game.@Config.@RuntimeSettings Wrap(Projection value) {
value = value.Canonical();
if (value.TryGetProjection<global::@Game.@Config.@RuntimeSettings>(out var projected)) return projected;
switch (value.TypeName) {
case "RuntimeSettings": return new global::@Game.@Config.@RuntimeSettings(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
