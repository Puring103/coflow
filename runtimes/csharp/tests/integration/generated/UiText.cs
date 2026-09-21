#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public class @UiText : RuntimeObject {
internal @UiText(Projection value) : base(value) { value.RequireContract(global::@Game.@Config.Generated.ContractIdentity); }
public string Id => Read("id", __CoflowCodecs.Id);
public RuntimeDimension<string> @welcome => Read("welcome", __CoflowCodecs.F0);
public RuntimeDimension<RuntimeArray<int>> @weights => Read("weights", __CoflowCodecs.F1);
public RuntimeDimension<global::@Game.@Config.@ThemeValue> @theme => Read("theme", __CoflowCodecs.F2);
public int @count => Read("count", __CoflowCodecs.F3);
public RuntimeFunction<int> @readCountFunction => Read("readCount", __CoflowCodecs.F4);
public int @readCount() => this.@readCountFunction.Invoke();
public RuntimeFunction<global::@Game.@Config.@ThemeValue,global::@Game.@Config.@ThemeValue,bool> @sameThemeFunction => Read("sameTheme", __CoflowCodecs.F5);
public bool @sameTheme(global::@Game.@Config.@ThemeValue @a0, global::@Game.@Config.@ThemeValue @a1) => this.@sameThemeFunction.Invoke(@a0, @a1);
private static class __CoflowCodecs {
internal static readonly Func<Projection,string> Id = ValueCodecs.String;
internal static readonly Func<Projection,RuntimeDimension<string>> F0 = v => new RuntimeDimension<string>(v, ValueCodecs.String);
internal static readonly Func<Projection,RuntimeDimension<RuntimeArray<int>>> F1 = v => new RuntimeDimension<RuntimeArray<int>>(v, v1 => new RuntimeArray<int>(v1, ValueCodecs.Int));
internal static readonly Func<Projection,RuntimeDimension<global::@Game.@Config.@ThemeValue>> F2 = v => new RuntimeDimension<global::@Game.@Config.@ThemeValue>(v, global::@Game.@Config.@ThemeValue.Wrap);
internal static readonly Func<Projection,int> F3 = ValueCodecs.Int;
internal static readonly Func<Projection,RuntimeFunction<int>> F4 = v0 => new RuntimeFunction<int>(v0, ValueCodecs.IntInvocation);
internal static readonly Func<Projection,RuntimeFunction<global::@Game.@Config.@ThemeValue,global::@Game.@Config.@ThemeValue,bool>> F5 = v0 => new RuntimeFunction<global::@Game.@Config.@ThemeValue,global::@Game.@Config.@ThemeValue,bool>(v0, ValueCodecs.RuntimeInvocation<global::@Game.@Config.@ThemeValue>(global::@Game.@Config.@ThemeValue.Wrap), ValueCodecs.RuntimeInvocation<global::@Game.@Config.@ThemeValue>(global::@Game.@Config.@ThemeValue.Wrap), ValueCodecs.BoolInvocation);
}
internal static global::@Game.@Config.@UiText Wrap(Projection value) {
value = value.Canonical();
if (value.TryGetProjection<global::@Game.@Config.@UiText>(out var projected)) return projected;
switch (value.TypeName) {
case "UiText": return new global::@Game.@Config.@UiText(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
