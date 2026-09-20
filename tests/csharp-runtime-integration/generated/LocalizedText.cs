#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public class @LocalizedText : RuntimeObject {
public @LocalizedText(RuntimeValue value) : base(value) { value.RequireContract(global::@Game.@Config.Generated.ContractIdentity); }
public string Id => Read("id", __CoflowCodecs.Id);
public RuntimeDimension<string> @value => Read("value", __CoflowCodecs.F0);
public RuntimeDimension<string?> @optional => Read("optional", __CoflowCodecs.F1);
private static class __CoflowCodecs {
internal static readonly Func<RuntimeValue,string> Id = ValueCodecs.String;
internal static readonly Func<RuntimeValue,RuntimeDimension<string>> F0 = v => new RuntimeDimension<string>(v, ValueCodecs.String);
internal static readonly Func<RuntimeValue,RuntimeDimension<string?>> F1 = v => new RuntimeDimension<string?>(v, v1 => ValueCodecs.OptionalReference(v1, ValueCodecs.String));
}
public static global::@Game.@Config.@LocalizedText Wrap(RuntimeValue value) {
value = value.Canonical();
if (value.TryGetProjection<global::@Game.@Config.@LocalizedText>(out var projected)) return projected;
switch (value.TypeName) {
case "LocalizedText": return new global::@Game.@Config.@LocalizedText(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
