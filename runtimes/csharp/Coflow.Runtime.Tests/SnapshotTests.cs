using System;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using Coflow;
using Game.Config;
using Xunit;

public sealed class SnapshotTests
{
    private sealed class CountingHost : IHostServices
    {
        internal int Reads;
        internal Character? Hero;
        public string environment { get { Reads++; return "test"; } }
        public Character? favorite { get { Reads++; return Hero; } }
        public Mood mood { get { Reads++; return Mood.Happy; } }
        public Unit log(string message) => new Unit();
    }
    [Fact]
    public void EachHostPropertyReadsExactlyOnceIncludingOptionalRecordsAndEnums()
    {
        using var contract = Generated.LoadContract(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "coflow.contract")));
        var host = new CountingHost();
        using var builder = new RuntimeBuilder(contract).BindHost(host).AddSource("hero: Hero { name: \"Hero\", stats: Stats { health: 10 } } RuntimeSettings: RuntimeSettings {}");
        using var runtime = builder.Build();
        var service = runtime.Singleton<HostServices>(); Assert.Equal(0, host.Reads);
        Assert.Equal("test", service.environment); Assert.Equal(1, host.Reads);
        Assert.Equal(Mood.Happy, service.mood); Assert.Equal(2, host.Reads);
        Assert.Null(service.favorite); Assert.Equal(3, host.Reads);
        host.Hero = runtime.Table<Character>()["hero"];
        Assert.Same(host.Hero, service.favorite); Assert.Equal(4, host.Reads);
        Assert.Throws<CoflowException>(() => host.Hero.roundtrip(default));
    }
    [Fact]
    public void ExplicitNoneDimensionRemainsPresentAndDoesNotFallBack()
    {
        using var contract = Generated.LoadContract(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "coflow.contract")));
        using var builder = new RuntimeBuilder(contract).AddSource("a: LocalizedText { value: dimension { default: \"base\", ja: \"Japanese\" }, optional: dimension { default: \"fallback\", zh: None } } RuntimeSettings: RuntimeSettings {}");
        var runtime = builder.Build();
        var text = runtime.Table<LocalizedText>()["a"];
        runtime.Dispose();
        long before = Native.RequestCount;
        Assert.Equal("fallback", text.optional.Default());
        Assert.Null(text.optional.For("zh"));
        Assert.True(text.optional.TryGetVariant("zh", out var explicitNone));
        Assert.Null(explicitNone);
        Assert.False(text.optional.TryGetVariant("ja", out _));
        Assert.Equal("fallback", text.optional.For("ja"));
        Assert.Equal("fallback", text.optional.For("unknown"));
        Assert.Null(text.optional.Variants()["zh"]);
        Assert.Equal("fallback", text.optional.Variants()["ja"]);
        Assert.Equal(before, Native.RequestCount);
    }
    [Fact]
    public async Task OrdinaryReadsRemainManagedAcrossThreadsAndDisposal()
    {
        using var contract = Generated.LoadContract(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "coflow.contract")));
        using var builder = new RuntimeBuilder(contract).AddSource("hero: Hero { name: \"Hero\", stats: Stats { health: 100, weights: [1.25, 2.5] }, friend: &hero, labels: { true: \"yes\" } } RuntimeSettings: RuntimeSettings {}");
        var runtime = builder.Build();
        var table = runtime.Table<Character>(); var hero = table["hero"];
        runtime.Dispose();
        await Task.Run(() => {
            long before = Native.RequestCount;
            for (int i = 0; i < 1000; ++i) {
                Assert.Equal("Hero", table["hero"].name);
                Assert.Equal(100, hero.stats.health);
                Assert.Equal(2.5f, hero.stats.weights[1]);
                Assert.Same(hero.stats.weights, hero.stats.weights);
                Assert.Same(hero.labels, hero.labels);
                Assert.Equal("yes", hero.labels[true]);
                Assert.Single(table);
                Assert.Same(hero, table.Single());
                Assert.Same(hero, hero.friend);
            }
            Assert.Equal(before, Native.RequestCount);
        });
        Assert.Throws<ObjectDisposedException>(() => hero.score(1));
    }
    [Fact]
    public void DetachedDataCopiesInputsAndRoundTripsThroughTypedImport()
    {
        using var contract = Generated.LoadContract(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "coflow.contract")));
        using var builder = new RuntimeBuilder(contract).AddSource("hero: Hero { name: \"Hero\", stats: Stats { health: 100 } } RuntimeSettings: RuntimeSettings {}");
        using var runtime = builder.Build(); var hero = runtime.Table<Character>()["hero"];
        var input = new float[] { 1, 2 };
        var stats = new Stats(17, new RuntimeArray<float>(input), 3);
        input[0] = 99;
        Assert.Equal(1, stats.weights[0]);
        var result = hero.roundtrip(stats);
        Assert.Equal(17, result.health); Assert.Equal(3, result.bonus); Assert.Equal(1, result.weights[0]);
        Native.Call(NativeOperation.Collect, runtime.Handle);
        Assert.Throws<CoflowException>(() => Native.Call(NativeOperation.InspectValue, runtime.Handle, value: result.RuntimeValue.Id));
        Assert.Equal(17, hero.roundtrip(result).health);
        Assert.Equal("literal", hero.templateIdentity(new RuntimeTemplate("literal")).Render());
        Assert.Equal("Hero", hero.templateIdentity(hero.text).Render());
        var closure = hero.closure(7);
        var callbacks = hero.callbacks(new RuntimeArray<RuntimeFunction<int>>(new[] { closure }));
        Assert.Equal(107, callbacks[0].Invoke());
        Assert.Equal(107, hero.callbacks(callbacks)[0].Invoke());
        GC.Collect(); GC.WaitForPendingFinalizers();
        Native.Call(NativeOperation.Collect, runtime.Handle);
        Assert.Equal(107, closure.Invoke());
        runtime.Dispose();
        Assert.Equal(17, result.health);
        Assert.Throws<ObjectDisposedException>(() => closure.Invoke());
    }
    [Fact]
    public void CrossInstanceInputsRejectCapabilitiesButImportPlainContent()
    {
        using var contract = Generated.LoadContract(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "coflow.contract")));
        using var firstBuilder = new RuntimeBuilder(contract).AddSource("hero: Hero { name: \"old\", stats: Stats { health: 10 } } RuntimeSettings: RuntimeSettings {}");
        using var secondBuilder = new RuntimeBuilder(contract).AddSource("hero: Hero { name: \"new\", stats: Stats { health: 20 } } RuntimeSettings: RuntimeSettings {}");
        using var first = firstBuilder.Build(); using var second = secondBuilder.Build();
        var oldHero = first.Table<Character>()["hero"]; var newHero = second.Table<Character>()["hero"];
        Assert.NotEqual(oldHero, newHero);
        Assert.Equal(10, newHero.roundtrip(oldHero.stats).health);
        Assert.Throws<CoflowException>(() => newHero.recordIdentity(oldHero));
        var closure = oldHero.closure(1);
        Assert.Throws<CoflowException>(() => newHero.callbacks(new RuntimeArray<RuntimeFunction<int>>(new[] { closure })));
        first.Dispose();
        Assert.Equal("old", oldHero.name);
        Assert.Equal(10, newHero.roundtrip(oldHero.stats).health);
        Assert.Equal(21, newHero.score(1));
        Assert.Throws<ObjectDisposedException>(() => closure.Invoke());
    }
    [Fact]
    public void FailedCandidateBuildPreservesOldSnapshotAndExecutionIsThreadAffine()
    {
        using var contract = Generated.LoadContract(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "coflow.contract")));
        using var builder = new RuntimeBuilder(contract).AddSource("hero: Hero { name: \"Hero\", stats: Stats { health: 10 } } RuntimeSettings: RuntimeSettings {}");
        using var runtime = builder.Build(); var hero = runtime.Table<Character>()["hero"];
        using var invalid = new RuntimeBuilder(contract).AddSource("hero: Hero { name: \"bad\" }");
        Assert.Throws<BuildException>(() => invalid.Build());
        Assert.Equal(12, hero.score(2));
        // 主测试必须留在创建线程；独立线程只读取快照并尝试被禁止的执行。
        Exception? failure = null;
        var thread = new System.Threading.Thread(() => {
            try {
                Assert.Equal(10, hero.stats.health);
                Assert.Throws<CoflowException>(() => hero.score(1));
            } catch (Exception error) { failure = error; }
        });
        thread.Start(); thread.Join(); Assert.Null(failure);
        Assert.Equal(13, hero.score(3));
    }
    [Fact]
    public void BulkDecoderRejectsTruncationLengthsAndDanglingReferences()
    {
        Assert.Throws<CoflowException>(() => DataSnapshot.Read(Array.Empty<byte>()));
        byte[] valid = { 67, 70, 83, 80, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0 };
        DataSnapshot.Read(valid);
        for (int length = 0; length < valid.Length; ++length)
            Assert.Throws<CoflowException>(() => DataSnapshot.Read(valid.Take(length).ToArray()));
        var invalid = (byte[])valid.Clone(); invalid[12] = 255;
        Assert.Throws<CoflowException>(() => DataSnapshot.Read(invalid));
        Assert.Throws<CoflowException>(() => DataSnapshot.Read(valid.Concat(new byte[] { 0 }).ToArray()));
    }
}
