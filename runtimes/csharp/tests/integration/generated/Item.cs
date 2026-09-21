#nullable enable
using System;
using Coflow;
namespace @Game.@Config {
public class @Item : RuntimeObject {
internal @Item(Projection value) : base(value) { value.RequireContract(global::@Game.@Config.Generated.ContractIdentity); }
public string Id => Read("id", __CoflowCodecs.Id);
public RuntimeDimension<string> @title => Read("title", __CoflowCodecs.F0);
public global::@Game.@Config.@ItemStats @stats => Read("stats", __CoflowCodecs.F1);
public global::@Game.@Config.@Item? @next => Read("next", __CoflowCodecs.F2);
public RuntimeFunction<int,int> @calculateFunction => Read("calculate", __CoflowCodecs.F3);
public int @calculate(int @a0) => this.@calculateFunction.Invoke(@a0);
private static class __CoflowCodecs {
internal static readonly Func<Projection,string> Id = ValueCodecs.String;
internal static readonly Func<Projection,RuntimeDimension<string>> F0 = v => new RuntimeDimension<string>(v, ValueCodecs.String);
internal static readonly Func<Projection,global::@Game.@Config.@ItemStats> F1 = global::@Game.@Config.@ItemStats.Wrap;
internal static readonly Func<Projection,global::@Game.@Config.@Item?> F2 = v0 => ValueCodecs.OptionalReference(v0, global::@Game.@Config.@Item.Wrap);
internal static readonly Func<Projection,RuntimeFunction<int,int>> F3 = v0 => new RuntimeFunction<int,int>(v0, ValueCodecs.IntInvocation, ValueCodecs.IntInvocation);
}
internal static global::@Game.@Config.@Item Wrap(Projection value) {
value = value.Canonical();
if (value.TryGetProjection<global::@Game.@Config.@Item>(out var projected)) return projected;
switch (value.TypeName) {
case "Item": return new global::@Game.@Config.@Item(value);
default: throw new CoflowException("Unexpected runtime type.");
}
}
}
}
