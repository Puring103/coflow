using System;
using System.Collections.Generic;
using Example.Localization;

namespace Example.Localization.Tests;

/// <summary>
/// Test-side <see cref="LocalizationProvider"/> that mirrors the engine's
/// authoritative variant tables (`Item_nameVariants`, `Item_regionVariants`,
/// ...) back into the static <see cref="Localization"/> entry point.
///
/// Real hosts would wire this up once at startup against their loaded
/// `CoflowTables`; here we do the same so the assertions exercise the same
/// code path as production callers.
///
/// The provider stores values keyed by `(language, key)` where `key` is the
/// codegen-emitted "OwnerType/field/recordKey" string (or
/// "OwnerType/field" for singletons). Lookups cast the stored value to
/// the requested type; a type mismatch is a wiring bug and panics, since
/// the wrapper's `T` is fixed by codegen.
/// </summary>
internal sealed class VariantLocalizationProvider : LocalizationProvider
{
    private readonly Dictionary<(string Language, string Key), object?> _values = new();

    public void Add(string language, string key, object? value)
    {
        _values[(language, key)] = value;
    }

    public T Resolve<T>(string language, string key, T defaultValue)
    {
        if (!_values.TryGetValue((language, key), out var stored))
        {
            return defaultValue;
        }
        // `null` means "translator left this cell empty" — the wrapper
        // contract is to fall back to the default-language value in that
        // case (see Localized.cs).
        if (stored is null)
        {
            return defaultValue;
        }
        return (T)stored;
    }

    /// <summary>
    /// Build a provider from the example's loaded `CoflowTables`, mapping
    /// every variant record back into the `(language, key) → value` form
    /// the <see cref="LocalizationProvider"/> contract expects.
    /// </summary>
    public static VariantLocalizationProvider FromTables(CoflowTables tables)
    {
        var provider = new VariantLocalizationProvider();

        foreach (var row in tables.TbItemNameVariants)
        {
            var key = $"Item/name/{row.Id}";
            provider.Add("zh", key, row.Zh);
            provider.Add("en", key, row.En);
        }
        foreach (var row in tables.TbItemDescriptionVariants)
        {
            var key = $"Item/description/{row.Id}";
            provider.Add("zh", key, row.Zh);
            provider.Add("en", key, row.En);
        }
        foreach (var row in tables.TbItemFlavorTextVariants)
        {
            var key = $"Item/flavor_text/{row.Id}";
            provider.Add("zh", key, row.Zh);
            provider.Add("en", key, row.En);
        }
        foreach (var row in tables.TbItemWeightGramsVariants)
        {
            var key = $"Item/weight_grams/{row.Id}";
            provider.Add("zh", key, row.Zh);
            provider.Add("en", key, row.En);
        }
        foreach (var row in tables.TbItemPriceFactorVariants)
        {
            var key = $"Item/price_factor/{row.Id}";
            provider.Add("zh", key, row.Zh);
            provider.Add("en", key, row.En);
        }
        foreach (var row in tables.TbItemRegionVariants)
        {
            var key = $"Item/region/{row.Id}";
            provider.Add("zh", key, row.Zh);
            provider.Add("en", key, row.En);
        }
        foreach (var row in tables.TbSkillNameVariants)
        {
            var key = $"Skill/name/{row.Id}";
            provider.Add("zh", key, row.Zh);
            provider.Add("en", key, row.En);
        }
        foreach (var row in tables.TbSkillTagsVariants)
        {
            var key = $"Skill/tags/{row.Id}";
            provider.Add("zh", key, row.Zh);
            provider.Add("en", key, row.En);
        }
        foreach (var row in tables.TbSkillTooltipVariants)
        {
            var key = $"Skill/tooltip/{row.Id}";
            provider.Add("zh", key, row.Zh);
            provider.Add("en", key, row.En);
        }
        // Singleton variant tables: every variant record's `Id` is the
        // field name; codegen emits `"Hud/<field>"` as the key (no
        // record-key suffix), so we strip the record-key half here.
        foreach (var row in tables.TbHudWelcomeVariants)
        {
            provider.Add("zh", "Hud/welcome", row.Zh);
            provider.Add("en", "Hud/welcome", row.En);
        }
        foreach (var row in tables.TbHudHintVariants)
        {
            provider.Add("zh", "Hud/hint", row.Zh);
            provider.Add("en", "Hud/hint", row.En);
        }
        foreach (var row in tables.TbHudBannerWidthPxVariants)
        {
            provider.Add("zh", "Hud/banner_width_px", row.Zh);
            provider.Add("en", "Hud/banner_width_px", row.En);
        }

        return provider;
    }
}
