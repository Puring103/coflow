using Coflow;
using Game.Config;

using var builder = new RuntimeBuilder(Generated.Contract);
builder.AddSource("RuntimeSettings: RuntimeSettings {}");
using var runtime = builder.Build();
if (!runtime.Singleton<RuntimeSettings>().enabled) throw new InvalidOperationException("Default value mismatch.");
Console.WriteLine("netstandard-native-smoke-ok");
