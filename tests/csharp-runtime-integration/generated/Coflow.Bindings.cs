using Coflow;
namespace @Game.@Config { public static class Generated { internal static byte[] ContractIdentity { get; } = new byte[] { 204,27,205,119,198,95,187,72,168,171,22,229,191,123,105,185,150,98,41,160,97,147,170,214,238,224,111,41,90,125,95,87 }; private static TypeBinding[] Bindings { get; } = new TypeBinding[] { new TypeBinding<global::@Game.@Config.@Character>("Character", global::@Game.@Config.@Character.Wrap),
new TypeBinding<global::@Game.@Config.@Hero>("Hero", global::@Game.@Config.@Hero.Wrap),
new TypeBinding<global::@Game.@Config.@HostServices>("HostServices", global::@Game.@Config.@HostServices.Wrap),
new TypeBinding<global::@Game.@Config.@LocalizedText>("LocalizedText", global::@Game.@Config.@LocalizedText.Wrap),
new TypeBinding<global::@Game.@Config.@RuntimeSettings>("RuntimeSettings", global::@Game.@Config.@RuntimeSettings.Wrap),
new TypeBinding<global::@Game.@Config.@Stats>("Stats", global::@Game.@Config.@Stats.Wrap) }; public static Contract LoadContract(byte[] bytes) => new Contract(bytes, ContractIdentity, Bindings); } }
