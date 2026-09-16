using BenchmarkDotNet.Attributes;
using BenchmarkDotNet.Running;
using Coflow;
using Game.Config;

BenchmarkRunner.Run<DataAccess>();
public class DataAccess
{
    private Runtime runtime = null!;
    private Character hero = null!;
    [GlobalSetup]
    public void Setup()
    {
        using var builder = new RuntimeBuilder(Generated.Contract);
        builder.AddSource("hero: Character { name: \"Hero\", stats: Stats { health: 100 } } RuntimeSettings: RuntimeSettings {}");
        runtime = builder.Build();
        hero = runtime.Table<Character>()["hero"];
    }
    [Benchmark] public string ReadName() => hero.name;
    [Benchmark] public int ReadNestedValue() => hero.stats.health;
    [GlobalCleanup] public void Cleanup() => runtime.Dispose();
}
