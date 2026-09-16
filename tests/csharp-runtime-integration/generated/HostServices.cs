#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public sealed class @HostServices : RuntimeObject {
public @HostServices(RuntimeValue value) : base(value) { value.RequireContract(global::@Game.@Config.Generated.Contract); }
public string Id => Read("id", ValueCodecs.String);
public string @environment => Read("environment", ValueCodecs.String);
public global::@Game.@Config.@Character? @favorite => Read("favorite", v0 => ValueCodecs.OptionalReference(v0, global::@Game.@Config.@Character.Wrap));
public global::@Game.@Config.@Mood @mood => Read("mood", v0 => (global::@Game.@Config.@Mood)ValueCodecs.Enum(v0));
public RuntimeFunction @log => Read("log", v0 => new RuntimeFunction(v0));
public static global::@Game.@Config.@HostServices Wrap(RuntimeValue value) {
value = value.Canonical();
switch (value.TypeName) {
case "HostServices": return new global::@Game.@Config.@HostServices(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
