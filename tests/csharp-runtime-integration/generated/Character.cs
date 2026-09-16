#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public class @Character : RuntimeObject {
public @Character(RuntimeValue value) : base(value) { value.RequireContract(global::@Game.@Config.Generated.Contract); }
public string Id => Read("id", ValueCodecs.String);
public string @name => Read("name", ValueCodecs.String);
public global::@Game.@Config.@Stats @stats => Read("stats", global::@Game.@Config.@Stats.Wrap);
public global::@Game.@Config.@Character? @friend => Read("friend", v0 => ValueCodecs.OptionalReference(v0, global::@Game.@Config.@Character.Wrap));
public RuntimeDictionary<bool,string> @labels => Read("labels", v0 => new RuntimeDictionary<bool,string>(v0, ValueCodecs.Bool, ValueCodecs.String, DictionaryKey.Bool));
public RuntimeDictionary<global::@Game.@Config.@Mood,string> @moods => Read("moods", v0 => new RuntimeDictionary<global::@Game.@Config.@Mood,string>(v0, v1 => (global::@Game.@Config.@Mood)ValueCodecs.Enum(v1), ValueCodecs.String, key => DictionaryKey.Enum("Mood", (uint)key)));
public global::@Game.@Config.@Mood? @mood => Read("mood", v0 => ValueCodecs.OptionalValue(v0, v1 => (global::@Game.@Config.@Mood)ValueCodecs.Enum(v1)));
public global::@Game.@Config.@Stats? @extra => Read("extra", v0 => ValueCodecs.OptionalValue(v0, global::@Game.@Config.@Stats.Wrap));
public RuntimeArray<string>? @notes => Read("notes", v0 => ValueCodecs.OptionalReference(v0, v1 => new RuntimeArray<string>(v1, ValueCodecs.String)));
public RuntimeFunction? @callback => Read("callback", v0 => ValueCodecs.OptionalReference(v0, v1 => new RuntimeFunction(v1)));
public string @text => Read("text", ValueCodecs.String);
public RuntimeValue Get_text_Template() => Value.Field("text");
public RuntimeFunction @score => Read("score", v0 => new RuntimeFunction(v0));
public static global::@Game.@Config.@Character Wrap(RuntimeValue value) {
value = value.Canonical();
switch (value.TypeName) {
case "Character": return new global::@Game.@Config.@Character(value);
case "Hero": return new global::@Game.@Config.@Hero(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
