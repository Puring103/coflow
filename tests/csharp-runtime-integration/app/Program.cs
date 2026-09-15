using System;
using Coflow.Runtime;
using Game.Config;

internal static class Program
{
    private static void Main()
    {
        using var contract = CoflowSchema.Load();
        using var builder = contract.CreateBuilder();
        builder.AddSource("characters.cfd", "hero: Character { name: \"Hero\", stats: Stats { health: 100 } } RuntimeSettings: RuntimeSettings {}");
        using var runtime = builder.Build();
        using var hero = Character.Wrap(runtime.Record("Character", "hero"));
        using var stats = hero.stats;
        if (hero.name != "Hero" || stats.health != 100) throw new Exception("Generated data mismatch.");
        using var function = hero.score;
        try { function.Call(); throw new Exception("Execution must remain unavailable."); }
        catch (CoflowException) { }
        Console.WriteLine("csharp-runtime-integration-ok");
    }
}
