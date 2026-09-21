#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public class Item : RuntimeObject
{
    public string Id { get; private set; } = default!;
    public RuntimeDimension<string> title { get; private set; } = default!;
    public global::Game.Config.ItemStats stats { get; private set; } = default!;
    public global::Game.Config.Item? next { get; private set; } = default!;
    public RuntimeFunction<int, int> calculateFunction { get; private set; } = default!;
    public int calculate(int a0) => calculateFunction.Invoke(a0);

    internal Item(Record record) : base(record)
    {
        Id = ValueCodecs.String(record.Field("id"));
        title = new RuntimeDimension<string>(record.Field("title"), ValueCodecs.String);
        stats = new global::Game.Config.ItemStats(record.Field("stats"));
        next = ValueCodecs.OptionalReference(record.Field("next"), v0 => v0.Resolve<global::Game.Config.Item>());
        calculateFunction = new RuntimeFunction<int, int>(record.Field("calculate"), ValueCodecs.IntInvocation, ValueCodecs.IntInvocation);
    }
}
}
