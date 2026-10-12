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
        var sources = new[] {
            new CfdSource("ja.cfd", File.ReadAllText(Path.Combine(root, "data/ja.cfd"))),
            new CfdSource("base.cfd", File.ReadAllText(Path.Combine(root, "data/base.cfd"))),
        };
        var loaded = CoflowData.Load(sources);
        Require(loaded.Success, string.Join("; ", loaded.Diagnostics));
        var data = loaded.Data;
        var alpha = data.Table<Item>()["alpha"];
        var beta = data.Table<Item>()["beta"];
        Require(typeof(Stats).IsValueType, "Stats must be a struct");
        Require(data.ItemCount == 2 && !alpha.IsFree && alpha.stats.IsAlive, "partial class, struct and data access extensions");
        Require(data.Singleton<Settings>().StartKey == "alpha", "partial singleton");
        Require(ReferenceEquals(data.Table<Item>(), data.Table<Item>()), "table access reuses registered table");
        Require(ReferenceEquals(data.Table<Item>().Get(EntityId.alpha), alpha), "enum key resolves to original identity");
        Require(data.Table<Item>().TryGet(EntityId.beta, out var enumBeta) && ReferenceEquals(enumBeta, beta), "typed enum lookup");
        Require(!data.Table<Item>().TryGet((EntityId)long.MaxValue, out _), "unknown enum value returns false");
        try { data.Table<Item>().Get((EntityId)long.MaxValue); throw new Exception("invalid enum must fail"); }
        catch (System.Collections.Generic.KeyNotFoundException) { }
        try { data.Table<Payload>(); throw new Exception("data must not register as table"); }
        catch (InvalidOperationException) { }
        try { data.Table<Settings>(); throw new Exception("singleton must not register as table"); }
        catch (InvalidOperationException) { }
        try { data.Singleton<Item>(); throw new Exception("table must not register as singleton"); }
        catch (InvalidOperationException) { }
        Require(ReferenceEquals(alpha.next.Value, beta) && ReferenceEquals(beta.next.Value, alpha), "circular references must preserve identity");
        Require(ReferenceEquals(data.Table<Entity>()["alpha"], alpha) && ReferenceEquals(data.Singleton<Settings>().start, alpha), "views and singleton references must share objects");
        Require(alpha.name.Default == "Alpha" && alpha.name.For("ja") == "アルファ" && alpha.name.For("unknown") == "Alpha", "dimension selection and fallback");
        Require(alpha.enum_label == "3", "generated enum interpolation");
        Require(alpha.label == "{item} Alpha: 12", "basic nested interpolation");
        Require(alpha.stats.extra.settings.hp == 9, "nested typed defaults and loader-name collisions");
        Require(alpha.price == 3 && alpha.permissions == (Flags.Read | Flags.Write), "generated defaults and flags");
        Require(alpha.nums.Count == 2 && alpha.indexed["score"] == 5, "typed collections");
        // 同一组夹具同时经过 Rust loader 和真正生成的 C# 类型，校验接受结果及业务值。
        var fixtures = Directory.GetFiles(Path.Combine(root, "../fixtures/cfd-runtime-parity"), "*.cfd");
        Require(fixtures.Length >= 7, "shared runtime corpus is incomplete");
        foreach (var fixture in fixtures)
        {
            var input = new CfdSource[sources.Length + 1];
            Array.Copy(sources, input, sources.Length);
            input[sources.Length] = new CfdSource(fixture, File.ReadAllText(fixture));
            var result = CoflowData.Load(input);
            var valid = fixture.EndsWith(".valid.cfd", StringComparison.Ordinal);
            Require(result.Success == valid, fixture + ": " + string.Join("; ", result.Diagnostics));
            if (Path.GetFileName(fixture) == "complex-values.valid.cfd")
                Require(result.Data.Table<ComplexValues>()["sample"].optional.Value.score == 13, "inline class within Some");
            if (Path.GetFileName(fixture) == "record-references.valid.cfd")
                Require(ReferenceEquals(result.Data.Table<Node>()["a"].next.Value, result.Data.Table<Node>()["b"]), "shared typed reference");
            if (Path.GetFileName(fixture) == "migration-contract.valid.cfd")
            {
                var entry = result.Data.Table<ContractEntry>()["entry"];
                var shape = result.Data.Table<ContractNode>()["a"];
                Require(entry.payload.score == 2147483648L && entry.shape.count == 2147483648L, "class and struct inline values");
                Require(entry.payload.label == "score 2147483648" && entry.payload.labels[0] == "array 2147483648"
                    && entry.payload.indexed_labels["label"] == "dict 2147483648" && entry.payload.template.Value == "optional 2147483648", "inline data owns templates in scalar and collection fields");
                Require(entry.payload.nested.label == "details 42" && entry.shape.inner.Value.label == "shape 4", "nested data self does not use the outer owner");
                Require(entry.plain_text == "{self.id}" && shape.label == "node a" && entry.own_id == "entry" && entry.other_id == "entry", "virtual ids and explicit references use their record scope");
                Require(entry.wide == long.MinValue && entry.precise == 1.0000000000000002d && entry.large == 1e100d, "64-bit scalar values and defaults");
                Require(entry.scalar_text == "-9223372036854775808|1.0000000000000002|1" + new string('0', 100), "scalar text matches Rust interpolation");
                Require(entry.nested.IsSome && entry.nested.Value.IsNone && entry.some_nested.Value.Value == long.MaxValue, "nested option defaults");
                Require(entry.constant_nested.IsSome && entry.constant_nested.Value.IsNone, "nested option constants");
                Require(entry.samples[0] == 2147483648L && entry.samples[1] == long.MinValue
                    && entry.flags == (ContractFlags.A | ContractFlags.B) && entry.indexed[entry.flags] == 7, "compound constants and flag keys");
                Require(entry.shape.inner.Value.count == 4 && entry.shape.inner.Value.inner.IsNone, "finite recursive struct values");
                Require(entry.localized.Default.Value.Value == "base" && entry.localized.For("ja").Value.IsNone && entry.localized.For("en").IsNone, "distinct option layers in overlays");
                Require(ReferenceEquals(shape.next.Value.next.Value, shape) && ReferenceEquals(entry.target.Default, shape), "table references preserve cycles");
                Require(entry.polymorphic is ContractChild child && child.score == 7, "data subtype in class field");
                Require(entry.child.score == 8 && entry.child.name == "value", "inherited data fields");
            }
        }
        Console.WriteLine("Generated C# loading and shared runtime corpus passed.");
    }
}
