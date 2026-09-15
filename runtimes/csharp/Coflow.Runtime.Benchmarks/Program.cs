using BenchmarkDotNet.Attributes;
using BenchmarkDotNet.Running;
using Coflow.Runtime;
using Game.Config;

BenchmarkRunner.Run<DataAccess>();

public class DataAccess
{
    private CoflowRuntime runtime = null!;
    private Character hero = null!;
    [GlobalSetup]
    public void Setup()
    {
        using var contract = CoflowSchema.Load();
        using var builder = contract.CreateBuilder();
        builder.AddSource("data.cfd", "hero: Character { name: \"Hero\", stats: Stats { health: 100 } } RuntimeSettings: RuntimeSettings {}");
        runtime = builder.Build();
        hero = Character.Wrap(runtime.Record("Character", "hero"));
    }
    [Benchmark] public string ReadName() => hero.name;
    [GlobalCleanup] public void Cleanup() { hero.Dispose(); runtime.Dispose(); }
}
