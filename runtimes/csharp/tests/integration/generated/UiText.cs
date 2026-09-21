#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public class UiText : RuntimeObject
{
    public string Id { get; private set; } = default!;
    public RuntimeDimension<string> welcome { get; private set; } = default!;
    public RuntimeDimension<RuntimeArray<int>> weights { get; private set; } = default!;
    public RuntimeDimension<global::Game.Config.ThemeValue> theme { get; private set; } = default!;
    public int count { get; private set; } = default!;
    public RuntimeFunction<int> readCountFunction { get; private set; } = default!;
    public int readCount() => readCountFunction.Invoke();
    public RuntimeFunction<global::Game.Config.ThemeValue, global::Game.Config.ThemeValue, bool> sameThemeFunction { get; private set; } = default!;
    public bool sameTheme(global::Game.Config.ThemeValue a0, global::Game.Config.ThemeValue a1) => sameThemeFunction.Invoke(a0, a1);

    internal UiText(Record record) : base(record)
    {
        Id = ValueCodecs.String(record.Field("id"));
        welcome = new RuntimeDimension<string>(record.Field("welcome"), ValueCodecs.String);
        weights = new RuntimeDimension<RuntimeArray<int>>(record.Field("weights"), v0 => new RuntimeArray<int>(v0, ValueCodecs.Int));
        theme = new RuntimeDimension<global::Game.Config.ThemeValue>(record.Field("theme"), v0 => new global::Game.Config.ThemeValue(v0));
        count = ValueCodecs.Int(record.Field("count"));
        readCountFunction = new RuntimeFunction<int>(record.Field("readCount"), ValueCodecs.IntInvocation);
        sameThemeFunction = new RuntimeFunction<global::Game.Config.ThemeValue, global::Game.Config.ThemeValue, bool>(record.Field("sameTheme"), ValueCodecs.RuntimeInvocation<global::Game.Config.ThemeValue>(v1 => new global::Game.Config.ThemeValue(v1)), ValueCodecs.RuntimeInvocation<global::Game.Config.ThemeValue>(v1 => new global::Game.Config.ThemeValue(v1)), ValueCodecs.BoolInvocation);
    }
}
}
