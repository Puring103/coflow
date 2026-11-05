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
        var catalog = loaded.Data;
        var data = catalog.Base;
        var collision = data.Table<NamedCollision>().Get(Ids.collision);
        Require(collision.types == Types.Normal && collision.constants == Constants.Normal
            && collision.table_value == Game.Config.Table.Normal && collision.load == Game.Config.Load.Normal
            && collision.singleton_value == Singleton.Normal, "generated helper names never shadow business enum types");
        Require(collision._coflowReader == 11 && collision._coflowReader_ == 12, "reader storage names never shadow business fields");
        Require(typeof(Item).GetProperty("name")!.GetSetMethod() == null
            && typeof(Stats).GetProperty("hp")!.GetSetMethod() == null,
            "generated class and struct properties have no public setters");
        var alpha = data.Table<Item>()["alpha"];
        var beta = data.Table<Item>()["beta"];
        Require(data.Table<@class>()["keyword"].@namespace == "public" && data.Table<@class>()["keyword"].state == @event.@public, "C# keywords are escaped in all generated declarations and accesses");
        Require(typeof(Stats).IsValueType, "Stats must be a struct");
        Require(catalog.ItemCount == 2 && !alpha.IsFree && alpha.stats.IsAlive, "partial class, struct and data access extensions");
        Require(data.Singleton<Settings>().StartId == "alpha", "partial singleton");
        Require(ReferenceEquals(data.Table<Item>(), data.Table<Item>()), "table access reuses registered table");
        Require(ReferenceEquals(data.Table<Item>().Get(EntityId.alpha), alpha), "enum id resolves to original identity");
        Require(data.Table<Item>().TryGet(EntityId.beta, out var enumBeta) && ReferenceEquals(enumBeta, beta), "typed enum lookup");
        Require(!data.Table<Item>().TryGet((EntityId)int.MaxValue, out _), "unknown enum value returns false");
        try { data.Table<Item>().Get((EntityId)int.MaxValue); throw new Exception("invalid enum must fail"); }
        catch (System.Collections.Generic.KeyNotFoundException) { }
        try { data.Table<Payload>(); throw new Exception("data must not register as table"); }
        catch (InvalidOperationException) { }
        try { data.Table<Settings>(); throw new Exception("singleton must not register as table"); }
        catch (InvalidOperationException) { }
        try { data.Singleton<Item>(); throw new Exception("table must not register as singleton"); }
        catch (InvalidOperationException) { }
        Require(ReferenceEquals(alpha.next.Value, beta) && ReferenceEquals(beta.next.Value, alpha), "circular references must preserve identity");
        Require(ReferenceEquals(data.Table<Entity>()["alpha"], alpha) && ReferenceEquals(data.Singleton<Settings>().start, alpha), "views and singleton references must share objects");
        Require(alpha.name == "Alpha" && catalog.Table<Item>()["alpha"].For("language", "ja").name == "アルファ", "dimension selection");
        try { catalog.Table<Item>()["alpha"].For("language", "unknown"); throw new Exception("unknown variant must fail"); }
        catch (ArgumentException error) when (error.ParamName == "variant") { }
        Require(catalog.Table<Item>()["alpha"].HasVariant(nameof(alpha.name), "ja") && !catalog.Table<Item>()["beta"].HasVariant(nameof(beta.name), "ja"), "explicit variant presence survives loading");
        Require(catalog.Table<Item>()["alpha"].For("language", "ja").label == "{item} アルファ: 12" && alpha.name == "Alpha", "template fallback is evaluated in an independent selected view");
        Require(ReferenceEquals(catalog.Table<Item>()["alpha"].For("language", "ja").next.Value.next.Value, catalog.Table<Item>()["alpha"].For("language", "ja")), "view references preserve cyclic identity");
        Require(ReferenceEquals(alpha.nums, catalog.Table<Item>()["alpha"].For("language", "ja").nums) && ReferenceEquals(alpha.indexed, catalog.Table<Item>()["alpha"].For("language", "ja").indexed), "fixed collections are shared across views");
        Require(alpha.enum_label == "Everything", "generated enum interpolation");
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
            if (Path.GetFileName(fixture) == "unicode-xid.valid.cfd")
            {
                // CFD 身份采用完整 XID；默认字符串跨生成、编译和加载后保持原值。
                foreach (var id in new[] { "℘", "𐐀key", "a·b" })
                    Require(result.Data.Base.Table<UnicodeDefaults>()[id].text == "a\u2028b\u2029c\u0085d", "Unicode identifiers and literal roundtrip");
            }
            if (Path.GetFileName(fixture) == "numeric-widths.valid.cfd")
            {
                var numbers = result.Data.Base.Table<NumericValues>()["numeric"];
                Require(numbers.small == int.MaxValue && numbers.wide == long.MaxValue, "declared integer widths");
                Require(numbers.rounded == 0.1f && numbers.precise == 0.1d && (double)numbers.rounded != numbers.precise, "declared floating point precision");
                Require(numbers.bits == NumericBits.Everything && (int)NumericBits.Everything == 9 && (int)NumericBits.None == 0, "built-in flag members");
                Require((long)numbers.code == 4294967296L, "int64 enum");
            }
            if (Path.GetFileName(fixture) == "complex-values.valid.cfd")
                Require(result.Data.Base.Table<ComplexValues>()["sample"].optional.Value.score == 13, "inline class within Some");
            if (Path.GetFileName(fixture) == "record-references.valid.cfd")
                Require(ReferenceEquals(result.Data.Base.Table<Node>()["a"].next.Value, result.Data.Base.Table<Node>()["b"]), "shared typed reference");
            if (Path.GetFileName(fixture) == "migration-contract.valid.cfd")
            {
                var entry = result.Data.Base.Table<ContractEntry>()["entry"];
                var shape = result.Data.Base.Table<ContractNode>()["a"];
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
                Require(entry.localized.Value.Value == "base" && result.Data.Table<ContractEntry>()["entry"].For("contract_language", "ja").localized.Value.IsNone && result.Data.Table<ContractEntry>()["entry"].For("contract_language", "en").localized.IsNone, "distinct option layers in overlays");
                Require(ReferenceEquals(shape.next.Value.next.Value, shape) && ReferenceEquals(entry.target, shape), "table references preserve cycles");
                Require(entry.polymorphic is ContractChild child && child.score == 7, "data subtype in class field");
                Require(entry.child.score == 8 && entry.child.name == "value", "inherited data fields");
            }
        }
        Require(default(Stats).hp == 0, "default struct has ordinary value fields");
        Require(typeof(Item).GetInterfaces().Length == 0, "business data has no loader interface");
        Require(ReferenceEquals(catalog.Table<Item>().Get(EntityId.alpha).Base, alpha), "dimension enum lookup shares base identity");
        Require(ReferenceEquals(catalog.For("language", "ja").Table<Item>()["alpha"], catalog.Table<Item>()["alpha"].For("language", "ja")), "catalog and record selection share objects");
        Console.WriteLine("Generated C# loading and shared runtime corpus passed.");
    }
}
