#nullable enable
using System;
using Coflow;
namespace @Coflow.@Generated {
public class @DimensionChild : global::@Coflow.@Generated.@DimensionBase {
public @DimensionChild(RuntimeValue value) : base(value) { value.RequireContract(global::@Coflow.@Generated.Generated.ContractIdentity); }
public new static global::@Coflow.@Generated.@DimensionChild Wrap(RuntimeValue value) {
value = value.Canonical();
switch (value.TypeName) {
case "DimensionChild": return new global::@Coflow.@Generated.@DimensionChild(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
