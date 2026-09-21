#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public class Item : CoflowObject
{
    public string Id { get; private set; } = default!;
    public CoflowDimension<string> title { get; private set; } = default!;
    public global::Game.Config.ItemStats stats { get; private set; } = default!;
    public global::Game.Config.Item? next { get; private set; } = default!;
    public CoflowFunction<int, int> calculateFunction { get; private set; } = default!;
    public int calculate(int a0) => calculateFunction.Invoke(a0);

    internal Item(Record record) : base(record)
    {
        Id = ValueCodecs.String(record.Field("id"));
        title = new CoflowDimension<string>(record.Field("title"), ValueCodecs.String);
        stats = new global::Game.Config.ItemStats(record.Field("stats"));
        next = ValueCodecs.OptionalReference(record.Field("next"), v0 => v0.Resolve<global::Game.Config.Item>());
        calculateFunction = new CoflowFunction<int, int>(record.Field("calculate"), ValueCodecs.IntInvocation, ValueCodecs.IntInvocation);
    }
}
}
