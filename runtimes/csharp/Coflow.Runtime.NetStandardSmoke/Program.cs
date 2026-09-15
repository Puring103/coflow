using Coflow.Runtime;

using var contract = Game.Config.CoflowSchema.Load();
using var builder = contract.CreateBuilder();
builder.AddSource("settings.cfd", "RuntimeSettings: RuntimeSettings {}");
using var runtime = builder.Build();
using var settings = runtime.Record("RuntimeSettings", "RuntimeSettings");
using var enabled = settings.Field("enabled");
if (!enabled.Bool) throw new InvalidOperationException("Default value mismatch.");
Console.WriteLine("netstandard-native-smoke-ok");
