#nullable enable
using System;
using Coflow;
namespace @Coflow.@Generated {
public sealed class @UiText : RuntimeObject {
public @UiText(RuntimeValue value) : base(value) { value.RequireContract(global::@Coflow.@Generated.Generated.ContractIdentity); }
public string Id => Read("id", ValueCodecs.String);
public RuntimeDimension<string> @welcome => Read("welcome", v => new RuntimeDimension<string>(v, ValueCodecs.String));
public RuntimeDimension<RuntimeArray<int>> @weights => Read("weights", v => new RuntimeDimension<RuntimeArray<int>>(v, v1 => new RuntimeArray<int>(v1, ValueCodecs.Int)));
public RuntimeDimension<global::@Coflow.@Generated.@ThemeValue> @theme => Read("theme", v => new RuntimeDimension<global::@Coflow.@Generated.@ThemeValue>(v, global::@Coflow.@Generated.@ThemeValue.Wrap));
public int @count => Read("count", ValueCodecs.Int);
public RuntimeFunction<int> @readCount => Read("readCount", v0 => new RuntimeFunction<int>(v0, ValueCodecs.IntInvocation));
public RuntimeFunction<global::@Coflow.@Generated.@ThemeValue,global::@Coflow.@Generated.@ThemeValue,bool> @sameTheme => Read("sameTheme", v0 => new RuntimeFunction<global::@Coflow.@Generated.@ThemeValue,global::@Coflow.@Generated.@ThemeValue,bool>(v0, ValueCodecs.RuntimeInvocation<global::@Coflow.@Generated.@ThemeValue>(global::@Coflow.@Generated.@ThemeValue.Wrap), ValueCodecs.RuntimeInvocation<global::@Coflow.@Generated.@ThemeValue>(global::@Coflow.@Generated.@ThemeValue.Wrap), ValueCodecs.BoolInvocation));
public static global::@Coflow.@Generated.@UiText Wrap(RuntimeValue value) {
value = value.Canonical();
switch (value.TypeName) {
case "UiText": return new global::@Coflow.@Generated.@UiText(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
