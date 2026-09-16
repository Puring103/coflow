using System;
using System.IO;
using Coflow;
using Game.Config;

internal static class Program
{
    private static void Main()
    {
        using var contract = Generated.LoadContract(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "coflow.contract")));
        using var builder = new RuntimeBuilder(contract);
        builder.AddSource("hero: Hero { name: \"Hero\", stats: Stats { health: 100 } } RuntimeSettings: RuntimeSettings {} localized: LocalizedText { value: dimension { default: \"Hello\", zh: \"你好\" } }");
        using var runtime = builder.Build();
        var hero = runtime.Table<Character>()["hero"];
        if (!(hero is Hero) || hero.name != "Hero" || hero.stats.health != 100) throw new Exception("Generated data mismatch.");
        if (!runtime.Singleton<RuntimeSettings>().enabled) throw new Exception("Singleton mismatch.");
        if (hero.score.Invoke(23) != 123) throw new Exception("Function execution mismatch.");
        if (hero.text != "Hero") throw new Exception("Template execution mismatch.");
        var localized = runtime.Table<LocalizedText>()["localized"].value;
        if (localized.Default() != "Hello" || localized.For("zh") != "你好") throw new Exception("Dimension lookup mismatch.");
        var variants = localized.Variants();
        if (variants.Count != 1 || variants["zh"] != "你好") throw new Exception("Dimension variants mismatch.");
        Console.WriteLine("csharp-runtime-integration-ok");
    }
}
