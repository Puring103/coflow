#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public class Character : RuntimeObject
{
    public string Id { get; private set; } = default!;
    public string name { get; private set; } = default!;
    public global::Game.Config.Stats stats { get; private set; } = default!;
    public global::Game.Config.Character? friend { get; private set; } = default!;
    public RuntimeDictionary<bool, string> labels { get; private set; } = default!;
    public RuntimeDictionary<global::Game.Config.Mood, string> moods { get; private set; } = default!;
    public global::Game.Config.Mood? mood { get; private set; } = default!;
    public global::Game.Config.Stats? extra { get; private set; } = default!;
    public global::Game.Config.Profile? profile { get; private set; } = default!;
    public RuntimeArray<global::Game.Config.Profile> profiles { get; private set; } = default!;
    public RuntimeDictionary<string, global::Game.Config.Profile> profilesByName { get; private set; } = default!;
    public RuntimeArray<string>? notes { get; private set; } = default!;
    public RuntimeFunction<int>? callback { get; private set; } = default!;
    public RuntimeTemplate text { get; private set; } = default!;
    public string Rendertext() => text.Render();
    public RuntimeFunction<int, int> scoreFunction { get; private set; } = default!;
    public int score(int bonus) => scoreFunction.Invoke(bonus);
    public RuntimeFunction<global::Game.Config.Character, global::Game.Config.Character> recordIdentityFunction { get; private set; } = default!;
    public global::Game.Config.Character recordIdentity(global::Game.Config.Character value) => recordIdentityFunction.Invoke(value);
    public RuntimeFunction<RuntimeArray<RuntimeFunction<int>>, RuntimeArray<RuntimeFunction<int>>> callbacksFunction { get; private set; } = default!;
    public RuntimeArray<RuntimeFunction<int>> callbacks(RuntimeArray<RuntimeFunction<int>> values) => callbacksFunction.Invoke(values);
    public RuntimeFunction<RuntimeTemplate, RuntimeTemplate> templateIdentityFunction { get; private set; } = default!;
    public RuntimeTemplate templateIdentity(RuntimeTemplate value) => templateIdentityFunction.Invoke(value);
    public RuntimeFunction<global::Game.Config.Stats, global::Game.Config.Stats> roundtripFunction { get; private set; } = default!;
    public global::Game.Config.Stats roundtrip(global::Game.Config.Stats value) => roundtripFunction.Invoke(value);
    public RuntimeFunction<global::Game.Config.Profile, global::Game.Config.Profile> profileIdentityFunction { get; private set; } = default!;
    public global::Game.Config.Profile profileIdentity(global::Game.Config.Profile value) => profileIdentityFunction.Invoke(value);
    public RuntimeFunction<global::Game.Config.Stats, global::Game.Config.Stats> hostStatsFunction { get; private set; } = default!;
    public global::Game.Config.Stats hostStats(global::Game.Config.Stats value) => hostStatsFunction.Invoke(value);
    public RuntimeFunction<global::Game.Config.Profile, global::Game.Config.Profile> hostProfileFunction { get; private set; } = default!;
    public global::Game.Config.Profile hostProfile(global::Game.Config.Profile value) => hostProfileFunction.Invoke(value);
    public RuntimeFunction<global::Game.Config.Character, global::Game.Config.Character> hostCharacterFunction { get; private set; } = default!;
    public global::Game.Config.Character hostCharacter(global::Game.Config.Character value) => hostCharacterFunction.Invoke(value);
    public RuntimeFunction<int, RuntimeFunction<int>> closureFunction { get; private set; } = default!;
    public RuntimeFunction<int> closure(int seed) => closureFunction.Invoke(seed);

    internal Character(Record record) : base(record)
    {
        Id = ValueCodecs.String(record.Field("id"));
        name = ValueCodecs.String(record.Field("name"));
        stats = new global::Game.Config.Stats(record.Field("stats"));
        friend = ValueCodecs.OptionalReference(record.Field("friend"), v0 => v0.Resolve<global::Game.Config.Character>());
        labels = new RuntimeDictionary<bool, string>(record.Field("labels"), ValueCodecs.Bool, ValueCodecs.String);
        moods = new RuntimeDictionary<global::Game.Config.Mood, string>(record.Field("moods"), v0 => (global::Game.Config.Mood)ValueCodecs.Enum(v0), ValueCodecs.String);
        mood = ValueCodecs.OptionalValue(record.Field("mood"), v0 => (global::Game.Config.Mood)ValueCodecs.Enum(v0));
        extra = ValueCodecs.OptionalValue(record.Field("extra"), v0 => new global::Game.Config.Stats(v0));
        profile = ValueCodecs.OptionalReference(record.Field("profile"), v0 => v0.Resolve<global::Game.Config.Profile>());
        profiles = new RuntimeArray<global::Game.Config.Profile>(record.Field("profiles"), v0 => v0.Resolve<global::Game.Config.Profile>());
        profilesByName = new RuntimeDictionary<string, global::Game.Config.Profile>(record.Field("profilesByName"), ValueCodecs.String, v0 => v0.Resolve<global::Game.Config.Profile>());
        notes = ValueCodecs.OptionalReference(record.Field("notes"), v0 => new RuntimeArray<string>(v0, ValueCodecs.String));
        callback = ValueCodecs.OptionalReference(record.Field("callback"), v0 => new RuntimeFunction<int>(v0, ValueCodecs.IntInvocation));
        text = new RuntimeTemplate(record.Field("text"));
        scoreFunction = new RuntimeFunction<int, int>(record.Field("score"), ValueCodecs.IntInvocation, ValueCodecs.IntInvocation);
        recordIdentityFunction = new RuntimeFunction<global::Game.Config.Character, global::Game.Config.Character>(record.Field("recordIdentity"), ValueCodecs.RuntimeInvocation<global::Game.Config.Character>(v1 => v1.Resolve<global::Game.Config.Character>()), ValueCodecs.RuntimeInvocation<global::Game.Config.Character>(v1 => v1.Resolve<global::Game.Config.Character>()));
        callbacksFunction = new RuntimeFunction<RuntimeArray<RuntimeFunction<int>>, RuntimeArray<RuntimeFunction<int>>>(record.Field("callbacks"), ValueCodecs.RuntimeInvocation<RuntimeArray<RuntimeFunction<int>>>(v1 => new RuntimeArray<RuntimeFunction<int>>(v1, v2 => new RuntimeFunction<int>(v2, ValueCodecs.IntInvocation))), ValueCodecs.RuntimeInvocation<RuntimeArray<RuntimeFunction<int>>>(v1 => new RuntimeArray<RuntimeFunction<int>>(v1, v2 => new RuntimeFunction<int>(v2, ValueCodecs.IntInvocation))));
        templateIdentityFunction = new RuntimeFunction<RuntimeTemplate, RuntimeTemplate>(record.Field("templateIdentity"), ValueCodecs.TemplateInvocation, ValueCodecs.TemplateInvocation);
        roundtripFunction = new RuntimeFunction<global::Game.Config.Stats, global::Game.Config.Stats>(record.Field("roundtrip"), ValueCodecs.RuntimeInvocation<global::Game.Config.Stats>(v1 => new global::Game.Config.Stats(v1)), ValueCodecs.RuntimeInvocation<global::Game.Config.Stats>(v1 => new global::Game.Config.Stats(v1)));
        profileIdentityFunction = new RuntimeFunction<global::Game.Config.Profile, global::Game.Config.Profile>(record.Field("profileIdentity"), ValueCodecs.RuntimeInvocation<global::Game.Config.Profile>(v1 => v1.Resolve<global::Game.Config.Profile>()), ValueCodecs.RuntimeInvocation<global::Game.Config.Profile>(v1 => v1.Resolve<global::Game.Config.Profile>()));
        hostStatsFunction = new RuntimeFunction<global::Game.Config.Stats, global::Game.Config.Stats>(record.Field("hostStats"), ValueCodecs.RuntimeInvocation<global::Game.Config.Stats>(v1 => new global::Game.Config.Stats(v1)), ValueCodecs.RuntimeInvocation<global::Game.Config.Stats>(v1 => new global::Game.Config.Stats(v1)));
        hostProfileFunction = new RuntimeFunction<global::Game.Config.Profile, global::Game.Config.Profile>(record.Field("hostProfile"), ValueCodecs.RuntimeInvocation<global::Game.Config.Profile>(v1 => v1.Resolve<global::Game.Config.Profile>()), ValueCodecs.RuntimeInvocation<global::Game.Config.Profile>(v1 => v1.Resolve<global::Game.Config.Profile>()));
        hostCharacterFunction = new RuntimeFunction<global::Game.Config.Character, global::Game.Config.Character>(record.Field("hostCharacter"), ValueCodecs.RuntimeInvocation<global::Game.Config.Character>(v1 => v1.Resolve<global::Game.Config.Character>()), ValueCodecs.RuntimeInvocation<global::Game.Config.Character>(v1 => v1.Resolve<global::Game.Config.Character>()));
        closureFunction = new RuntimeFunction<int, RuntimeFunction<int>>(record.Field("closure"), ValueCodecs.IntInvocation, ValueCodecs.RuntimeInvocation<RuntimeFunction<int>>(v1 => new RuntimeFunction<int>(v1, ValueCodecs.IntInvocation)));
    }
}
}
