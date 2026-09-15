using System;
using Coflow.Runtime;
namespace @Game.@Config {
public readonly struct @Stats : IDisposable, ICoflowValue {
private readonly CoflowValue Value;
public @Stats(CoflowValue value) { value.RequireContract(global::@Game.@Config.CoflowSchema.Identity); Value = value; }
private T Read<T>(string field, Func<CoflowValue,T> codec) => codec(Value.Field(field));
public CoflowValue RetainValue() => Value.Retain();
public void Dispose() => Value.Dispose();
public int @health => Read("health", CoflowCodecs.Int);
public CoflowArray<float> @weights => Read("weights", v0 => new CoflowArray<float>(v0, CoflowCodecs.Float));
public static global::@Game.@Config.@Stats Wrap(CoflowValue value) {
switch (value.TypeName) {
case "Stats": return new global::@Game.@Config.@Stats(value);
default: value.Dispose(); throw new CoflowException("Unexpected runtime type.");
}
}
}
}
