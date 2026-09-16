using Coflow;
namespace @Coflow.@Generated { public static class Generated { internal static byte[] ContractIdentity { get; } = new byte[] { 246,187,162,1,213,207,240,222,21,68,7,26,216,246,151,68,91,175,98,168,59,74,235,247,125,31,194,190,88,221,122,241 }; private static TypeBinding[] Bindings { get; } = new TypeBinding[] { new TypeBinding<global::@Coflow.@Generated.@DimensionBase>("DimensionBase", global::@Coflow.@Generated.@DimensionBase.Wrap),
new TypeBinding<global::@Coflow.@Generated.@DimensionChild>("DimensionChild", global::@Coflow.@Generated.@DimensionChild.Wrap),
new TypeBinding<global::@Coflow.@Generated.@Item>("Item", global::@Coflow.@Generated.@Item.Wrap),
new TypeBinding<global::@Coflow.@Generated.@Services>("Services", global::@Coflow.@Generated.@Services.Wrap),
new TypeBinding<global::@Coflow.@Generated.@Stats>("Stats", global::@Coflow.@Generated.@Stats.Wrap),
new TypeBinding<global::@Coflow.@Generated.@ThemeValue>("ThemeValue", global::@Coflow.@Generated.@ThemeValue.Wrap),
new TypeBinding<global::@Coflow.@Generated.@UiText>("UiText", global::@Coflow.@Generated.@UiText.Wrap) }; public static Contract LoadContract(byte[] bytes) => new Contract(bytes, ContractIdentity, Bindings); } }
