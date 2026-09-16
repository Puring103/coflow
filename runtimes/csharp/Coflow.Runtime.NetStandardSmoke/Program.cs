using Coflow;
using Game.Config;

using var contract = Generated.LoadContract(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "coflow.contract")));
using var builder = new RuntimeBuilder(contract);
builder.AddSource("RuntimeSettings: RuntimeSettings {}");
using var runtime = builder.Build();
if (!runtime.Singleton<RuntimeSettings>().enabled) throw new InvalidOperationException("Default value mismatch.");
Console.WriteLine("netstandard-native-smoke-ok");
