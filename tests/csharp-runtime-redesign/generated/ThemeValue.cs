#nullable enable
using System;
using Coflow;
namespace @Coflow.@Generated {
public readonly struct @ThemeValue : IRuntimeValue {
private readonly RuntimeValue Value;
public @ThemeValue(RuntimeValue value) { value.RequireContract(global::@Coflow.@Generated.Generated.ContractIdentity); Value = value; }
private T Read<T>(string field, Func<RuntimeValue,T> codec) => codec(Value.Field(field));
public RuntimeValue RuntimeValue => Value;
public int @value => Read("value", ValueCodecs.Int);
public static global::@Coflow.@Generated.@ThemeValue Wrap(RuntimeValue value) {
value = value.Canonical();
switch (value.TypeName) {
case "ThemeValue": return new global::@Coflow.@Generated.@ThemeValue(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
