#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public sealed class @RuntimeSettings : RuntimeObject {
public @RuntimeSettings(RuntimeValue value) : base(value) { value.RequireContract(global::@Game.@Config.Generated.ContractIdentity); }
public string Id => Read("id", __CoflowCodecs.Id);
public bool @enabled => Read("enabled", __CoflowCodecs.F0);
private static class __CoflowCodecs {
internal static readonly Func<RuntimeValue,string> Id = ValueCodecs.String;
internal static readonly Func<RuntimeValue,bool> F0 = ValueCodecs.Bool;
}
public static global::@Game.@Config.@RuntimeSettings Wrap(RuntimeValue value) {
value = value.Canonical();
if (value.TryGetProjection<global::@Game.@Config.@RuntimeSettings>(out var projected)) return projected;
switch (value.TypeName) {
case "RuntimeSettings": return new global::@Game.@Config.@RuntimeSettings(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
