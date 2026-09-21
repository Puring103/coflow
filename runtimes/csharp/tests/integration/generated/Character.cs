#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public class @Character : RuntimeObject {
internal @Character(Projection value) : base(value) { value.RequireContract(global::@Game.@Config.Generated.ContractIdentity); }
public string Id => Read("id", __CoflowCodecs.Id);
public string @name => Read("name", __CoflowCodecs.F0);
public global::@Game.@Config.@Stats @stats => Read("stats", __CoflowCodecs.F1);
public global::@Game.@Config.@Character? @friend => Read("friend", __CoflowCodecs.F2);
public RuntimeDictionary<bool,string> @labels => Read("labels", __CoflowCodecs.F3);
public RuntimeDictionary<global::@Game.@Config.@Mood,string> @moods => Read("moods", __CoflowCodecs.F4);
public global::@Game.@Config.@Mood? @mood => Read("mood", __CoflowCodecs.F5);
public global::@Game.@Config.@Stats? @extra => Read("extra", __CoflowCodecs.F6);
public RuntimeArray<string>? @notes => Read("notes", __CoflowCodecs.F7);
public RuntimeFunction<int>? @callback => Read("callback", __CoflowCodecs.F8);
public RuntimeTemplate @text => Read("text", __CoflowCodecs.F9);
public string Rendertext() => @text.Render();
public RuntimeFunction<int,int> @scoreFunction => Read("score", __CoflowCodecs.F10);
public int @score(int @bonus) => this.@scoreFunction.Invoke(@bonus);
public RuntimeFunction<global::@Game.@Config.@Character,global::@Game.@Config.@Character> @recordIdentityFunction => Read("recordIdentity", __CoflowCodecs.F11);
public global::@Game.@Config.@Character @recordIdentity(global::@Game.@Config.@Character @value) => this.@recordIdentityFunction.Invoke(@value);
public RuntimeFunction<RuntimeArray<RuntimeFunction<int>>,RuntimeArray<RuntimeFunction<int>>> @callbacksFunction => Read("callbacks", __CoflowCodecs.F12);
public RuntimeArray<RuntimeFunction<int>> @callbacks(RuntimeArray<RuntimeFunction<int>> @values) => this.@callbacksFunction.Invoke(@values);
public RuntimeFunction<RuntimeTemplate,RuntimeTemplate> @templateIdentityFunction => Read("templateIdentity", __CoflowCodecs.F13);
public RuntimeTemplate @templateIdentity(RuntimeTemplate @value) => this.@templateIdentityFunction.Invoke(@value);
public RuntimeFunction<global::@Game.@Config.@Stats,global::@Game.@Config.@Stats> @roundtripFunction => Read("roundtrip", __CoflowCodecs.F14);
public global::@Game.@Config.@Stats @roundtrip(global::@Game.@Config.@Stats @value) => this.@roundtripFunction.Invoke(@value);
public RuntimeFunction<int,RuntimeFunction<int>> @closureFunction => Read("closure", __CoflowCodecs.F15);
public RuntimeFunction<int> @closure(int @seed) => this.@closureFunction.Invoke(@seed);
private static class __CoflowCodecs {
internal static readonly Func<Projection,string> Id = ValueCodecs.String;
internal static readonly Func<Projection,string> F0 = ValueCodecs.String;
internal static readonly Func<Projection,global::@Game.@Config.@Stats> F1 = global::@Game.@Config.@Stats.Wrap;
internal static readonly Func<Projection,global::@Game.@Config.@Character?> F2 = v0 => ValueCodecs.OptionalReference(v0, global::@Game.@Config.@Character.Wrap);
internal static readonly Func<Projection,RuntimeDictionary<bool,string>> F3 = v0 => new RuntimeDictionary<bool,string>(v0, ValueCodecs.Bool, ValueCodecs.String);
internal static readonly Func<Projection,RuntimeDictionary<global::@Game.@Config.@Mood,string>> F4 = v0 => new RuntimeDictionary<global::@Game.@Config.@Mood,string>(v0, v1 => (global::@Game.@Config.@Mood)ValueCodecs.Enum(v1), ValueCodecs.String);
internal static readonly Func<Projection,global::@Game.@Config.@Mood?> F5 = v0 => ValueCodecs.OptionalValue(v0, v1 => (global::@Game.@Config.@Mood)ValueCodecs.Enum(v1));
internal static readonly Func<Projection,global::@Game.@Config.@Stats?> F6 = v0 => ValueCodecs.OptionalValue(v0, global::@Game.@Config.@Stats.Wrap);
internal static readonly Func<Projection,RuntimeArray<string>?> F7 = v0 => ValueCodecs.OptionalReference(v0, v1 => new RuntimeArray<string>(v1, ValueCodecs.String));
internal static readonly Func<Projection,RuntimeFunction<int>?> F8 = v0 => ValueCodecs.OptionalReference(v0, v1 => new RuntimeFunction<int>(v1, ValueCodecs.IntInvocation));
internal static readonly Func<Projection,RuntimeTemplate> F9 = v => new RuntimeTemplate(v);
internal static readonly Func<Projection,RuntimeFunction<int,int>> F10 = v0 => new RuntimeFunction<int,int>(v0, ValueCodecs.IntInvocation, ValueCodecs.IntInvocation);
internal static readonly Func<Projection,RuntimeFunction<global::@Game.@Config.@Character,global::@Game.@Config.@Character>> F11 = v0 => new RuntimeFunction<global::@Game.@Config.@Character,global::@Game.@Config.@Character>(v0, ValueCodecs.RuntimeInvocation<global::@Game.@Config.@Character>(global::@Game.@Config.@Character.Wrap), ValueCodecs.RuntimeInvocation<global::@Game.@Config.@Character>(global::@Game.@Config.@Character.Wrap));
internal static readonly Func<Projection,RuntimeFunction<RuntimeArray<RuntimeFunction<int>>,RuntimeArray<RuntimeFunction<int>>>> F12 = v0 => new RuntimeFunction<RuntimeArray<RuntimeFunction<int>>,RuntimeArray<RuntimeFunction<int>>>(v0, ValueCodecs.RuntimeInvocation<RuntimeArray<RuntimeFunction<int>>>(v2 => new RuntimeArray<RuntimeFunction<int>>(v2, v3 => new RuntimeFunction<int>(v3, ValueCodecs.IntInvocation))), ValueCodecs.RuntimeInvocation<RuntimeArray<RuntimeFunction<int>>>(v2 => new RuntimeArray<RuntimeFunction<int>>(v2, v3 => new RuntimeFunction<int>(v3, ValueCodecs.IntInvocation))));
internal static readonly Func<Projection,RuntimeFunction<RuntimeTemplate,RuntimeTemplate>> F13 = v0 => new RuntimeFunction<RuntimeTemplate,RuntimeTemplate>(v0, ValueCodecs.TemplateInvocation, ValueCodecs.TemplateInvocation);
internal static readonly Func<Projection,RuntimeFunction<global::@Game.@Config.@Stats,global::@Game.@Config.@Stats>> F14 = v0 => new RuntimeFunction<global::@Game.@Config.@Stats,global::@Game.@Config.@Stats>(v0, ValueCodecs.RuntimeInvocation<global::@Game.@Config.@Stats>(global::@Game.@Config.@Stats.Wrap), ValueCodecs.RuntimeInvocation<global::@Game.@Config.@Stats>(global::@Game.@Config.@Stats.Wrap));
internal static readonly Func<Projection,RuntimeFunction<int,RuntimeFunction<int>>> F15 = v0 => new RuntimeFunction<int,RuntimeFunction<int>>(v0, ValueCodecs.IntInvocation, ValueCodecs.RuntimeInvocation<RuntimeFunction<int>>(v2 => new RuntimeFunction<int>(v2, ValueCodecs.IntInvocation)));
}
internal static global::@Game.@Config.@Character Wrap(Projection value) {
value = value.Canonical();
if (value.TryGetProjection<global::@Game.@Config.@Character>(out var projected)) return projected;
switch (value.TypeName) {
case "Character": return new global::@Game.@Config.@Character(value);
case "Hero": return new global::@Game.@Config.@Hero(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
