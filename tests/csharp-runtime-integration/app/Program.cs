using System;
using Coflow;
using Game.Config;

internal static class Program
{
    private static void Main()
    {
        using var builder = new RuntimeBuilder(Generated.Contract);
        builder.AddSource("hero: Hero { name: \"Hero\", stats: Stats { health: 100 } } RuntimeSettings: RuntimeSettings {}");
        using var runtime = builder.Build();
        var hero = runtime.Table<Character>()["hero"];
        if (!(hero is Hero) || hero.name != "Hero" || hero.stats.health != 100) throw new Exception("Generated data mismatch.");
        if (!runtime.Singleton<RuntimeSettings>().enabled) throw new Exception("Singleton mismatch.");
        bool unavailable = false;
        try { hero.score.Call(); } catch (CoflowException) { unavailable = true; }
        if (!unavailable) throw new Exception("Execution must remain unavailable.");
        Console.WriteLine("csharp-runtime-integration-ok");
    }
}
