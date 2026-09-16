#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public readonly struct @Stats : IRuntimeValue {
private readonly RuntimeValue Value;
public @Stats(RuntimeValue value) { value.RequireContract(global::@Game.@Config.Generated.Contract); Value = value; }
private T Read<T>(string field, Func<RuntimeValue,T> codec) => codec(Value.Field(field));
public RuntimeValue RuntimeValue => Value;
public int @health => Read("health", ValueCodecs.Int);
public RuntimeArray<float> @weights => Read("weights", v0 => new RuntimeArray<float>(v0, ValueCodecs.Float));
public int? @bonus => Read("bonus", v0 => ValueCodecs.OptionalValue(v0, ValueCodecs.Int));
public static global::@Game.@Config.@Stats Wrap(RuntimeValue value) {
value = value.Canonical();
switch (value.TypeName) {
case "Stats": return new global::@Game.@Config.@Stats(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
