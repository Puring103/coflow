using System;
using System.IO;
using Coflow.Loader;
using Game.Config;

internal static class Program
{
    private static void Require(bool condition, string message)
    { if (!condition) throw new Exception(message); }

    private static void Main()
    {
        var root = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../../../"));
        var loaded = CoflowData.Load(new[] {
            new CfdSource("ja.cfd", File.ReadAllText(Path.Combine(root, "data/ja.cfd"))),
            new CfdSource("base.cfd", File.ReadAllText(Path.Combine(root, "data/base.cfd"))),
        });
        Require(loaded.Success, string.Join("; ", loaded.Diagnostics));
        var data = loaded.Data;
        var alpha = data.Item["alpha"];
        var beta = data.Item["beta"];
        Require(typeof(Stats).IsValueType, "Stats must be a struct");
        Require(ReferenceEquals(alpha.next, beta) && ReferenceEquals(beta.next, alpha), "circular references must preserve identity");
        Require(ReferenceEquals(data.Entity["alpha"], alpha) && ReferenceEquals(data.Settings.start, alpha), "views and singleton references must share objects");
        Require(alpha.name.Default == "Alpha" && alpha.name.For("ja") == "アルファ" && alpha.name.For("unknown") == "Alpha", "dimension selection and fallback");
        Require(alpha.enum_label == "3", "generated enum interpolation");
        Require(alpha.label == "{item} Alpha: 12", "basic nested interpolation");
        Require(alpha.stats.extra.settings.hp == 9, "nested typed defaults and loader-name collisions");
        Require(alpha.price == 3 && alpha.permissions == (Flags.Read | Flags.Write), "generated defaults and flags");
        Require(alpha.nums.Count == 2 && alpha.indexed["score"] == 5, "typed collections");
        Console.WriteLine("Generated C# direct CFD loading passed.");
    }
}
