#nullable enable
using System;
using Coflow;
namespace @Coflow.@Generated {
public sealed class @Services : RuntimeObject {
public @Services(RuntimeValue value) : base(value) { value.RequireContract(global::@Coflow.@Generated.Generated.ContractIdentity); }
public string Id => Read("id", ValueCodecs.String);
public string @environment => Read("environment", ValueCodecs.String);
public RuntimeFunction<int,int> @adjust => Read("adjust", v0 => new RuntimeFunction<int,int>(v0, ValueCodecs.IntInvocation, ValueCodecs.IntInvocation));
public RuntimeFunction<int,Unit> @notify => Read("notify", v0 => new RuntimeFunction<int,Unit>(v0, ValueCodecs.IntInvocation, ValueCodecs.UnitInvocation));
public static global::@Coflow.@Generated.@Services Wrap(RuntimeValue value) {
value = value.Canonical();
switch (value.TypeName) {
case "Services": return new global::@Coflow.@Generated.@Services(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
