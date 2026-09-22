using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using Coflow;
using Game.Config;
using Xunit;

public sealed class NativeRuntimeTests : IDisposable
{
    private const string Source = "hero: Hero { name: \"Hero\", stats: Stats { health: 100, weights: [1.25, 2.5] }, labels: { true: \"yes\", false: \"no\" }, moods: { Mood::Happy: \"happy\" }, mood: Mood::Calm, extra: Stats { health: 12, bonus: 5 }, profile: Profile { title: \"Leader\", stats: Stats { health: 80 }, owner: &Character::hero }, profiles: [Profile { title: \"Array\", stats: Stats { health: 70 }, owner: &Character::hero }], profilesByName: { \"main\": Profile { title: \"Map\", stats: Stats { health: 60 }, owner: &Character::hero } } } RuntimeSettings: RuntimeSettings {}";
    private readonly Contract Contract = Generated.LoadContract(
        File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "coflow.contract")));
    private Runtime Build(IHostServices? host = null)
    {
        using var builder = new RuntimeBuilder(Contract).AddSource(Source);
        if (host != null) builder.BindHost(host);
        return builder.Build();
    }
    [System.Runtime.CompilerServices.MethodImpl(System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    private (WeakReference runtime, WeakReference host) CreateUnreachableHostCycle()
    {
        var host = new Host();
        using var runtime = Build(host);
        host.favorite = runtime.Table<Character>().Get("hero");
        return (new WeakReference(runtime), new WeakReference(host));
    }
    [System.Runtime.CompilerServices.MethodImpl(System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    private Runtime CreateRuntimeOwningHost() => Build(new Host());
    [System.Runtime.CompilerServices.MethodImpl(System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    private static void ReturnTemporaryClosure(HostCall call, Character hero) => call.Return(
        ValueCodecs.CoflowInvocation<CoflowFunction<int>>(p => new CoflowFunction<int>(p, ValueCodecs.IntInvocation)), hero.closure(3));
    [Fact]
    public void HostReturnRetainsClosureAcrossGcAndReentry()
    {
        using var runtime = Build();
        var call = new HostCall(new byte[4], 0);
        ReturnTemporaryClosure(call, runtime.Table<Character>().Get("hero"));
        GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect();
        runtime.RunChecks();
        var codec = ValueCodecs.CoflowInvocation<CoflowFunction<int>>(p => new CoflowFunction<int>(p, ValueCodecs.IntInvocation));
        Assert.Equal(103, codec.ReadResult(runtime, call.Result).Invoke());
        GC.KeepAlive(call);
    }
    private sealed class SweepingHost : HostBinding
    {
        internal Runtime Runtime = null!;
        internal Character Hero = null!;
        internal SweepingHost() : base("HostServices") { }
        private static readonly InvocationCodec<Profile> Codec = ValueCodecs.CoflowInvocation<Profile>(p => p.Resolve<Profile>());
        public override object? Read(string field) => throw new InvalidOperationException();
        [System.Runtime.CompilerServices.MethodImpl(System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
        private void ReturnTemporary(HostCall call) => call.Return(Codec,
            Hero.profileIdentity(new Profile("returned", new Stats(73, new CoflowArray<float>(Array.Empty<float>()), null), Hero)));
        public override void Call(string field, HostCall call)
        {
            Assert.Equal("echoProfile", field);
            call.Argument(Codec);
            ReturnTemporary(call);
            GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect();
            Runtime.RunChecks();
        }
    }
    [Fact]
    public void NativeHostCallbackRetainsDynamicReturnUntilAdoption()
    {
        var host = new SweepingHost();
        using var builder = new RuntimeBuilder(Contract).AddSource(Source).BindHost(host);
        using var runtime = builder.Build();
        host.Runtime = runtime;
        host.Hero = runtime.Table<Character>().Get("hero");
        var returned = host.Hero.hostProfile(new Profile("input", host.Hero.stats, host.Hero));
        Assert.Equal("returned", returned.title);
        Assert.Equal(73, returned.stats.health);
    }
    private sealed class SweepingArgument : ICoflowValue
    {
        private readonly Character hero;
        private readonly Runtime runtime;
        internal SweepingArgument(Character hero, Runtime runtime) { this.hero = hero; this.runtime = runtime; }
        [System.Runtime.CompilerServices.MethodImpl(System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
        private void EncodeTemporary(ArgumentWriter writer) =>
            ((ICoflowValue)new CoflowArray<CoflowFunction<int>>(new[] { hero.closure(3) })).Encode(writer);
        public void Encode(ArgumentWriter writer)
        {
            EncodeTemporary(writer);
            GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect();
            runtime.RunChecks();
        }
    }
    [Fact]
    public void EncodedArgumentsRetainClosuresAcrossGcAndReentry()
    {
        using var runtime = Build();
        var hero = runtime.Table<Character>().Get("hero");
        var function = new CoflowFunction<SweepingArgument, CoflowArray<CoflowFunction<int>>>(
            Projection.From(hero.callbacksFunction),
            ValueCodecs.CoflowInvocation<SweepingArgument>(_ => throw new InvalidOperationException()),
            ValueCodecs.CoflowInvocation<CoflowArray<CoflowFunction<int>>>(p => new CoflowArray<CoflowFunction<int>>(p,
                v => new CoflowFunction<int>(v, ValueCodecs.IntInvocation))));
        Assert.Equal(103, function.Invoke(new SweepingArgument(hero, runtime))[0].Invoke());
    }
    [System.Runtime.CompilerServices.MethodImpl(System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    private static WeakReference DiscardDynamicResult(Character hero) => new WeakReference(
        hero.profileIdentity(new Profile("transient", new Stats(1, new CoflowArray<float>(Array.Empty<float>()), null), null)));
    [Fact]
    public void DynamicObjectsAreCollectedWhileTheirRuntimeRemainsAlive()
    {
        using var runtime = Build();
        var hero = runtime.Table<Character>().Get("hero");
        var results = Enumerable.Range(0, 100).Select(_ => DiscardDynamicResult(hero)).ToArray();
        GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect();
        Assert.All(results, value => Assert.False(value.IsAlive));
        GC.KeepAlive(runtime);
    }
    [Fact]
    public void HostCyclesAreCollectibleAndLiveRuntimeOwnsItsHost()
    {
        var cycle = CreateUnreachableHostCycle();
        for (int i = 0; i < 4; ++i) { GC.Collect(); GC.WaitForPendingFinalizers(); }
        Assert.False(cycle.runtime.IsAlive);
        Assert.False(cycle.host.IsAlive);
        using var runtime = CreateRuntimeOwningHost();
        GC.Collect(); GC.WaitForPendingFinalizers();
        Assert.Equal("Unity", runtime.Get<HostServices>().environment);
        runtime.Get<HostServices>().log("still alive");
    }
    [Fact]
    public void GeneratedBindingsRejectAnotherContractIdentity()
    {
        var bytes = File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "coflow.contract"));
        Assert.Throws<CoflowException>(() => new Contract(bytes, new byte[32]));
    }
    [Fact]
    public void TypedQueriesAndNullableValuesReadNativeData()
    {
        using var runtime = Build();
        var characters = runtime.Table<Character>();
        long requests = Native.RequestCount;
        var hero = characters.Get("hero");
        Assert.Equal(2, Native.RequestCount - requests);
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
        Assert.Equal("Leader", hero.profile!.title);
        Assert.Equal(80, hero.profile.stats.health);
        Assert.Same(hero, hero.profile.owner);
        Assert.Equal("Array", hero.profiles[0].title);
        Assert.Same(hero, hero.profiles[0].owner);
        Assert.Equal("Map", hero.profilesByName["main"].title);
        Assert.Same(hero, hero.profilesByName["main"].owner);
        Assert.Null(hero.friend);
        Assert.Null(hero.notes);
        Assert.Null(hero.callback);
        Assert.True(runtime.Get<RuntimeSettings>().enabled);
        Assert.False(characters.TryGet("missing", out _));
        Assert.Throws<KeyNotFoundException>(() => characters.Get("missing"));
        Assert.Throws<CoflowException>(() => runtime.Table<Stats>());
        Assert.Throws<CoflowException>(() => runtime.Table<RuntimeSettings>());
        Assert.Throws<CoflowException>(() => runtime.Get<Character>());
    }
    [Fact]
    public void RecordIdentityIsSharedOnlyInsideOneRuntime()
    {
        using var first = Build();
        using var second = Build();
        var hero = first.Table<Character>().Get("hero");
        var derived = first.Table<Hero>().Get("hero");
        Assert.True(hero == derived);
        Assert.Equal(hero.GetHashCode(), derived.GetHashCode());
        Assert.NotEqual(hero, second.Table<Character>().Get("hero"));
    }
    [Fact]
    public void RuntimeOwnsAllValuesAndSurvivesWhileValuesAreReferenced()
    {
        var runtime = Build();
        var hero = runtime.Table<Character>().Get("hero");
        var weights = hero.stats.weights;
        runtime.Dispose();
        Assert.Equal("Hero", hero.name);
        Assert.Equal(2, weights.Count);
        Assert.Throws<ObjectDisposedException>(() => hero.score(1));
        runtime.Dispose();
        var kept = MakeRecord();
        GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect();
        Assert.Equal("Hero", kept.name);
    }
    private Character MakeRecord()
    {
        using var runtime = Build();
        return runtime.Table<Character>().Get("hero");
    }
    public void Dispose() => Contract.Dispose();
    [Fact]
    public void BuilderAppendsSourcesAndIsConsumedOnlyOnSuccess()
    {
        using var builder = new RuntimeBuilder(Contract);
        var failure = Assert.Throws<BuildException>(() => builder.Build());
        Assert.NotEmpty(failure.Diagnostics);
        builder.AddSource(Source, "same-name");
        builder.AddSource("other: Character { name: \"Other\", stats: Stats { health: 10 } }", "same-name");
        using var runtime = builder.Build();
        Assert.Equal(2, runtime.Table<Character>().Count);
        Assert.Throws<CoflowException>(() => builder.Build());
        Assert.Throws<CoflowException>(() => builder.AddSource(""));
        Assert.Throws<CoflowException>(() => builder.BindHost(new Host()));
        using var invalid = new RuntimeBuilder(Contract).AddSource("a: Missing {}");
        var diagnostic = Assert.Throws<BuildException>(() => invalid.Build()).Diagnostics[0];
        Assert.Equal("source-1", diagnostic.SourceName);
        Assert.NotEmpty(diagnostic.Code);
        Assert.NotNull(diagnostic.StartOffset);
    }
    [Fact]
    public void GeneratedHostBindingsAreTypedLazyAndRuntimeScoped()
    {
        using var missing = Build();
        Assert.Throws<CoflowException>(() => missing.Get<HostServices>().environment);
        var host = new Host();
        using var bound = Build(host);
        host.favorite = bound.Table<Character>().Get("hero");
        var service = bound.Get<HostServices>();
        Assert.Equal("Unity", service.environment);
        Assert.Equal(Mood.Happy, service.mood);
        Assert.Equal(host.favorite, service.favorite);
        host.favorite = missing.Table<Character>().Get("hero");
        Assert.Throws<CoflowException>(() => service.favorite);
        using var builder = new RuntimeBuilder(Contract).BindHost(host);
        Assert.Throws<CoflowException>(() => builder.BindHost(host));
    }
    [Fact]
    public void FunctionsTemplatesAndHostCallsExecute()
    {
        var host = new Host();
        using var runtime = Build(host);
        var hero = runtime.Table<Character>().Get("hero");
        Assert.Contains("self.name", hero.text.Source);
        Assert.Equal("Hero", hero.text.Render());
        Assert.Contains("bonus", hero.scoreFunction.Source);
        Assert.Equal(123, hero.score(bonus: 23));
        var stats = hero.hostStats(new Stats(41, new CoflowArray<float>(Array.Empty<float>()), null));
        Assert.Equal(41, stats.health);
        var profile = hero.hostProfile(new Profile("Host", stats, hero));
        Assert.Equal("Host", profile.title);
        Assert.Equal(41, profile.stats.health);
        Assert.Same(hero, profile.owner);
        Assert.Same(hero, hero.hostCharacter(hero));
        runtime.Get<HostServices>().log("ready");
        Assert.Equal("ready", host.lastMessage);
    }
    [Fact]
    public void ChecksReturnStructuredResultsAndSupportRecordSelection()
    {
        using var healthy = Build();
        var success = healthy.RunChecks();
        Assert.True(success.Success);
        Assert.Equal(1UL, success.Statistics.ExecutedTasks);

        using var builder = new RuntimeBuilder(Contract).AddSource(Source.Replace("health: 100", "health: -1"));
        using var invalid = builder.Build();
        var hero = invalid.Table<Character>().Get("hero");
        var failure = invalid.RunChecks(new CheckOptions(records: new CoflowObject[] { hero }, includeGlobal: false));
        Assert.False(failure.Success);
        var diagnostic = Assert.Single(failure.Diagnostics);
        Assert.Equal("CHECK-001", diagnostic.Code);
        Assert.Equal("health must be positive", diagnostic.Message);
        Assert.Contains("Healthy", diagnostic.CheckNames);
    }
    private sealed class Host : IHostServices
    {
        public string? lastMessage { get; private set; }
        public string environment => "Unity";
        public Character? favorite { get; set; }
        public Mood mood => Mood.Happy;
        public Unit log(string message) { lastMessage = message; return default; }
        public Stats echoStats(Stats value) => value;
        public Profile echoProfile(Profile value) => value;
        public Character echoCharacter(Character value) => value;
    }
}
