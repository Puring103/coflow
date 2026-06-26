using System;
using System.IO;
using System.Linq;
using Example.Localization;
using Xunit;

namespace Example.Localization.Tests;

/// <summary>
/// End-to-end tests for the generated C# bindings of
/// `examples/localization/`. Each test loads the artifacts produced by
/// `coflow build`, wires the variant tables into a
/// <see cref="VariantLocalizationProvider"/>, and asserts that
/// `Localized&lt;T&gt;.For(language)` returns the translator's value, not
/// the source default.
///
/// The tests prove that:
///   1. Codegen wraps `@localized` fields as `Localized&lt;T&gt;`
///      preserving T's original CLR type (string, long, double, enum,
///      list, nested object).
///   2. The variant tables (`Item_nameVariants`, ...) are loaded with the
///      same translations the variant CSV/CFD files carry.
///   3. The static <see cref="Localization"/> entry point dispatches
///      lookups by language correctly.
///   4. Missing translations fall back to the default-language value
///      (covered by the trivial English copy of `phoenix_feather`).
/// </summary>
public class LocalizationTests
{
    private static readonly string DataDir = ResolveDataDir();

    private static string ResolveDataDir()
    {
        // Walk up from the test assembly until we hit the example project.
        var dir = new DirectoryInfo(AppContext.BaseDirectory);
        while (dir != null)
        {
            var candidate = Path.Combine(dir.FullName, "generated", "data");
            if (Directory.Exists(candidate))
            {
                return candidate;
            }
            dir = dir.Parent;
        }
        throw new DirectoryNotFoundException(
            "could not locate `generated/data/` — run `cargo run -p coflow -- build examples/localization/coflow.yaml` first");
    }

    private static (CoflowTables Tables, VariantLocalizationProvider Provider) Setup()
    {
        var tables = CoflowTables.Load(DataDir);
        var provider = VariantLocalizationProvider.FromTables(tables);
        // Replace the global provider for the duration of the test. xunit
        // serialises tests in a single class so the global state is fine.
        Localization.Provider = provider;
        Localization.CurrentLanguage = "default";
        return (tables, provider);
    }

    [Fact]
    public void DefaultValueComesFromSourceData()
    {
        var (tables, _) = Setup();
        var potion = tables.TbItem.Get("potion");
        Assert.Equal("Potion", potion.Name.Default);
        Assert.Equal("Restores 50 HP when consumed.", potion.Description.Default);
        // Numeric defaults are the source's value.
        Assert.Equal(50L, potion.WeightGrams.Default);
        Assert.Equal(1.0, potion.PriceFactor.Default);
        Assert.Equal(Region.Global, potion.Region.Default);
    }

    [Fact]
    public void StringFieldsTranslateForEachLanguage()
    {
        var (tables, _) = Setup();
        var potion = tables.TbItem.Get("potion");

        Assert.Equal("药水", potion.Name.For("zh"));
        Assert.Equal("Potion", potion.Name.For("en"));

        var elixir = tables.TbItem.Get("elixir");
        Assert.Equal("药水（强效）", elixir.Name.For("zh"));
        // English variant equals the source default — that's fine, it
        // means the translator copied the source as-is.
        Assert.Equal("Elixir", elixir.Name.For("en"));

        // Description: zh differs from default in both wording and length.
        Assert.NotEqual(elixir.Description.Default, elixir.Description.For("zh"));
        Assert.StartsWith("恢复", elixir.Description.For("zh"));
    }

    [Fact]
    public void NullableStringFieldsTranslateAndKeepNullSemantics()
    {
        var (tables, _) = Setup();
        var elixir = tables.TbItem.Get("elixir");
        // The example fills every variant; assert the translated value is
        // present and matches.
        Assert.NotNull(elixir.FlavorText.Default);
        Assert.Equal("晶莹剔透 草药气息浓郁。", elixir.FlavorText.For("zh"));
    }

    [Fact]
    public void IntegerFieldsTranslate()
    {
        var (tables, _) = Setup();

        // Elixir: zh weighs 125g, en weighs 120g (matches source).
        var elixir = tables.TbItem.Get("elixir");
        Assert.Equal(120L, elixir.WeightGrams.Default);
        Assert.Equal(125L, elixir.WeightGrams.For("zh"));
        Assert.Equal(120L, elixir.WeightGrams.For("en"));

        // Potion: en weight differs from default (55g vs 50g).
        var potion = tables.TbItem.Get("potion");
        Assert.Equal(50L, potion.WeightGrams.Default);
        Assert.Equal(50L, potion.WeightGrams.For("zh"));
        Assert.Equal(55L, potion.WeightGrams.For("en"));
    }

    [Fact]
    public void FloatFieldsTranslate()
    {
        var (tables, _) = Setup();
        var potion = tables.TbItem.Get("potion");
        Assert.Equal(1.0, potion.PriceFactor.Default);
        Assert.Equal(0.7, potion.PriceFactor.For("zh"));
        Assert.Equal(1.0, potion.PriceFactor.For("en"));

        var feather = tables.TbItem.Get("phoenix_feather");
        Assert.Equal(0.9, feather.PriceFactor.For("zh"));
        Assert.Equal(1.2, feather.PriceFactor.For("en"));
    }

    [Fact]
    public void EnumFieldsTranslate()
    {
        var (tables, _) = Setup();
        var elixir = tables.TbItem.Get("elixir");
        // Same record, three different enum values per locale — proves
        // enum localization is actually being applied.
        Assert.Equal(Region.Global, elixir.Region.Default);
        Assert.Equal(Region.CnOnly, elixir.Region.For("zh"));
        Assert.Equal(Region.EuOnly, elixir.Region.For("en"));
    }

    [Fact]
    public void ListFieldsTranslateAsAWhole()
    {
        var (tables, _) = Setup();
        var fireball = tables.TbSkill.Get("fireball");

        // Default tags are the source data.
        Assert.Equal(new[] { "fire", "ranged", "aoe" }, fireball.Tags.Default);

        // zh tags fully translated.
        Assert.Equal(new[] { "火焰", "远程", "群体" }, fireball.Tags.For("zh"));

        // en tags echo the default (translator copied as-is).
        Assert.Equal(new[] { "fire", "ranged", "aoe" }, fireball.Tags.For("en"));
    }

    [Fact]
    public void NestedObjectFieldsTranslate()
    {
        var (tables, _) = Setup();
        var fireball = tables.TbSkill.Get("fireball");

        // The whole TooltipStyle object is per-variant: background colour
        // differs between default and zh.
        Assert.Equal("#400000", fireball.Tooltip.Default.Background);
        Assert.Equal("#5a1a1a", fireball.Tooltip.For("zh").Background);
        Assert.NotEqual(
            fireball.Tooltip.Default.Background,
            fireball.Tooltip.For("zh").Background);
    }

    [Fact]
    public void SingletonHudResolvesPerVariantValues()
    {
        var (tables, _) = Setup();
        // Singleton type → single-instance accessor on the database. The
        // synthesized variant record key is the field name (not a row
        // key), so the generated `Localized<T>.Key` is `"Hud/<field>"`
        // with no record-key suffix.
        var hud = tables.Hud;

        Assert.Equal("Welcome, adventurer!", hud.Welcome.Default);
        Assert.Equal("欢迎，冒险者！", hud.Welcome.For("zh"));
        Assert.Equal("Welcome, adventurer!", hud.Welcome.For("en"));

        Assert.Equal(800L, hud.BannerWidthPx.Default);
        Assert.Equal(720L, hud.BannerWidthPx.For("zh"));
        Assert.Equal(800L, hud.BannerWidthPx.For("en"));
    }

    [Fact]
    public void CurrentLanguageDispatchesAtAccessTime()
    {
        var (tables, _) = Setup();
        var potion = tables.TbItem.Get("potion");

        Localization.CurrentLanguage = "zh";
        Assert.Equal("药水", potion.Name.Value);

        Localization.CurrentLanguage = "en";
        Assert.Equal("Potion", potion.Name.Value);

        Localization.CurrentLanguage = "default";
        Assert.Equal("Potion", potion.Name.Value);
    }

    [Fact]
    public void MissingLanguageFallsBackToDefault()
    {
        var (tables, _) = Setup();
        var potion = tables.TbItem.Get("potion");

        // No variant column "fr" exists — the provider falls back to the
        // default value the wrapper packs.
        Assert.Equal("Potion", potion.Name.For("fr"));
        Assert.Equal(50L, potion.WeightGrams.For("fr"));
        Assert.Equal(Region.Global, potion.Region.For("fr"));
    }

    [Fact]
    public void KeysFollowCodegenContract()
    {
        var (tables, _) = Setup();
        var potion = tables.TbItem.Get("potion");

        // Keys are stable and follow the spec 13 §3 format
        // `OwnerType/field/recordKey` (singletons drop the record-key
        // segment because the field IS the key).
        Assert.Equal("Item/name/potion", potion.Name.Key);
        Assert.Equal("Item/description/potion", potion.Description.Key);
        Assert.Equal("Item/weight_grams/potion", potion.WeightGrams.Key);
        Assert.Equal("Item/region/potion", potion.Region.Key);

        var hud = tables.Hud;
        Assert.Equal("Hud/welcome", hud.Welcome.Key);
        Assert.Equal("Hud/banner_width_px", hud.BannerWidthPx.Key);
    }

    [Fact]
    public void NonLocalizedFieldsAreNotWrapped()
    {
        var (tables, _) = Setup();
        var potion = tables.TbItem.Get("potion");

        // `rarity` and `price` are plain CLR types — no Localized<> sleeve.
        // The compile-time type system is the assertion: if these were
        // wrapped, the explicit cast below would fail to compile.
        Rarity rarity = potion.Rarity;
        long price = potion.Price;

        Assert.Equal(Rarity.Common, rarity);
        Assert.Equal(20L, price);
    }
}
