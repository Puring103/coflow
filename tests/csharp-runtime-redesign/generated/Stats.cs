#nullable enable
using System;
using Coflow;
namespace @Coflow.@Generated {
public readonly struct @Stats : IRuntimeValue {
private readonly RuntimeValue Value;
public @Stats(RuntimeValue value) { value.RequireContract(global::@Coflow.@Generated.Generated.ContractIdentity); Value = value; }
private T Read<T>(string field, Func<RuntimeValue,T> codec) => codec(Value.Field(field));
public RuntimeValue RuntimeValue => Value;
public int @value => Read("value", ValueCodecs.Int);
public RuntimeFunction<int,int> @transform => Read("transform", v0 => new RuntimeFunction<int,int>(v0, ValueCodecs.IntInvocation, ValueCodecs.IntInvocation));
public static global::@Coflow.@Generated.@Stats Wrap(RuntimeValue value) {
value = value.Canonical();
switch (value.TypeName) {
case "Stats": return new global::@Coflow.@Generated.@Stats(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
