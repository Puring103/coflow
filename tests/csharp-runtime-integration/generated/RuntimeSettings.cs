using System;
using Coflow.Runtime;
namespace @Game.@Config {
public sealed class @RuntimeSettings : CoflowObject {
public @RuntimeSettings(CoflowValue value) : base(value) { value.RequireContract(global::@Game.@Config.CoflowSchema.Identity); }
public string Id => Read("id", CoflowCodecs.String);
public bool @enabled => Read("enabled", CoflowCodecs.Bool);
public static global::@Game.@Config.@RuntimeSettings Wrap(CoflowValue value) {
switch (value.TypeName) {
case "RuntimeSettings": return new global::@Game.@Config.@RuntimeSettings(value);
default: value.Dispose(); throw new CoflowException("Unexpected runtime type.");
}
}
}
}
