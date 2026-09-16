#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public class @LocalizedText : RuntimeObject {
public @LocalizedText(RuntimeValue value) : base(value) { value.RequireContract(global::@Game.@Config.Generated.ContractIdentity); }
public string Id => Read("id", ValueCodecs.String);
public RuntimeDimension<string> @value => Read("value", v => new RuntimeDimension<string>(v, ValueCodecs.String));
public static global::@Game.@Config.@LocalizedText Wrap(RuntimeValue value) {
value = value.Canonical();
switch (value.TypeName) {
case "LocalizedText": return new global::@Game.@Config.@LocalizedText(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
