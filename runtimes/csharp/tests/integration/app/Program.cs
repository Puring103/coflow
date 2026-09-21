using System;
using System.IO;
using System.Linq;
using Coflow;
using Game.Config;

internal static class Program
{
    private static void Main()
    {
        using var contract = Generated.LoadContract(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "coflow.contract")));
        using var builder = new RuntimeBuilder(contract);
        builder.AddSource(
            "hero: Hero { name: \"Hero\", stats: Stats { health: 100 } } " +
            "RuntimeSettings: RuntimeSettings {} " +
            "localized: LocalizedText { value: dimension { default: \"Hello\", zh: \"你好\" } } " +
            "ui: UiText { welcome: dimension { default: \"Hello\", zh: \"你好\" }, " +
            "weights: dimension { default: [1, 2], zh: [3, 4, 5] }, " +
            "theme: dimension { default: ThemeValue { value: 5 }, zh: ThemeValue { value: 9 } }, count: 17 } " +
            "child: DimensionChild { name: dimension { default: \"Base name\", zh: \"Translated name\" }, " +
            "hint: dimension { default: \"Base hint\", mobile: \"Tap\", desktop: \"Click\" } }");
        using var runtime = builder.Build();
        var hero = runtime.Table<Character>().Get("hero");
        if (!(hero is Hero) || hero.name != "Hero" || hero.stats.health != 100) throw new Exception("Generated data mismatch.");
        if (!runtime.Get<RuntimeSettings>().enabled) throw new Exception("Singleton mismatch.");
        if (hero.score(23) != 123) throw new Exception("Function execution mismatch.");
        if (hero.text.Render() != "Hero") throw new Exception("Template execution mismatch.");
        var localized = runtime.Table<LocalizedText>().Get("localized").value;
        if (localized.Default() != "Hello" || localized.For("zh") != "你好") throw new Exception("Dimension lookup mismatch.");
        var variants = localized.Variants();
        if (variants.Count != 1 || variants["zh"] != "你好") throw new Exception("Dimension variants mismatch.");
        var text = runtime.Table<UiText>().Get("ui");
        if (!text.weights.For("zh").SequenceEqual(new[] { 3, 4, 5 })) throw new Exception("Dimension collection mismatch.");
        if (text.theme.For("zh").value != 9) throw new Exception("Dimension object mismatch.");
        var child = runtime.Table<DimensionBase>().Get("child");
        if (child.name.For("zh") != "Translated name" || child.hint.For("mobile") != "Tap")
            throw new Exception("Inherited dimension mismatch.");
        Console.WriteLine("csharp-runtime-integration-ok");
    }
}
