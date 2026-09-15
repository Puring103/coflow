using System;
using Coflow.Runtime;
using Game.Config;
using Xunit;

public sealed class NativeRuntimeTests
{
    private static CoflowRuntime Build(ICoflowHost? host = null)
    {
        using var contract = CoflowSchema.Load();
        using var builder = contract.CreateBuilder();
        builder.AddSource("data.cfd", "hero: Character { name: \"Hero\", stats: Stats { health: 100, weights: [1.25, 2.5] }, labels: { true: \"yes\", false: \"no\" } } RuntimeSettings: RuntimeSettings {}");
        if (host != null) builder.BindHost("HostServices", host);
        return builder.Build();
    }

    [Fact]
    public void GeneratedObjectsStructsCollectionsAndOptionalValuesUseNativeData()
    {
        using var runtime = Build();
        using var hero = Character.Wrap(runtime.Record("Character", "hero"));
        Assert.Equal("hero", hero.Id);
        Assert.Equal("Hero", hero.name);
        using var stats = hero.stats;
        Assert.Equal(100, stats.health);
        using var weights = stats.weights;
        Assert.Equal(2, weights.Count);
        Assert.Equal(1.25f, weights[0]);
        using var labels = hero.labels;
        Assert.Equal("yes", labels[true]);
        Assert.Equal("no", labels[false]);
        using var friend = hero.friend;
        Assert.False(friend.HasValue);
    }

    [Fact]
    public void FunctionsAndTemplatesKeepSourceAndReportUnavailableExecution()
    {
        using var runtime = Build();
        using var hero = Character.Wrap(runtime.Record("Character", "hero"));
        using var template = hero.Get_text_Template();
        Assert.Contains("self.name", template.ProgramSource);
        Assert.Throws<CoflowException>(() => template.Text);
        using var function = hero.score;
        Assert.Contains("bonus", function.Source);
        Assert.Throws<CoflowException>(() => function.Call());
    }

    [Fact]
    public void ExplicitDisposeInvalidatesDependentHandles()
    {
        var runtime = Build();
        using var record = runtime.Record("Character", "hero");
        using var retained = record.Retain();
        runtime.Dispose();
        Assert.Throws<CoflowException>(() => retained.TypeName);
        runtime.Dispose();
    }

    [Fact]
    public void ValueWrapperKeepsRuntimeAliveAcrossGarbageCollection()
    {
        var runtime = Build();
        using var record = runtime.Record("Character", "hero");
        runtime = null!;
        GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect();
        using var name = record.Field("name");
        Assert.Equal("Hero", name.Text);
    }

    [Fact]
    public void HostBindingsAreLazyAndExceptionsBecomeCoflowErrors()
    {
        using var missing = Build();
        using var service = missing.Record("HostServices", "HostServices");
        using var environment = service.Field("environment");
        Assert.Throws<CoflowException>(() => environment.Text);
        using var bound = Build(new Host());
        using var boundService = bound.Record("HostServices", "HostServices");
        using var boundEnvironment = boundService.Field("environment");
        Assert.Equal("Unity", boundEnvironment.Text);
    }

    private sealed class Host : ICoflowHost
    {
        public string MemberType(string field) => field == "environment" ? "string" : "fn(string) -> ()";
        public object Read(string field) => field == "environment" ? "Unity" : throw new InvalidOperationException("not a data member");
    }
}
