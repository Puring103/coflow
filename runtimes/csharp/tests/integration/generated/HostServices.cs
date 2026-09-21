#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public sealed class @HostServices : RuntimeObject {
internal @HostServices(Projection value) : base(value) { value.RequireContract(global::@Game.@Config.Generated.ContractIdentity); }
public string Id => Read("id", __CoflowCodecs.Id);
public string @environment => Read("environment", __CoflowCodecs.F0);
public global::@Game.@Config.@Character? @favorite => Read("favorite", __CoflowCodecs.F1);
public global::@Game.@Config.@Mood @mood => Read("mood", __CoflowCodecs.F2);
public RuntimeFunction<string,Unit> @logFunction => Read("log", __CoflowCodecs.F3);
public Unit @log(string @message) => this.@logFunction.Invoke(@message);
private static class __CoflowCodecs {
internal static readonly Func<Projection,string> Id = ValueCodecs.String;
internal static readonly Func<Projection,string> F0 = ValueCodecs.String;
internal static readonly Func<Projection,global::@Game.@Config.@Character?> F1 = v0 => ValueCodecs.OptionalReference(v0, global::@Game.@Config.@Character.Wrap);
internal static readonly Func<Projection,global::@Game.@Config.@Mood> F2 = v0 => (global::@Game.@Config.@Mood)ValueCodecs.Enum(v0);
internal static readonly Func<Projection,RuntimeFunction<string,Unit>> F3 = v0 => new RuntimeFunction<string,Unit>(v0, ValueCodecs.StringInvocation, ValueCodecs.UnitInvocation);
}
internal static global::@Game.@Config.@HostServices Wrap(Projection value) {
value = value.Canonical();
if (value.TryGetProjection<global::@Game.@Config.@HostServices>(out var projected)) return projected;
switch (value.TypeName) {
case "HostServices": return new global::@Game.@Config.@HostServices(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
