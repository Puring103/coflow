#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public class UiText : CoflowObject
{
    public string Id { get; private set; } = default!;
    public CoflowDimension<string> welcome { get; private set; } = default!;
    public CoflowDimension<CoflowArray<int>> weights { get; private set; } = default!;
    public CoflowDimension<global::Game.Config.ThemeValue> theme { get; private set; } = default!;
    public int count { get; private set; } = default!;
    public CoflowFunction<int> readCountFunction { get; private set; } = default!;
    public int readCount() => readCountFunction.Invoke();
    public CoflowFunction<global::Game.Config.ThemeValue, global::Game.Config.ThemeValue, bool> sameThemeFunction { get; private set; } = default!;
    public bool sameTheme(global::Game.Config.ThemeValue a0, global::Game.Config.ThemeValue a1) => sameThemeFunction.Invoke(a0, a1);

    internal UiText(Record record) : base(record)
    {
        Id = ValueCodecs.String(record.Field("id"));
        welcome = new CoflowDimension<string>(record.Field("welcome"), ValueCodecs.String);
        weights = new CoflowDimension<CoflowArray<int>>(record.Field("weights"), v0 => new CoflowArray<int>(v0, ValueCodecs.Int));
        theme = new CoflowDimension<global::Game.Config.ThemeValue>(record.Field("theme"), v0 => new global::Game.Config.ThemeValue(v0));
        count = ValueCodecs.Int(record.Field("count"));
        readCountFunction = new CoflowFunction<int>(record.Field("readCount"), ValueCodecs.IntInvocation);
        sameThemeFunction = new CoflowFunction<global::Game.Config.ThemeValue, global::Game.Config.ThemeValue, bool>(record.Field("sameTheme"), ValueCodecs.CoflowInvocation<global::Game.Config.ThemeValue>(v1 => new global::Game.Config.ThemeValue(v1)), ValueCodecs.CoflowInvocation<global::Game.Config.ThemeValue>(v1 => new global::Game.Config.ThemeValue(v1)), ValueCodecs.BoolInvocation);
    }
}
}
