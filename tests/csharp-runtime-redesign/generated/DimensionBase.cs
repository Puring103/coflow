#nullable enable
using System;
using Coflow;
namespace @Coflow.@Generated {
public class @DimensionBase : RuntimeObject {
public @DimensionBase(RuntimeValue value) : base(value) { value.RequireContract(global::@Coflow.@Generated.Generated.ContractIdentity); }
public string Id => Read("id", ValueCodecs.String);
public RuntimeDimension<string> @name => Read("name", v => new RuntimeDimension<string>(v, ValueCodecs.String));
public RuntimeDimension<string> @hint => Read("hint", v => new RuntimeDimension<string>(v, ValueCodecs.String));
public static global::@Coflow.@Generated.@DimensionBase Wrap(RuntimeValue value) {
value = value.Canonical();
switch (value.TypeName) {
case "DimensionBase": return new global::@Coflow.@Generated.@DimensionBase(value);
case "DimensionChild": return new global::@Coflow.@Generated.@DimensionChild(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
