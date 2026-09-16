using Coflow;
namespace @Game.@Config { public static class Generated { internal static byte[] ContractIdentity { get; } = new byte[] { 229,85,10,19,243,233,184,166,65,107,131,1,49,188,49,185,238,55,236,54,79,124,186,36,93,27,97,91,175,196,185,112 }; private static TypeBinding[] Bindings { get; } = new TypeBinding[] { new TypeBinding<global::@Game.@Config.@Character>("Character", global::@Game.@Config.@Character.Wrap),
new TypeBinding<global::@Game.@Config.@Hero>("Hero", global::@Game.@Config.@Hero.Wrap),
new TypeBinding<global::@Game.@Config.@HostServices>("HostServices", global::@Game.@Config.@HostServices.Wrap),
new TypeBinding<global::@Game.@Config.@RuntimeSettings>("RuntimeSettings", global::@Game.@Config.@RuntimeSettings.Wrap),
new TypeBinding<global::@Game.@Config.@Stats>("Stats", global::@Game.@Config.@Stats.Wrap) }; public static Contract LoadContract(byte[] bytes) => new Contract(bytes, ContractIdentity, Bindings); } }
