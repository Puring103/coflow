using System;
using Coflow.Runtime;
namespace @Game.@Config {
public class @Character : CoflowObject {
public @Character(CoflowValue value) : base(value) { value.RequireContract(global::@Game.@Config.CoflowSchema.Identity); }
public string Id => Read("id", CoflowCodecs.String);
public string @name => Read("name", CoflowCodecs.String);
public global::@Game.@Config.@Stats @stats => Read("stats", global::@Game.@Config.@Stats.Wrap);
public CoflowOptional<global::@Game.@Config.@Character> @friend => Read("friend", v0 => new CoflowOptional<global::@Game.@Config.@Character>(v0, global::@Game.@Config.@Character.Wrap));
public CoflowDictionary<bool,string> @labels => Read("labels", v0 => new CoflowDictionary<bool,string>(v0, CoflowCodecs.Bool, CoflowCodecs.String));
public string @text => Read("text", CoflowCodecs.String);
public CoflowValue Get_text_Template() => Value.Field("text");
public CoflowFunction @score => Read("score", v0 => new CoflowFunction(v0));
public static global::@Game.@Config.@Character Wrap(CoflowValue value) {
switch (value.TypeName) {
case "Character": return new global::@Game.@Config.@Character(value);
default: value.Dispose(); throw new CoflowException("Unexpected runtime type.");
}
}
}
}
