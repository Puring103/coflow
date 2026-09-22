using Coflow;

namespace Game.Config
{
public static class Generated
{
    internal static byte[] ContractIdentity { get; } = new byte[] { 32,102,162,141,13,147,68,107,85,214,5,14,135,137,187,195,21,8,82,130,173,127,32,36,190,140,100,132,48,180,123,194 };

    private static TypeBinding[] Bindings { get; } = new TypeBinding[]
    {
        new TypeBinding<global::Game.Config.Character>("Character", value => new global::Game.Config.Character(new Record(value))),
        new TypeBinding<global::Game.Config.DimensionBase>("DimensionBase", value => new global::Game.Config.DimensionBase(new Record(value))),
        new TypeBinding<global::Game.Config.DimensionChild>("DimensionChild", value => new global::Game.Config.DimensionChild(new Record(value))),
        new TypeBinding<global::Game.Config.Hero>("Hero", value => new global::Game.Config.Hero(new Record(value))),
        new TypeBinding<global::Game.Config.HostServices>("HostServices", value => new global::Game.Config.HostServices(new Record(value))),
        new TypeBinding<global::Game.Config.Item>("Item", value => new global::Game.Config.Item(new Record(value))),
        new TypeBinding<global::Game.Config.ItemStats>("ItemStats", value => new global::Game.Config.ItemStats(value)),
        new TypeBinding<global::Game.Config.LocalizedText>("LocalizedText", value => new global::Game.Config.LocalizedText(new Record(value))),
        new TypeBinding<global::Game.Config.Profile>("Profile", value => new global::Game.Config.Profile(new Record(value))),
        new TypeBinding<global::Game.Config.RuntimeSettings>("RuntimeSettings", value => new global::Game.Config.RuntimeSettings(new Record(value))),
        new TypeBinding<global::Game.Config.Services>("Services", value => new global::Game.Config.Services(new Record(value))),
        new TypeBinding<global::Game.Config.Stats>("Stats", value => new global::Game.Config.Stats(value)),
        new TypeBinding<global::Game.Config.ThemeValue>("ThemeValue", value => new global::Game.Config.ThemeValue(value)),
        new TypeBinding<global::Game.Config.UiText>("UiText", value => new global::Game.Config.UiText(new Record(value)))
    };

    public static Contract LoadContract(byte[] bytes) => new Contract(bytes, ContractIdentity, Bindings);
}
}
