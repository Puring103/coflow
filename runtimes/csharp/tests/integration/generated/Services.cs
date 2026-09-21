#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public sealed class Services : RuntimeObject
{
    public string Id => Read("id", __Codecs.Id);
    public string environment => Read("environment", __Codecs.environment);
    public RuntimeFunction<int, int> adjustFunction => Read("adjust", __Codecs.adjust);
    public int adjust(int a0) => adjustFunction.Invoke(a0);
    public RuntimeFunction<int, Unit> notifyFunction => Read("notify", __Codecs.notify);
    public Unit notify(int a0) => notifyFunction.Invoke(a0);

    internal Services(Record record) : base(record)
    {
    }

    private static class __Codecs
    {
        internal static readonly Func<Projection, string> Id = ValueCodecs.String;
        internal static readonly Func<Projection, string> environment = ValueCodecs.String;
        internal static readonly Func<Projection, RuntimeFunction<int, int>> adjust = v0 => new RuntimeFunction<int, int>(v0, ValueCodecs.IntInvocation, ValueCodecs.IntInvocation);
        internal static readonly Func<Projection, RuntimeFunction<int, Unit>> notify = v0 => new RuntimeFunction<int, Unit>(v0, ValueCodecs.IntInvocation, ValueCodecs.UnitInvocation);
    }
}
}
