using System;
using System.Collections.Generic;
using Coflow.Loader;
using Xunit;

namespace Coflow.Loader.Tests;

public sealed class LoadingTests
{
    [Flags]
    private enum Permission : uint { Read = 1, Write = 2, High = 0x80000000 }
    private readonly struct Stats
    {
        internal Stats(long hp, string label) { Hp = hp; Label = label; }
        internal long Hp { get; }
        internal string Label { get; }
    }
    private sealed class Item : ICfdRecord
    {
        internal Item(string id) { Id = id; }
        public CfdReader Reader { get; set; } = null!;
        internal string Id { get; }
        internal long Price { get; set; }
        internal Permission Permissions { get; set; }
        internal string Name { get; set; } = null!;
        internal Option<string> Note { get; set; }
        internal Option<Item> Next { get; set; }
        internal Stats Stats { get; set; }
        internal string Label { get; set; } = null!;
        internal IReadOnlyList<string> Labels { get; set; } = null!;
        internal IReadOnlyDictionary<string, string> IndexedLabels { get; set; } = null!;
    }
    private sealed class Data
    {
        internal Data(CfdData data) { Items = data.Table<Item>(); }
        internal IReadOnlyDictionary<string, Item> Items { get; }
    }
    private static readonly CfdType[] Types =
    {
        new CfdType<Stats>("Stats", CfdTypeKind.Data, new CfdField[] {
            new CfdField<long>("hp", CfdValues.Int, CfdDefaults.Int(10)),
            new CfdField<string>("label", CfdValues.String, CfdDefaults.Template("f\"HP {self.hp}\"")),
        }, create: null, fill: null, construct:
        reader => new Stats(reader.Get<long>("hp"), reader.Get<string>("label"))),
        new CfdType<Item>("Item", CfdTypeKind.Table, new CfdField[] {
            new CfdField<long>("price", CfdValues.Dimension("balance", CfdValues.Int), CfdDefaults.Int(5)),
            new CfdField<Permission>("permissions", CfdValues.Enum("Permission", new Dictionary<string, Permission> {
                ["Read"] = Permission.Read, ["Write"] = Permission.Write, ["High"] = Permission.High,
            }, bits => (Permission)(uint)bits), CfdDefaults.Enum("Permission", 1)),
            new CfdField<string>("name", CfdValues.Dimension("language", CfdValues.String)),
            new CfdField<Option<string>>("note", CfdValues.Dimension("language", CfdValues.Optional(CfdValues.String)), CfdDefaults.String("base note")),
            new CfdField<Option<Item>>("next", CfdValues.Optional(CfdValues.Reference<Item>("Item")), CfdDefaults.None()),
            new CfdField<IReadOnlyList<string>>("labels", CfdValues.List(CfdValues.String), CfdDefaults.Array(CfdDefaults.Template("f\"{self.name}\""))),
            new CfdField<IReadOnlyDictionary<string, string>>("indexed", CfdValues.Dictionary(CfdValues.String, CfdValues.String), CfdDefaults.Dictionary(new CfdDefaultEntry(CfdDefaults.String("label"), CfdDefaults.Template("f\"{self.name}\"")))),
            new CfdField<Stats>("stats", CfdValues.Object<Stats>("Stats"), CfdDefaults.Object("Stats")),
            new CfdField<string>("label", CfdValues.String, CfdDefaults.Template("f\"{self.id}: {self.name}, {self.stats.label}\"")),
        }, id => new Item(id), (item, reader) => {
            item.Permissions = reader.Get<Permission>("permissions");
            item.Price = reader.Get<long>("price"); item.Name = reader.Get<string>("name");
            item.Note = reader.Get<Option<string>>("note"); item.Next = reader.Get<Option<Item>>("next");
            item.Labels = reader.Get<IReadOnlyList<string>>("labels");
            item.IndexedLabels = reader.Get<IReadOnlyDictionary<string, string>>("indexed");
            item.Stats = reader.Get<Stats>("stats"); item.Label = reader.Get<string>("label");
        }, construct: null),
    };
    private static LoadResult<Data> Load(params CfdSource[] sources) => CfdLoader.Load(sources, Types, reader => new Data(reader));

    [Fact]
    public void ConstructsGeneratedTypesAndLinksCyclicReferences()
    {
        var result = Load(new CfdSource("items.cfd", "a: Item { name: \"Alpha\", next: &Item::b } b: Item { name: \"Beta\", next: &Item::a }"));
        Assert.True(result.Success, result.Diagnostics.Count > 0 ? result.Diagnostics[0].ToString() : "");
        var a = result.Data.Items["a"];
        Assert.Same(a, a.Next.Value.Next.Value);
        Assert.Equal(5, a.Price);
        Assert.Equal(10, a.Stats.Hp);
        Assert.Equal("a: Alpha, HP 10", a.Label);
    }

    [Fact]
    public void CrossFilePartialOverridesAreOrderIndependentAndRetainExplicitNone()
    {
        var overlay = new CfdSource("ja.cfd", "language[ja] { a: Item { name: \"アルファ\", note: None } } language[en] { b: Item { name: \"Beta\" } }");
        var records = new CfdSource("items.cfd", "a: Item { name: \"Alpha\" } b: Item { name: \"B\" }");
        foreach (var result in new[] { Load(overlay, records), Load(records, overlay) })
        {
            Assert.True(result.Success);
            var a = result.Data.Items["a"];
            Assert.Equal("アルファ", a.For("language", "ja").Name);
            Assert.Equal("Alpha", a.For("language", "en").Name);
            Assert.Equal("Alpha", a.For("language", "unknown").Name);
            Assert.True(a.For("language", "ja").Note.IsNone);
            Assert.Equal("base note", a.For("language", "en").Note.Value);
            Assert.Equal(new[] { "en", "ja" }, a.Variants("language"));
        }
    }

    [Fact]
    public void IndependentViewsCarryContextsThroughTemplatesCollectionsAndReferences()
    {
        var result = Load(
            new CfdSource("base.cfd", "a: Item { name: \"A\", next: &Item::b, label: f\"{self.name}/{&Item::b.name}/{self.stats.label}/{self.price}\", stats: Stats { label: f\"{&Item::b.name}\" } } b: Item { name: \"B\", next: &Item::a }"),
            new CfdSource("ja.cfd", "language[ja] { a: Item { name: \"甲\", note: None } b: Item { name: \"乙\" } } balance[ja] { a: Item { price: 99 } }"));
        Assert.True(result.Success, string.Join("; ", result.Diagnostics));
        var a = result.Data.Items["a"];
        var ja = a.For("language", "ja");
        var balanced = ja.For("balance", "ja");
        Assert.Equal("A/B/B/5", a.Label);
        Assert.Equal("甲/乙/乙/5", ja.Label);
        Assert.Equal("甲/乙/乙/99", balanced.Label);
        Assert.Equal("A/B/B/99", a.For("balance", "ja").Label);
        Assert.Equal("甲", ja.Labels[0]);
        Assert.Equal("甲", ja.IndexedLabels["label"]);
        Assert.Equal("乙", ja.Next.Value.Name);
        Assert.Same(ja, ja.Next.Value.Next.Value);
        Assert.Same(balanced, a.For("balance", "ja").For("language", "ja"));
        Assert.NotSame(a, ja);
        Assert.Equal("A", a.Name);
    }

    [System.Runtime.CompilerServices.MethodImpl(System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    private static (LoadResult<Data> Result, List<WeakReference<Item>> Instances) LoadTrackedVariants()
    {
        var instances = new List<WeakReference<Item>>();
        var type = new CfdType<Item>("Item", CfdTypeKind.Table, new CfdField[] {
            new CfdField<string>("name", CfdValues.Dimension("language", CfdValues.String)),
            new CfdField<Option<Item>>("next", CfdValues.Optional(CfdValues.Reference<Item>("Item"))),
        }, id => {
            var item = new Item(id);
            instances.Add(new WeakReference<Item>(item));
            return item;
        }, (item, reader) => {
            item.Name = reader.Get<string>("name");
            item.Next = reader.Get<Option<Item>>("next");
        }, construct: null);
        var result = CfdLoader.Load(new[] { new CfdSource("items.cfd",
            "a: Item { name: \"A\", next: &Item::b } b: Item { name: \"B\", next: &Item::a } " +
            "language[ja] { a: Item { name: \"甲\" } b: Item { name: \"乙\" } } " +
            "language[fr] { a: Item { name: \"Un\" } }") }, new CfdType[] { type }, data => new Data(data));
        return (result, instances);
    }

    [Fact]
    public void ValidationOnlyViewsAreCollectibleAndRequestedViewsKeepReferenceIdentity()
    {
        var (result, instances) = LoadTrackedVariants();
        Assert.True(result.Success, string.Join("; ", result.Diagnostics));
        Assert.Equal(6, instances.Count);
        // 使用弱引用验证释放行为，不读取私有缓存，也不依赖具体分配字节数。
        GC.Collect();
        GC.WaitForPendingFinalizers();
        GC.Collect();
        Assert.Equal(2, instances.FindAll(reference => reference.TryGetTarget(out _)).Count);

        var a = result.Data.Items["a"];
        var ja = a.For("language", "ja");
        Assert.Equal("甲", ja.Name);
        Assert.Equal("乙", ja.Next.Value.Name);
        Assert.Same(ja, ja.Next.Value.Next.Value);
        Assert.Same(ja, a.For("language", "ja"));
        Assert.Equal(8, instances.Count);
        GC.KeepAlive(result);
    }

    [Fact]
    public void ExplicitVariantPresenceIsIndependentOfValueAndFallback()
    {
        var result = Load(new CfdSource("base.cfd", "a: Item { name: \"A\" } b: Item { name: \"B\" }"),
            new CfdSource("ja.cfd", "language[ja] { a: Item { name: \"A\", note: None } }"));
        Assert.True(result.Success);
        var a = result.Data.Items["a"];
        Assert.True(a.HasVariant("name", "ja"));
        Assert.True(a.HasVariant("note", "ja"));
        Assert.False(result.Data.Items["b"].HasVariant("name", "ja"));
        Assert.False(a.HasVariant("name", "unknown"));
        Assert.True(a.For("language", "ja").HasVariant("name", "ja"));
        Assert.Throws<ArgumentException>(() => a.HasVariant("label", "ja"));
        Assert.Throws<ArgumentException>(() => a.For("missing", "ja"));
    }

    [Theory]
    [InlineData("a: Item {}", false, false)]
    [InlineData("a: Item { note: None }", false, true)]
    [InlineData("a: Item {}", true, true)]
    public void NullableFieldsRequireAnExplicitDefaultBeforeTheyMayBeOmitted(string source, bool hasDefault, bool success)
    {
        var type = new CfdType<Item>("Item", CfdTypeKind.Table, new CfdField[] {
            new CfdField<Option<string>>("note", CfdValues.Optional(CfdValues.String), hasDefault ? CfdDefaults.None() : null),
        }, id => new Item(id), (item, reader) => item.Note = reader.Get<Option<string>>("note"), construct: null);
        var result = CfdLoader.Load(new[] { new CfdSource("optional.cfd", source) }, new CfdType[] { type }, data => new Data(data));
        Assert.Equal(success, result.Success);
        if (!success) Assert.Equal("CFD-MISSING", result.Diagnostics[0].Code);
        else Assert.True(result.Data.Items["a"].Note.IsNone);
    }

    [Fact]
    public void VariantOnlyInterpolationCyclesAreReportedAtLoadTime()
    {
        var result = Load(new CfdSource("base.cfd", "a: Item { name: \"A\" }"),
            new CfdSource("cycle.cfd", "language[ja] { a: Item { name: f\"{self.label}\" } }"));
        Assert.False(result.Success);
        Assert.Equal("CFD-INTERPOLATION-CYCLE", result.Diagnostics[0].Code);
    }

    [Fact]
    public void OrdinaryStringsStayLiteralAndFormattedStringsEscapeBraces()
    {
        var result = Load(new CfdSource("items.cfd", "a: Item { name: \"{self.price}\", label: f\"{{literal}} {self.price}\" }"));
        Assert.True(result.Success);
        Assert.Equal("{self.price}", result.Data.Items["a"].Name);
        Assert.Equal("{literal} 5", result.Data.Items["a"].Label);
    }

    [Fact]
    public void TemplatesResolveOtherRecordsBeforeTheirFieldsAreAssigned()
    {
        var result = Load(new CfdSource("items.cfd", "a: Item { name: \"A\", label: f\"{&Item::b.stats.hp}\" } b: Item { name: \"B\", stats: Stats { hp: 25 } }"));
        Assert.True(result.Success);
        Assert.Equal("25", result.Data.Items["a"].Label);
    }


    private sealed class VariantCycleRecord : ICfdRecord
    {
        public CfdReader Reader { get; set; } = null!;
        internal Option<VariantCycleRecord> Next { get; set; }
        internal string First { get; set; } = null!;
        internal string Second { get; set; } = null!;
    }

    [Fact]
    public void FailedCombinedViewsNeverExposePartiallyBuiltCircularNeighbors()
    {
        var type = new CfdType<VariantCycleRecord>("Node", CfdTypeKind.Table, new CfdField[] {
            new CfdField<Option<VariantCycleRecord>>("next", CfdValues.Optional(CfdValues.Reference<VariantCycleRecord>("Node"))),
            new CfdField<string>("first", CfdValues.Dimension("one", CfdValues.String)),
            new CfdField<string>("second", CfdValues.Dimension("two", CfdValues.String)),
        }, _ => new VariantCycleRecord(), (node, reader) => {
            node.Next = reader.Get<Option<VariantCycleRecord>>("next");
            node.First = reader.Get<string>("first");
            node.Second = reader.Get<string>("second");
        }, construct: null);
        var result = CfdLoader.Load(new[] { new CfdSource("nodes.cfd", "a: Node { next: &Node::b, first: \"A\", second: \"A\" } b: Node { next: &Node::a, first: \"B\", second: \"B\" } one[v] { a: Node { first: f\"{self.second}\" } } two[v] { a: Node { second: f\"{self.first}\" } }") }, new CfdType[] { type }, data => data.Table<VariantCycleRecord>());
        Assert.True(result.Success, string.Join("; ", result.Diagnostics));
        var a = result.Data["a"];
        var b = result.Data["b"];
        Assert.Throws<CfdLoadException>(() => a.For("one", "v").For("two", "v"));
        Assert.Throws<CfdLoadException>(() => b.For("one", "v").For("two", "v"));
        Assert.Same(a, b.Next.Value);
        Assert.Equal("A", a.First);
    }

    [Theory]
    [InlineData("", false)]
    [InlineData("wrong: Singleton {}", false)]
    [InlineData("Singleton: Singleton {}", true)]
    public void SingletonRequiresOneRecordWithItsDeclaredName(string source, bool success)
    {
        var singleton = new CfdType<Item>("Singleton", CfdTypeKind.Singleton, Array.Empty<CfdField>(), id => new Item(id), (_, _) => { }, construct: null);
        var result = CfdLoader.Load(new[] { new CfdSource("singleton.cfd", source) }, new CfdType[] { singleton }, data => new Tuple<Item>(data.Singleton<Item>()));
        Assert.Equal(success, result.Success);
        if (!success) Assert.Equal("CFD-SINGLETON", result.Diagnostics[0].Code);
        if (source.StartsWith("wrong", StringComparison.Ordinal)) Assert.Equal("singleton.cfd", result.Diagnostics[0].Path);
    }

    [Theory]
    [InlineData("Read", "Read")]
    [InlineData("Read | Write", "3")]
    [InlineData("High", "High")]
    public void EnumInterpolationUsesDeclaredNamesAndNumericCombinationValues(string literal, string expected)
    {
        var result = Load(new CfdSource("enum.cfd", "a: Item { name: \"A\", permissions: " + literal + ", label: f\"{self.permissions}\" }"));
        Assert.True(result.Success);
        Assert.Equal(expected, result.Data.Items["a"].Label);
    }

    [Theory]
    [InlineData("note")]
    [InlineData("stats")]
    [InlineData("next.price")]
    public void TemplatesRejectOptionalAndCompositeTerminals(string path)
    {
        var result = Load(new CfdSource("optional.cfd", "a: Item { name: \"A\", next: &Item::a, label: f\"{self." + path + "}\" }"));
        Assert.False(result.Success);
        Assert.Equal("CFD-INTERPOLATION", result.Diagnostics[0].Code);
    }

    [Theory]
    [InlineData("a: Item { name: \"A\", price: 9223372036854775808 }", "CFD-TYPE")]
    [InlineData("a: Item { price: 1 }", "CFD-MISSING")]
    [InlineData("a: Item { name: \"A\", unexpected: 1 }", "CFD-FIELD")]
    [InlineData("a: Item { name: \"A\", next: &Item::missing }", "CFD-REFERENCE")]
    [InlineData("a: Item { name: \"A\", label: f\"{self.label}\" }", "CFD-INTERPOLATION-CYCLE")]
    [InlineData("a: Item { name: \"A\", label: f\"{self.price + 1}\" }", "CFD-SYNTAX-013")]
    [InlineData("a: Item { name: \"A\", label: f\"{self.price:000}\" }", "CFD-SYNTAX-013")]
    [InlineData("a: Item { name: \"A\", label: f\"}\" }", "CFD-SYNTAX-013")]
    [InlineData("a: Item { name: \"A\", label: fn() { 1 } }", "CFD-SYNTAX-009")]
    public void FailedLoadsExposeDiagnosticsAndNeverPartialData(string source, string code)
    {
        var result = Load(new CfdSource("bad.cfd", source));
        Assert.False(result.Success);
        Assert.Equal(code, result.Diagnostics[0].Code);
        Assert.Equal("bad.cfd", result.Diagnostics[0].Path);
        Assert.Throws<InvalidOperationException>(() => result.Data);
    }

    [Theory]
    [InlineData("unknown[ja] { a: Item { name: \"A\" } }", "CFD-DIMENSION")]
    [InlineData("language[ja] { a: Item { price: 1 } }", "CFD-DIMENSION")]
    [InlineData("language[ja] { missing: Item { name: \"A\" } }", "CFD-REFERENCE")]
    [InlineData("language[ja] { a: Item { name: \"A\" } } language[ja] { a: Item { name: \"B\" } }", "CFD-DUPLICATE-OVERLAY")]
    public void InvalidOverlaysReportTheirOwnSource(string source, string code)
    {
        var result = Load(new CfdSource("base.cfd", "a: Item { name: \"base\" }"), new CfdSource("bad-overlay.cfd", source));
        Assert.False(result.Success);
        Assert.Equal(code, result.Diagnostics[0].Code);
        Assert.Equal("bad-overlay.cfd", result.Diagnostics[0].Path);
    }
}
