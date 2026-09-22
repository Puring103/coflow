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
            "hero: Hero { name: \"Hero\", stats: Stats { health: 100 }, " +
            "profile: Profile { title: \"Leader\", stats: Stats { health: 80 }, owner: &Character::hero }, " +
            "profiles: [Profile { title: \"Array\", stats: Stats { health: 70 }, owner: &Character::hero }], " +
            "profilesByName: { \"main\": Profile { title: \"Map\", stats: Stats { health: 60 }, owner: &Character::hero } } } " +
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
        var profile = hero.profile ?? throw new Exception("Nested data missing.");
        if (profile.title != "Leader" || profile.stats.health != 80) throw new Exception("Nested data mismatch.");
        if (!ReferenceEquals(hero, profile.owner)) throw new Exception("Nested data cycle mismatch.");
        if (hero.profiles[0].title != "Array" || !ReferenceEquals(hero, hero.profiles[0].owner)) throw new Exception("Nested data array mismatch.");
        if (hero.profilesByName["main"].title != "Map" || !ReferenceEquals(hero, hero.profilesByName["main"].owner)) throw new Exception("Nested data dictionary mismatch.");
        var returnedProfile = hero.profileIdentity(profile);
        if (returnedProfile.title != "Leader" || returnedProfile.stats.health != 80) throw new Exception("Nested data roundtrip mismatch.");
        var detachedProfile = new Profile("Detached", new Stats(33, new CoflowArray<float>(Array.Empty<float>()), null), null);
        var importedProfile = hero.profileIdentity(detachedProfile);
        if (importedProfile.title != "Detached" || importedProfile.stats.health != 33) throw new Exception("Detached data roundtrip mismatch.");
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
