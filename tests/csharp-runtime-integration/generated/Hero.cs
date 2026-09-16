#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public class @Hero : global::@Game.@Config.@Character {
public @Hero(RuntimeValue value) : base(value) { value.RequireContract(global::@Game.@Config.Generated.Contract); }
public int @level => Read("level", ValueCodecs.Int);
public new static global::@Game.@Config.@Hero Wrap(RuntimeValue value) {
value = value.Canonical();
switch (value.TypeName) {
case "Hero": return new global::@Game.@Config.@Hero(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
