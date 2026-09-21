#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public class Character : CoflowObject
{
    public string Id { get; private set; } = default!;
    public string name { get; private set; } = default!;
    public global::Game.Config.Stats stats { get; private set; } = default!;
    public global::Game.Config.Character? friend { get; private set; } = default!;
    public CoflowDictionary<bool, string> labels { get; private set; } = default!;
    public CoflowDictionary<global::Game.Config.Mood, string> moods { get; private set; } = default!;
    public global::Game.Config.Mood? mood { get; private set; } = default!;
    public global::Game.Config.Stats? extra { get; private set; } = default!;
    public global::Game.Config.Profile? profile { get; private set; } = default!;
    public CoflowArray<global::Game.Config.Profile> profiles { get; private set; } = default!;
    public CoflowDictionary<string, global::Game.Config.Profile> profilesByName { get; private set; } = default!;
    public CoflowArray<string>? notes { get; private set; } = default!;
    public CoflowFunction<int>? callback { get; private set; } = default!;
    public CoflowTemplate text { get; private set; } = default!;
    public string Rendertext() => text.Render();
    public CoflowFunction<int, int> scoreFunction { get; private set; } = default!;
    public int score(int bonus) => scoreFunction.Invoke(bonus);
    public CoflowFunction<global::Game.Config.Character, global::Game.Config.Character> recordIdentityFunction { get; private set; } = default!;
    public global::Game.Config.Character recordIdentity(global::Game.Config.Character value) => recordIdentityFunction.Invoke(value);
    public CoflowFunction<CoflowArray<CoflowFunction<int>>, CoflowArray<CoflowFunction<int>>> callbacksFunction { get; private set; } = default!;
    public CoflowArray<CoflowFunction<int>> callbacks(CoflowArray<CoflowFunction<int>> values) => callbacksFunction.Invoke(values);
    public CoflowFunction<CoflowTemplate, CoflowTemplate> templateIdentityFunction { get; private set; } = default!;
    public CoflowTemplate templateIdentity(CoflowTemplate value) => templateIdentityFunction.Invoke(value);
    public CoflowFunction<global::Game.Config.Stats, global::Game.Config.Stats> roundtripFunction { get; private set; } = default!;
    public global::Game.Config.Stats roundtrip(global::Game.Config.Stats value) => roundtripFunction.Invoke(value);
    public CoflowFunction<global::Game.Config.Profile, global::Game.Config.Profile> profileIdentityFunction { get; private set; } = default!;
    public global::Game.Config.Profile profileIdentity(global::Game.Config.Profile value) => profileIdentityFunction.Invoke(value);
    public CoflowFunction<global::Game.Config.Stats, global::Game.Config.Stats> hostStatsFunction { get; private set; } = default!;
    public global::Game.Config.Stats hostStats(global::Game.Config.Stats value) => hostStatsFunction.Invoke(value);
    public CoflowFunction<global::Game.Config.Profile, global::Game.Config.Profile> hostProfileFunction { get; private set; } = default!;
    public global::Game.Config.Profile hostProfile(global::Game.Config.Profile value) => hostProfileFunction.Invoke(value);
    public CoflowFunction<global::Game.Config.Character, global::Game.Config.Character> hostCharacterFunction { get; private set; } = default!;
    public global::Game.Config.Character hostCharacter(global::Game.Config.Character value) => hostCharacterFunction.Invoke(value);
    public CoflowFunction<int, CoflowFunction<int>> closureFunction { get; private set; } = default!;
    public CoflowFunction<int> closure(int seed) => closureFunction.Invoke(seed);

    internal Character(Record record) : base(record)
    {
        Id = ValueCodecs.String(record.Field("id"));
        name = ValueCodecs.String(record.Field("name"));
        stats = new global::Game.Config.Stats(record.Field("stats"));
        friend = ValueCodecs.OptionalReference(record.Field("friend"), v0 => v0.Resolve<global::Game.Config.Character>());
        labels = new CoflowDictionary<bool, string>(record.Field("labels"), ValueCodecs.Bool, ValueCodecs.String);
        moods = new CoflowDictionary<global::Game.Config.Mood, string>(record.Field("moods"), v0 => (global::Game.Config.Mood)ValueCodecs.Enum(v0), ValueCodecs.String);
        mood = ValueCodecs.OptionalValue(record.Field("mood"), v0 => (global::Game.Config.Mood)ValueCodecs.Enum(v0));
        extra = ValueCodecs.OptionalValue(record.Field("extra"), v0 => new global::Game.Config.Stats(v0));
        profile = ValueCodecs.OptionalReference(record.Field("profile"), v0 => v0.Resolve<global::Game.Config.Profile>());
        profiles = new CoflowArray<global::Game.Config.Profile>(record.Field("profiles"), v0 => v0.Resolve<global::Game.Config.Profile>());
        profilesByName = new CoflowDictionary<string, global::Game.Config.Profile>(record.Field("profilesByName"), ValueCodecs.String, v0 => v0.Resolve<global::Game.Config.Profile>());
        notes = ValueCodecs.OptionalReference(record.Field("notes"), v0 => new CoflowArray<string>(v0, ValueCodecs.String));
        callback = ValueCodecs.OptionalReference(record.Field("callback"), v0 => new CoflowFunction<int>(v0, ValueCodecs.IntInvocation));
        text = new CoflowTemplate(record.Field("text"));
        scoreFunction = new CoflowFunction<int, int>(record.Field("score"), ValueCodecs.IntInvocation, ValueCodecs.IntInvocation);
        recordIdentityFunction = new CoflowFunction<global::Game.Config.Character, global::Game.Config.Character>(record.Field("recordIdentity"), ValueCodecs.CoflowInvocation<global::Game.Config.Character>(v1 => v1.Resolve<global::Game.Config.Character>()), ValueCodecs.CoflowInvocation<global::Game.Config.Character>(v1 => v1.Resolve<global::Game.Config.Character>()));
        callbacksFunction = new CoflowFunction<CoflowArray<CoflowFunction<int>>, CoflowArray<CoflowFunction<int>>>(record.Field("callbacks"), ValueCodecs.CoflowInvocation<CoflowArray<CoflowFunction<int>>>(v1 => new CoflowArray<CoflowFunction<int>>(v1, v2 => new CoflowFunction<int>(v2, ValueCodecs.IntInvocation))), ValueCodecs.CoflowInvocation<CoflowArray<CoflowFunction<int>>>(v1 => new CoflowArray<CoflowFunction<int>>(v1, v2 => new CoflowFunction<int>(v2, ValueCodecs.IntInvocation))));
        templateIdentityFunction = new CoflowFunction<CoflowTemplate, CoflowTemplate>(record.Field("templateIdentity"), ValueCodecs.TemplateInvocation, ValueCodecs.TemplateInvocation);
        roundtripFunction = new CoflowFunction<global::Game.Config.Stats, global::Game.Config.Stats>(record.Field("roundtrip"), ValueCodecs.CoflowInvocation<global::Game.Config.Stats>(v1 => new global::Game.Config.Stats(v1)), ValueCodecs.CoflowInvocation<global::Game.Config.Stats>(v1 => new global::Game.Config.Stats(v1)));
        profileIdentityFunction = new CoflowFunction<global::Game.Config.Profile, global::Game.Config.Profile>(record.Field("profileIdentity"), ValueCodecs.CoflowInvocation<global::Game.Config.Profile>(v1 => v1.Resolve<global::Game.Config.Profile>()), ValueCodecs.CoflowInvocation<global::Game.Config.Profile>(v1 => v1.Resolve<global::Game.Config.Profile>()));
        hostStatsFunction = new CoflowFunction<global::Game.Config.Stats, global::Game.Config.Stats>(record.Field("hostStats"), ValueCodecs.CoflowInvocation<global::Game.Config.Stats>(v1 => new global::Game.Config.Stats(v1)), ValueCodecs.CoflowInvocation<global::Game.Config.Stats>(v1 => new global::Game.Config.Stats(v1)));
        hostProfileFunction = new CoflowFunction<global::Game.Config.Profile, global::Game.Config.Profile>(record.Field("hostProfile"), ValueCodecs.CoflowInvocation<global::Game.Config.Profile>(v1 => v1.Resolve<global::Game.Config.Profile>()), ValueCodecs.CoflowInvocation<global::Game.Config.Profile>(v1 => v1.Resolve<global::Game.Config.Profile>()));
        hostCharacterFunction = new CoflowFunction<global::Game.Config.Character, global::Game.Config.Character>(record.Field("hostCharacter"), ValueCodecs.CoflowInvocation<global::Game.Config.Character>(v1 => v1.Resolve<global::Game.Config.Character>()), ValueCodecs.CoflowInvocation<global::Game.Config.Character>(v1 => v1.Resolve<global::Game.Config.Character>()));
        closureFunction = new CoflowFunction<int, CoflowFunction<int>>(record.Field("closure"), ValueCodecs.IntInvocation, ValueCodecs.CoflowInvocation<CoflowFunction<int>>(v1 => new CoflowFunction<int>(v1, ValueCodecs.IntInvocation)));
    }
}
}
