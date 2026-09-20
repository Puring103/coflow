using BenchmarkDotNet.Attributes;
using BenchmarkDotNet.Running;
using System;
using System.IO;
using Coflow;
using Game.Config;

if (args.Length == 1 && args[0] == "--probe") SnapshotProbe.Run();
else BenchmarkRunner.Run<DataAccess>();
[MemoryDiagnoser]
public class DataAccess
{
    private Contract contract = null!;
    private Runtime runtime = null!;
    private Character hero = null!;
    [GlobalSetup]
    public void Setup()
    {
        contract = Generated.LoadContract(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "coflow.contract")));
        using var builder = new RuntimeBuilder(contract);
        builder.AddSource("hero: Character { name: \"Hero\", stats: Stats { health: 100 } } RuntimeSettings: RuntimeSettings {}");
        runtime = builder.Build();
        hero = runtime.Table<Character>()["hero"];
    }
    [Benchmark] public string ReadName() => hero.name;
    [Benchmark] public int ReadNestedValue() => hero.stats.health;
    [GlobalCleanup] public void Cleanup() { runtime.Dispose(); contract.Dispose(); }
}
