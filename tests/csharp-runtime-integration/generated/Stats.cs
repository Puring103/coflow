#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public readonly struct @Stats : IRuntimeValue {
private readonly RuntimeValue Value;
public @Stats(RuntimeValue value) { value.RequireContract(global::@Game.@Config.Generated.ContractIdentity); Value = value; }
private T Read<T>(string field, Func<RuntimeValue,T> codec) => Value.Field(field).ReadProjected(codec);
public RuntimeValue RuntimeValue => Value;
public @Stats(int @health, RuntimeArray<float> @weights, int? @bonus) : this(RuntimeValue.Data(global::@Game.@Config.Generated.ContractIdentity, "Stats", new string[] { "health", "weights", "bonus" }, new RuntimeValue[] { RuntimeValue.From(@health), RuntimeValue.From(@weights), RuntimeValue.From(@bonus) })) { }
public int @health => Read("health", __CoflowCodecs.F0);
public RuntimeArray<float> @weights => Read("weights", __CoflowCodecs.F1);
public int? @bonus => Read("bonus", __CoflowCodecs.F2);
private static class __CoflowCodecs {
internal static readonly Func<RuntimeValue,int> F0 = ValueCodecs.Int;
internal static readonly Func<RuntimeValue,RuntimeArray<float>> F1 = v0 => new RuntimeArray<float>(v0, ValueCodecs.Float);
internal static readonly Func<RuntimeValue,int?> F2 = v0 => ValueCodecs.OptionalValue(v0, ValueCodecs.Int);
}
public static global::@Game.@Config.@Stats Wrap(RuntimeValue value) {
value = value.Canonical();
if (value.TryGetProjection<global::@Game.@Config.@Stats>(out var projected)) return projected;
switch (value.TypeName) {
case "Stats": return new global::@Game.@Config.@Stats(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
