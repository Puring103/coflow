using Coflow;
using Coflow.Generated;

using var contract = Generated.LoadContract(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "coflow.contract")));
using var builder = new RuntimeBuilder(contract);
builder.AddSource("""
UiText: UiText {
  welcome: dimension { default: "Hello", zh: "你好" },
  weights: dimension { default: [1, 2], zh: [3, 4, 5] },
  theme: dimension { default: ThemeValue { value: 5 }, zh: ThemeValue { value: 9 } },
  count: 17,
}
child: DimensionChild {
  name: dimension { default: "Base name", zh: "Translated name" },
  hint: dimension { default: "Base hint", mobile: "Tap", desktop: "Click" },
}
""");
using var runtime = builder.Build();

var text = runtime.Singleton<UiText>();
if (text.welcome.Default() != "Hello" || text.welcome.For("zh") != "你好")
    throw new InvalidOperationException("Dimension lookup failed.");
var variants = text.welcome.Variants();
if (variants.Count != 1 || variants["zh"] != "你好")
    throw new InvalidOperationException("Dynamic dimension union or fallback failed.");
if (!text.weights.For("zh").SequenceEqual(new[] { 3, 4, 5 }))
    throw new InvalidOperationException("Dimension collection value failed.");
if (text.theme.For("zh").value != 9)
    throw new InvalidOperationException("Dimension object value failed.");

var child = runtime.Table<DimensionBase>()["child"];
if (child.name.For("zh") != "Translated name" || child.hint.For("mobile") != "Tap")
    throw new InvalidOperationException("Inherited dimension field failed.");

Console.WriteLine("csharp-runtime-redesign-ok");
