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
        internal Stats(int hp, string label) { Hp = hp; Label = label; }
        internal int Hp { get; }
        internal string Label { get; }
    }
    private sealed class Item
    {
        internal Item(string id) { Id = id; }
        internal string Id { get; }
        internal int Price { get; set; }
        internal Permission Permissions { get; set; }
        internal DimensionValue<string> Name { get; set; } = null!;
        internal DimensionValue<string?> Note { get; set; } = null!;
        internal Item? Next { get; set; }
        internal Stats Stats { get; set; }
        internal string Label { get; set; } = null!;
    }
    private sealed class Data
    {
        internal Data(CfdData data) { Items = data.Records<Item>(); }
        internal IReadOnlyDictionary<string, Item> Items { get; }
    }
    private static readonly CfdType[] Types =
    {
        new CfdType<Stats>("Stats", new CfdField[] {
            new CfdField<int>("hp", CfdValues.Int, CfdDefaults.Int(10)),
            new CfdField<string>("label", CfdValues.FormattedString, CfdDefaults.Template("f\"HP {self.hp}\"")),
        }, reader => new Stats(reader.Get<int>("hp"), reader.Get<string>("label"))),
        new CfdType<Item>("Item", new CfdField[] {
            new CfdField<int>("price", CfdValues.Int, CfdDefaults.Int(5)),
            new CfdField<Permission>("permissions", CfdValues.Enum("Permission", new Dictionary<string, Permission> {
                ["Read"] = Permission.Read, ["Write"] = Permission.Write, ["High"] = Permission.High,
            }, bits => (Permission)(uint)bits), CfdDefaults.Enum("Permission", 1)),
            new CfdField<DimensionValue<string>>("name", CfdValues.Dimension("language", CfdValues.String)),
            new CfdField<DimensionValue<string?>>("note", CfdValues.Dimension("language", CfdValues.OptionalReference(CfdValues.String)), CfdDefaults.String("base note")),
            new CfdField<Item?>("next", CfdValues.OptionalReference(CfdValues.Reference<Item>("Item"))),
            new CfdField<Stats>("stats", CfdValues.Object<Stats>("Stats"), CfdDefaults.Object("Stats")),
            new CfdField<string>("label", CfdValues.FormattedString, CfdDefaults.Template("f\"{self.id}: {self.name}, {self.stats.label}\"")),
        }, id => new Item(id), (item, reader) => {
            item.Permissions = reader.Get<Permission>("permissions");
            item.Price = reader.Get<int>("price"); item.Name = reader.Get<DimensionValue<string>>("name");
            item.Note = reader.Get<DimensionValue<string?>>("note"); item.Next = reader.Get<Item?>("next");
            item.Stats = reader.Get<Stats>("stats"); item.Label = reader.Get<string>("label");
        }),
    };
    private static LoadResult<Data> Load(params CfdSource[] sources) => CfdLoader.Load(sources, Types, reader => new Data(reader));

    [Fact]
    public void ConstructsGeneratedTypesAndLinksCyclicReferences()
    {
        var result = Load(new CfdSource("items.cfd", "a: Item { name: \"Alpha\", next: &Item::b } b: Item { name: \"Beta\", next: &Item::a }"));
        Assert.True(result.Success, result.Diagnostics.Count > 0 ? result.Diagnostics[0].ToString() : "");
        var a = result.Data.Items["a"];
        Assert.Same(a, a.Next!.Next);
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
            Assert.Equal("アルファ", a.Name.For("ja"));
            Assert.Equal("Alpha", a.Name.For("en"));
            Assert.Equal("Alpha", a.Name.For("unknown"));
            Assert.Null(a.Note.For("ja"));
            Assert.Equal("base note", a.Note.For("en"));
            Assert.Equal(new[] { "en", "ja" }, a.Name.Variants.Keys);
        }
    }

    [Fact]
    public void OrdinaryStringsStayLiteralAndFormattedStringsEscapeBraces()
    {
        var result = Load(new CfdSource("items.cfd", "a: Item { name: \"{self.price}\", label: f\"{{literal}} {self.price}\" }"));
        Assert.True(result.Success);
        Assert.Equal("{self.price}", result.Data.Items["a"].Name.Default);
        Assert.Equal("{literal} 5", result.Data.Items["a"].Label);
    }

    [Fact]
    public void TemplatesResolveOtherRecordsBeforeTheirFieldsAreAssigned()
    {
        var result = Load(new CfdSource("items.cfd", "a: Item { name: \"A\", label: f\"{&Item::b.stats.hp}\" } b: Item { name: \"B\", stats: Stats { hp: 25 } }"));
        Assert.True(result.Success);
        Assert.Equal("25", result.Data.Items["a"].Label);
    }

    [Theory]
    [InlineData("", false)]
    [InlineData("wrong: Singleton {}", false)]
    [InlineData("Singleton: Singleton {}", true)]
    public void SingletonRequiresOneRecordWithItsDeclaredName(string source, bool success)
    {
        var singleton = new CfdType<Item>("Singleton", Array.Empty<CfdField>(), id => new Item(id), (_, _) => { }, singleton: true);
        var result = CfdLoader.Load(new[] { new CfdSource("singleton.cfd", source) }, new CfdType[] { singleton }, data => new Data(data));
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
    [InlineData("a: Item { name: \"A\", price: 2147483648 }", "CFD-TYPE")]
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
