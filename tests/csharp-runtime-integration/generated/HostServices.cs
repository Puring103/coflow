using System;
using Coflow.Runtime;
namespace @Game.@Config {
public sealed class @HostServices : CoflowObject {
public @HostServices(CoflowValue value) : base(value) { value.RequireContract(global::@Game.@Config.CoflowSchema.Identity); }
public string Id => Read("id", CoflowCodecs.String);
public string @environment => Read("environment", CoflowCodecs.String);
public CoflowFunction @log => Read("log", v0 => new CoflowFunction(v0));
public static global::@Game.@Config.@HostServices Wrap(CoflowValue value) {
switch (value.TypeName) {
case "HostServices": return new global::@Game.@Config.@HostServices(value);
default: value.Dispose(); throw new CoflowException("Unexpected runtime type.");
}
}
}
}
