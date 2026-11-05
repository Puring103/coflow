using System;
using System.Collections.Generic;
using System.Linq;
using Xunit;

namespace Coflow.Loader.Tests;

public sealed class LoadingTests
{
    internal sealed class Item
    {
        internal string Id = "";
        internal string Name = "";
        internal int Price;
        internal string Label = "";
        internal Option<Item> Next;
        internal Option<string> Note;
        internal IReadOnlyList<string> Labels = null!;
        internal IReadOnlyDictionary<string, string> Map = null!;
        internal IReadOnlyList<Item> Links = null!;
        internal Stats Stats;
    }
    internal struct Stats { internal int Hp; internal string Label; }
    internal static CfdType[] Types(Action? created = null) => new CfdType[] {
        new CfdType<Stats>("Stats", CfdTypeKind.Data, new CfdField[] {
            new CfdField<int>("hp", CfdValues.Int, CfdDefaults.Int(10)),
            new CfdField<string>("label", CfdValues.String, CfdDefaults.Template("f\"HP {self.hp}\"")),
        }, _ => new Stats(), (value, reader) => { value.Hp = reader.Get<int>(0); value.Label = reader.Get<string>(1); return value; }),
        new CfdType<Item>("Item", CfdTypeKind.Table, new CfdField[] {
            new CfdField<string>("name", CfdValues.String, dimension: "language"),
            new CfdField<int>("price", CfdValues.Int, CfdDefaults.Int(5), dimension: "balance"),
            new CfdField<string>("label", CfdValues.String, CfdDefaults.Template("f\"{self.id}: {self.name}/{self.price}/{self.stats.label}\"")),
            new CfdField<Option<Item>>("next", CfdValues.Optional(CfdValues.Reference<Item>("Item")), CfdDefaults.None()),
            new CfdField<Option<string>>("note", CfdValues.Optional(CfdValues.String), CfdDefaults.String("note"), dimension: "language"),
            new CfdField<IReadOnlyList<string>>("labels", CfdValues.List(CfdValues.String), CfdDefaults.Array(CfdDefaults.Template("f\"{self.name}\""))),
            new CfdField<IReadOnlyDictionary<string,string>>("map", CfdValues.Dictionary(CfdValues.String,CfdValues.String), CfdDefaults.Dictionary(new CfdDefaultEntry(CfdDefaults.String("key"),CfdDefaults.Template("f\"{self.name}\"")))),
            new CfdField<IReadOnlyList<Item>>("links", CfdValues.List(CfdValues.Reference<Item>("Item")), CfdDefaults.Array()),
            new CfdField<Stats>("stats", CfdValues.Object<Stats>("Stats"), CfdDefaults.Object("Stats")),
        }, id => { created?.Invoke(); return new Item { Id = id }; }, (value, reader) => {
            value.Name=reader.Get<string>(0); value.Price=reader.Get<int>(1); value.Label=reader.Get<string>(2);
            value.Next=reader.Get<Option<Item>>(3); value.Note=reader.Get<Option<string>>(4);
            value.Labels=reader.Get<IReadOnlyList<string>>(5); value.Map=reader.Get<IReadOnlyDictionary<string,string>>(6);
            value.Links=reader.Get<IReadOnlyList<Item>>(7); value.Stats=reader.Get<Stats>(8); return value;
        })
    };
    internal static LoadResult<CfdData> Load(string source, LoadOptions? options = null, Action? created = null)
        => CfdLoader.Load(new[]{new CfdSource("items.cfd",source)}, Types(created), data=>data, options:options);
    internal const string Source = "a: Item { name: \"A\", next: &Item::b, links: [&Item::b], stats: Stats { label: f\"{&Item::b.name}\" } } b: Item { name: \"B\", next: &Item::a } language[ja] { a: Item { name: \"甲\", note: None } b: Item { name: \"乙\" } } language[en] {} balance[hard] { a: Item { price: 99 } }";

    [Fact]
    public void LoadBuildsEveryRequestedGraphBeforeReturning()
    {
        int created=0;
        var combination=new DimensionSelection(("language","ja"),("balance","hard"));
        var result=Load(Source,new LoadOptions { Combinations=new[]{combination, new DimensionSelection(("balance","hard"),("language","ja"))} },()=>created++);
        Assert.True(result.Success,string.Join(";",result.Diagnostics));
        Assert.Equal(10,created); // 基础、三个单维变体、一个去重后的组合，各两条记录。
        var a=result.Data.Table<Item>()["a"];
        Assert.Equal("a: A/5/B",a.Base.Label);
        var ja=a.For("language","ja"); var combined=a.For(combination);
        Assert.Equal("a: 甲/5/乙",ja.Label); Assert.Equal("a: 甲/99/乙",combined.Label);
        Assert.Equal("甲",ja.Labels[0]); Assert.Equal("甲",ja.Map["key"]); Assert.Equal("乙",ja.Links[0].Name);
        Assert.Same(ja,ja.Next.Value.Next.Value); Assert.Same(ja.Next.Value,ja.Links[0]);
        Assert.True(ja.Note.IsNone); Assert.Equal("note",a.Base.Note.Value);
        Assert.Same(ja,result.Data.For("language","ja").Table<Item>()["a"]);
        Assert.Same(combined,result.Data.For(("balance","hard"),("language","ja")).Table<Item>()["a"]);
        Assert.Same(combined,a.For(("balance","hard"),("language","ja")));
        Assert.Same(a.Base,result.Data.Base.Table<Item>()["a"]);
        Assert.Equal(10,created);
    }

    [Fact]
    public void SelectionIsStrictAndNeverBuildsMissingCombinations()
    {
        var result=Load(Source); Assert.True(result.Success);
        var a=result.Data.Table<Item>()["a"];
        Assert.Equal("A",a.For("language","en").Name);
        Assert.Contains("[en, ja]",Assert.Throws<ArgumentException>(()=>a.For("language","missing")).Message);
        Assert.Throws<ArgumentException>(()=>a.For("missing","ja"));
        Assert.Contains("not built",Assert.Throws<ArgumentException>(()=>a.For(("language","ja"),("balance","hard"))).Message);
        Assert.False(a.TryFor(new DimensionSelection(("language","ja"),("balance","hard")),out _));
        Assert.False(a.TryFor("language","JA",out _)); Assert.True(a.TryFor("language","ja",out var ja));
        Assert.Same(ja,a.For("language","ja")); Assert.Same(a.Base,a.For(DimensionSelection.Empty));
        Assert.Equal(new[]{"en","ja"},a.Variants("language"));
        Assert.True(a.HasVariant("name","ja")); Assert.False(a.HasVariant("name","en"));
        Assert.False(a.HasVariant("name","unknown")); Assert.Throws<ArgumentException>(()=>a.HasVariant("label","ja"));
        Assert.Throws<ArgumentException>(()=>new DimensionSelection(("language","ja"),("language","en")));
        Assert.Throws<ArgumentException>(()=>Load(Source,new LoadOptions { Combinations=new[]{new DimensionSelection(("language","unknown"))} }));
    }

    [Fact]
    public void CrossFileOverridesAreOrderIndependentAndExplicitValuesAreTracked()
    {
        var baseSource=new CfdSource("base.cfd","a: Item { name: \"A\" } b: Item { name: \"B\" }");
        var overlay=new CfdSource("ja.cfd","language[ja] { a: Item { name: \"A\", note: None } } language[en] {}");
        foreach(var sources in new[]{new[]{baseSource,overlay},new[]{overlay,baseSource}})
        {
            var result=CfdLoader.Load(sources,Types(),data=>data); Assert.True(result.Success);
            var a=result.Data.Table<Item>()["a"]; Assert.True(a.HasVariant("name","ja"));
            Assert.False(result.Data.Table<Item>()["b"].HasVariant("name","ja"));
            Assert.True(a.For("language","ja").Note.IsNone); Assert.Equal("note",a.For("language","en").Note.Value);
        }
    }

    [Theory]
    [InlineData("a: Item {}", "CFD-MISSING")]
    [InlineData("a: Item { name: 1 }", "CFD-TYPE")]
    [InlineData("a: Item { name: \"A\", missing: 1 }", "CFD-FIELD")]
    [InlineData("a: Item { name: \"A\", next: &Item::missing }", "CFD-REFERENCE")]
    [InlineData("a: Item { name: \"A\", label: f\"{self.label}\" }", "CFD-INTERPOLATION-CYCLE")]
    [InlineData("a: Item { name: \"A\", label: f\"{self.next.name}\" }", "CFD-INTERPOLATION")]
    [InlineData("a: Item { name: \"A\", label: f\"{self.stats}\" }", "CFD-INTERPOLATION")]
    [InlineData("a: Item { name: \"A\", label: f\"{self.note}\" }", "CFD-INTERPOLATION")]
    [InlineData("a: Item { name: \"A\", label: f\"{self.missing}\" }", "CFD-FIELD")]
    [InlineData("a: Item { name: \"A\", map: { \"x\": \"1\", \"x\": \"2\" } }", "CFD-DUPLICATE")]
    [InlineData("a: Item { name: \"A\" } unknown[x] {}", "CFD-DIMENSION")]
    [InlineData("a: Item { name: \"A\" } language[ja] { a: Item { price: 1 } }", "CFD-DIMENSION")]
    [InlineData("a: Item { name: \"A\" } language[ja] { a: Item { name: \"X\" } a: Item { name: \"Y\" } }", "CFD-DUPLICATE-OVERLAY")]
    public void InvalidDataFailsBeforePublication(string source,string code)
    {
        var result=Load(source); Assert.False(result.Success); Assert.Equal(code,result.Diagnostics[0].Code);
        Assert.Equal("items.cfd",result.Diagnostics[0].Path); Assert.Throws<InvalidOperationException>(()=>result.Data);
    }

    [Fact]
    public void TemplatesResolveForwardReferencesAndKeepLiteralStrings()
    {
        var result=Load("a: Item { name: \"{literal}\", label: f\"{{literal}} {&Item::b.name}\" } b: Item { name: \"B\" }");
        Assert.True(result.Success); Assert.Equal("{literal}",result.Data.Base.Table<Item>()["a"].Name);
        Assert.Equal("{literal} B",result.Data.Base.Table<Item>()["a"].Label);
    }
}
