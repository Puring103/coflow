#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public sealed class @RuntimeSettings : RuntimeObject {
public @RuntimeSettings(RuntimeValue value) : base(value) { value.RequireContract(global::@Game.@Config.Generated.ContractIdentity); }
public string Id => Read("id", ValueCodecs.String);
public bool @enabled => Read("enabled", ValueCodecs.Bool);
public static global::@Game.@Config.@RuntimeSettings Wrap(RuntimeValue value) {
value = value.Canonical();
switch (value.TypeName) {
case "RuntimeSettings": return new global::@Game.@Config.@RuntimeSettings(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
