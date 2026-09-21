#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public sealed class HostServices : RuntimeObject
{
    public string Id => Read("id", __Codecs.Id);
    public string environment => Read("environment", __Codecs.environment);
    public global::Game.Config.Character? favorite => Read("favorite", __Codecs.favorite);
    public global::Game.Config.Mood mood => Read("mood", __Codecs.mood);
    public RuntimeFunction<string, Unit> logFunction => Read("log", __Codecs.log);
    public Unit log(string message) => logFunction.Invoke(message);
    public RuntimeFunction<global::Game.Config.Stats, global::Game.Config.Stats> echoStatsFunction => Read("echoStats", __Codecs.echoStats);
    public global::Game.Config.Stats echoStats(global::Game.Config.Stats value) => echoStatsFunction.Invoke(value);
    public RuntimeFunction<global::Game.Config.Profile, global::Game.Config.Profile> echoProfileFunction => Read("echoProfile", __Codecs.echoProfile);
    public global::Game.Config.Profile echoProfile(global::Game.Config.Profile value) => echoProfileFunction.Invoke(value);
    public RuntimeFunction<global::Game.Config.Character, global::Game.Config.Character> echoCharacterFunction => Read("echoCharacter", __Codecs.echoCharacter);
    public global::Game.Config.Character echoCharacter(global::Game.Config.Character value) => echoCharacterFunction.Invoke(value);

    internal HostServices(Record record) : base(record)
    {
    }

    private static class __Codecs
    {
        internal static readonly Func<Projection, string> Id = ValueCodecs.String;
        internal static readonly Func<Projection, string> environment = ValueCodecs.String;
        internal static readonly Func<Projection, global::Game.Config.Character?> favorite = v0 => ValueCodecs.OptionalReference(v0, v1 => v1.Resolve<global::Game.Config.Character>());
        internal static readonly Func<Projection, global::Game.Config.Mood> mood = v0 => (global::Game.Config.Mood)ValueCodecs.Enum(v0);
        internal static readonly Func<Projection, RuntimeFunction<string, Unit>> log = v0 => new RuntimeFunction<string, Unit>(v0, ValueCodecs.StringInvocation, ValueCodecs.UnitInvocation);
        internal static readonly Func<Projection, RuntimeFunction<global::Game.Config.Stats, global::Game.Config.Stats>> echoStats = v0 => new RuntimeFunction<global::Game.Config.Stats, global::Game.Config.Stats>(v0, ValueCodecs.RuntimeInvocation<global::Game.Config.Stats>(v2 => new global::Game.Config.Stats(v2)), ValueCodecs.RuntimeInvocation<global::Game.Config.Stats>(v2 => new global::Game.Config.Stats(v2)));
        internal static readonly Func<Projection, RuntimeFunction<global::Game.Config.Profile, global::Game.Config.Profile>> echoProfile = v0 => new RuntimeFunction<global::Game.Config.Profile, global::Game.Config.Profile>(v0, ValueCodecs.RuntimeInvocation<global::Game.Config.Profile>(v2 => v2.Resolve<global::Game.Config.Profile>()), ValueCodecs.RuntimeInvocation<global::Game.Config.Profile>(v2 => v2.Resolve<global::Game.Config.Profile>()));
        internal static readonly Func<Projection, RuntimeFunction<global::Game.Config.Character, global::Game.Config.Character>> echoCharacter = v0 => new RuntimeFunction<global::Game.Config.Character, global::Game.Config.Character>(v0, ValueCodecs.RuntimeInvocation<global::Game.Config.Character>(v2 => v2.Resolve<global::Game.Config.Character>()), ValueCodecs.RuntimeInvocation<global::Game.Config.Character>(v2 => v2.Resolve<global::Game.Config.Character>()));
    }
}
}
