using System;
using System.Collections.Generic;
using System.Linq;
using Coflow;
using Game.Config;
using Xunit;

public sealed class NativeRuntimeTests
{
    private const string Source = "hero: Hero { name: \"Hero\", stats: Stats { health: 100, weights: [1.25, 2.5] }, labels: { true: \"yes\", false: \"no\" }, moods: { Mood::Happy: \"happy\" }, mood: Mood::Calm, extra: Stats { health: 12, bonus: 5 } } RuntimeSettings: RuntimeSettings {}";
    private static Runtime Build(IHostServices? host = null)
    {
        using var builder = new RuntimeBuilder(Generated.Contract).AddSource(Source);
        if (host != null) builder.BindHost(host);
        return builder.Build();
    }
    [Fact]
    public void TypedQueriesAndNullableValuesReadNativeData()
    {
        using var runtime = Build();
        var characters = runtime.Table<Character>();
        var hero = characters["hero"];
        Assert.IsType<Hero>(hero);
        Assert.Single(characters);
        Assert.Equal(hero, characters.Single());
        Assert.Equal("hero", hero.Id);
        Assert.Equal(100, hero.stats.health);
        Assert.Null(hero.stats.bonus);
        Assert.Equal(1.25f, hero.stats.weights[0]);
        Assert.Equal("yes", hero.labels[true]);
        Assert.Equal("happy", hero.moods[Mood.Happy]);
        Assert.False(hero.moods.TryGetValue(Mood.Calm, out _));
        Assert.Equal(Mood.Calm, hero.mood);
        Assert.Equal(5, hero.extra!.Value.bonus);
        Assert.Null(hero.friend);
        Assert.Null(hero.notes);
        Assert.Null(hero.callback);
        Assert.True(runtime.Singleton<RuntimeSettings>().enabled);
        Assert.False(characters.TryGet("missing", out _));
        Assert.Throws<KeyNotFoundException>(() => characters["missing"]);
        Assert.Throws<CoflowException>(() => runtime.Table<Stats>());
        Assert.Throws<CoflowException>(() => runtime.Table<RuntimeSettings>());
        Assert.Throws<CoflowException>(() => runtime.Singleton<Character>());
    }
    [Fact]
    public void RecordIdentityIsSharedOnlyInsideOneRuntime()
    {
        using var first = Build();
        using var second = Build();
        var hero = first.Table<Character>()["hero"];
        var derived = first.Table<Hero>()["hero"];
        Assert.True(hero == derived);
        Assert.Equal(hero.GetHashCode(), derived.GetHashCode());
        Assert.NotEqual(hero, second.Table<Character>()["hero"]);
    }
    [Fact]
    public void RuntimeOwnsAllValuesAndSurvivesWhileValuesAreReferenced()
    {
        var runtime = Build();
        var hero = runtime.Table<Character>()["hero"];
        var weights = hero.stats.weights;
        runtime.Dispose();
        Assert.Throws<ObjectDisposedException>(() => hero.name);
        Assert.Throws<ObjectDisposedException>(() => weights.Count);
        runtime.Dispose();
        var kept = MakeRecord();
        GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect();
        Assert.Equal("Hero", kept.name);
    }
    private static Character MakeRecord() => Build().Table<Character>()["hero"];
    [Fact]
    public void BuilderAppendsSourcesAndIsConsumedOnlyOnSuccess()
    {
        using var builder = new RuntimeBuilder(Generated.Contract);
        var failure = Assert.Throws<BuildException>(() => builder.Build());
        Assert.NotEmpty(failure.Diagnostics);
        builder.AddSource(Source, "same-name");
        builder.AddSource("other: Character { name: \"Other\", stats: Stats { health: 10 } }", "same-name");
        using var runtime = builder.Build();
        Assert.Equal(2, runtime.Table<Character>().Count);
        Assert.Throws<CoflowException>(() => builder.Build());
        Assert.Throws<CoflowException>(() => builder.AddSource(""));
        Assert.Throws<CoflowException>(() => builder.BindHost(new Host()));
        using var invalid = new RuntimeBuilder(Generated.Contract).AddSource("a: Missing {}");
        var diagnostic = Assert.Throws<BuildException>(() => invalid.Build()).Diagnostics[0];
        Assert.Equal("source-1", diagnostic.SourceName);
        Assert.NotEmpty(diagnostic.Code);
        Assert.NotNull(diagnostic.StartOffset);
    }
    [Fact]
    public void GeneratedHostBindingsAreTypedLazyAndRuntimeScoped()
    {
        using var missing = Build();
        Assert.Throws<CoflowException>(() => missing.Singleton<HostServices>().environment);
        var host = new Host();
        using var bound = Build(host);
        host.favorite = bound.Table<Character>()["hero"];
        var service = bound.Singleton<HostServices>();
        Assert.Equal("Unity", service.environment);
        Assert.Equal(Mood.Happy, service.mood);
        Assert.Equal(host.favorite, service.favorite);
        host.favorite = missing.Table<Character>()["hero"];
        Assert.Throws<CoflowException>(() => service.favorite);
        using var builder = new RuntimeBuilder(Generated.Contract).BindHost(host);
        Assert.Throws<CoflowException>(() => builder.BindHost(host));
    }
    [Fact]
    public void ProgramSourcesRemainAvailableWithoutExecution()
    {
        using var runtime = Build();
        var hero = runtime.Table<Character>()["hero"];
        Assert.Contains("self.name", hero.Get_text_Template().ProgramSource);
        Assert.Throws<CoflowException>(() => hero.text);
        Assert.Contains("bonus", hero.score.Source);
        Assert.Throws<CoflowException>(() => hero.score.Call());
    }
    private sealed class Host : IHostServices
    {
        public string environment => "Unity";
        public Character? favorite { get; set; }
        public Mood mood => Mood.Happy;
    }
}
