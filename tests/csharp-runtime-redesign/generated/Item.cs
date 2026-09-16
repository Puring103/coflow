#nullable enable
using System;
using Coflow;
namespace @Coflow.@Generated {
public class @Item : RuntimeObject {
public @Item(RuntimeValue value) : base(value) { value.RequireContract(global::@Coflow.@Generated.Generated.ContractIdentity); }
public string Id => Read("id", ValueCodecs.String);
public RuntimeDimension<string> @title => Read("title", v => new RuntimeDimension<string>(v, ValueCodecs.String));
public global::@Coflow.@Generated.@Stats @stats => Read("stats", global::@Coflow.@Generated.@Stats.Wrap);
public global::@Coflow.@Generated.@Item? @next => Read("next", v0 => ValueCodecs.OptionalReference(v0, global::@Coflow.@Generated.@Item.Wrap));
public RuntimeFunction<int,int> @calculate => Read("calculate", v0 => new RuntimeFunction<int,int>(v0, ValueCodecs.IntInvocation, ValueCodecs.IntInvocation));
public static global::@Coflow.@Generated.@Item Wrap(RuntimeValue value) {
value = value.Canonical();
switch (value.TypeName) {
case "Item": return new global::@Coflow.@Generated.@Item(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
