using System;
using System.Collections.Generic;
using Coflow.Loader;
using Xunit;

namespace Coflow.Loader.Tests;

public sealed class LoadingPlanTests
{
    private sealed class Node : ICfdRecord
    {
        public CfdReader Reader { get; set; } = null!;
        internal Option<Node> Next { get; set; }
        internal string First { get; set; } = null!;
        internal string Second { get; set; } = null!;
        internal string Safe { get; set; } = null!;
        internal string Label { get; set; } = null!;
    }

    private static CfdType NodeType() => new CfdType<Node>("Node", CfdTypeKind.Table, new CfdField[] {
        new CfdField<Option<Node>>("next", CfdValues.Optional(CfdValues.Reference<Node>("Node")), CfdDefaults.None()),
        new CfdField<string>("first", CfdValues.Dimension("one", CfdValues.String), CfdDefaults.String("first")),
        new CfdField<string>("second", CfdValues.Dimension("two", CfdValues.String), CfdDefaults.String("second")),
        new CfdField<string>("safe", CfdValues.String, CfdDefaults.String("safe")),
        new CfdField<string>("label", CfdValues.String, CfdDefaults.String("label")),
    }, _ => new Node(), (node, reader) => {
        node.Next = reader.Get<Option<Node>>(0);
        node.First = reader.Get<string>(1);
        node.Second = reader.Get<string>(2);
        node.Safe = reader.Get<string>(3);
        node.Label = reader.Get<string>(4);
    }, construct: null);

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public void FailedCombinationRollsBackUnpublishedGraphAndPreservesPublishedIdentity(bool templateVisitedFailure)
    {
        var source = "a: Node { next: &Node::b } b: Node { next: &Node::a } " +
            "healthy: Node { next: &Node::friend, label: " + (templateVisitedFailure ? "f\"{&Node::a.safe}\"" : "\"healthy\"") + " } " +
            "friend: Node { next: &Node::healthy } " +
            "one[v] { a: Node { first: f\"{self.second}\" } } " +
            "two[v] { a: Node { second: f\"{self.first}\" } }";
        var result = CfdLoader.Load(new[] { new CfdSource("nodes.cfd", source) }, new[] { NodeType() }, data => data.Table<Node>());
        Assert.True(result.Success, string.Join("; ", result.Diagnostics));
        var healthy = result.Data["healthy"];
        var published = healthy.For("one", "v").For("two", "v");
        Assert.Same(published, published.Next.Value.Next.Value);
        Assert.Same(published, healthy.For("two", "v").For("one", "v"));

        // 失败对象可先被模板预读或由循环引用提前填充；两者都不能从后续读取泄漏。
        for (var attempt = 0; attempt < 2; attempt++)
        {
            var error = Assert.Throws<CfdLoadException>(() => result.Data["a"].For("one", "v").For("two", "v"));
            Assert.Equal("CFD-INTERPOLATION-CYCLE", error.Diagnostics[0].Code);
            Assert.Throws<CfdLoadException>(() => result.Data["b"].For("two", "v").For("one", "v"));
            Assert.Same(published, healthy.For("one", "v").For("two", "v"));
            Assert.Same(published, published.For("one", "v"));
            Assert.Same(published.Next.Value, result.Data["friend"].For("one", "v").For("two", "v"));
        }
        Assert.Equal(templateVisitedFailure ? "safe" : "healthy", published.Label);
    }

    private class Detail
    {
        internal Detail(string label) => Label = label;
        internal string Label { get; }
    }
    private sealed class RichDetail : Detail
    {
        internal RichDetail(string label, int count) : base(label) => Count = count;
        internal int Count { get; }
    }
    private sealed class CollectionRow : ICfdRecord
    {
        public CfdReader Reader { get; set; } = null!;
        internal string Name { get; set; } = null!;
        internal Detail Detail { get; set; } = null!;
        internal Option<Detail> Optional { get; set; }
        internal IReadOnlyList<Detail> Items { get; set; } = null!;
        internal IReadOnlyDictionary<string, Detail> Map { get; set; } = null!;
        internal Option<CollectionRow> Next { get; set; }
    }

    [Fact]
    public void StrictVariantValidationReusesReadersWithoutConstructingRecordsOrNestedBusinessValues()
    {
        var recordsCreated = 0;
        var detailsCreated = 0;
        var baseFields = new CfdField[] { new CfdField<string>("label", CfdValues.String, CfdDefaults.Template("f\"{&Row::row.name}\"")) };
        var types = new CfdType[] {
            new CfdType<Detail>("Detail", CfdTypeKind.Data, baseFields, null, null, null, abstractType: true),
            new CfdType<RichDetail>("RichDetail", CfdTypeKind.Data, new CfdField[] {
                baseFields[0], new CfdField<int>("count", CfdValues.Int, CfdDefaults.Scalar("DefaultCount")),
            }, null, null, reader => { detailsCreated++; return new RichDetail(reader.Get<string>(0), reader.Get<int>(1)); }, parent: "Detail"),
            new CfdType<CollectionRow>("Row", CfdTypeKind.Table, new CfdField[] {
                new CfdField<string>("name", CfdValues.Dimension("language", CfdValues.String)),
                new CfdField<Detail>("detail", CfdValues.Object<Detail>("Detail")),
                new CfdField<Option<Detail>>("optional", CfdValues.Optional(CfdValues.Object<Detail>("Detail"))),
                new CfdField<IReadOnlyList<Detail>>("items", CfdValues.List(CfdValues.Object<Detail>("Detail"))),
                new CfdField<IReadOnlyDictionary<string, Detail>>("map", CfdValues.Dictionary(CfdValues.String, CfdValues.Object<Detail>("Detail"))),
                new CfdField<Option<CollectionRow>>("next", CfdValues.Optional(CfdValues.Reference<CollectionRow>("Row"))),
            }, _ => { recordsCreated++; return new CollectionRow(); }, (row, reader) => {
                row.Name = reader.Get<string>(0); row.Detail = reader.Get<Detail>(1); row.Optional = reader.Get<Option<Detail>>(2);
                row.Items = reader.Get<IReadOnlyList<Detail>>(3); row.Map = reader.Get<IReadOnlyDictionary<string, Detail>>(4); row.Next = reader.Get<Option<CollectionRow>>(5);
            }, null),
        };
        var source = "row: Row { name: \"base\", detail: RichDetail {}, optional: Some(RichDetail {}), " +
            "items: [RichDetail {}], map: { \"key\": RichDetail {} }, next: &Row::row } " +
            "language[ja] { row: Row { name: \"日本語\" } } language[en] {} language[fr] {}";
        var result = CfdLoader.Load(new[] { new CfdSource("collections.cfd", source) }, types, data => data.Table<CollectionRow>(),
            new Dictionary<string, CfdValueNode> { ["DefaultCount"] = CfdDefaults.Int(7) });
        Assert.True(result.Success, string.Join("; ", result.Diagnostics));
        Assert.Equal(1, recordsCreated);
        Assert.Equal(4, detailsCreated);
        var baseRow = result.Data["row"];
        Assert.Equal("base", baseRow.Detail.Label);
        Assert.Equal(7, Assert.IsType<RichDetail>(baseRow.Detail).Count);

        var view = baseRow.For("language", "ja");
        Assert.Equal(2, recordsCreated);
        Assert.Equal(8, detailsCreated);
        Assert.Same(view, view.Next.Value);
        Assert.Equal("日本語", view.Detail.Label);
        Assert.Equal("日本語", view.Optional.Value.Label);
        Assert.Equal("日本語", view.Items[0].Label);
        Assert.Equal("日本語", view.Map["key"].Label);
    }

    private sealed class CheckedRow : ICfdRecord
    {
        public CfdReader Reader { get; set; } = null!;
    }
    private static CfdType[] CheckedTypes() => new CfdType[] {
        new CfdType<RichDetail>("Detail", CfdTypeKind.Data, new CfdField[] {
            new CfdField<int>("count", CfdValues.Int),
        }, null, null, reader => new RichDetail("", reader.Get<int>(0))),
        new CfdType<CheckedRow>("Row", CfdTypeKind.Table, new CfdField[] {
            new CfdField<Option<IReadOnlyList<Option<int>>>>("optional", CfdValues.Dimension("language", CfdValues.Optional(CfdValues.List(CfdValues.Optional(CfdValues.Int)))), CfdDefaults.None()),
            new CfdField<IReadOnlyDictionary<string, Option<int>>>("map", CfdValues.Dimension("language", CfdValues.Dictionary(CfdValues.String, CfdValues.Optional(CfdValues.Int))), CfdDefaults.Dictionary()),
            new CfdField<RichDetail>("detail", CfdValues.Dimension("language", CfdValues.Object<RichDetail>("Detail")), CfdDefaults.Object("Detail", new CfdDefaultField("count", CfdDefaults.Int(1)))),
            new CfdField<Option<CheckedRow>>("reference", CfdValues.Dimension("language", CfdValues.Optional(CfdValues.Reference<CheckedRow>("Row"))), CfdDefaults.None()),
        }, _ => new CheckedRow(), (_, _) => { }, null),
    };

    [Theory]
    [InlineData("optional: Some([Some(\"bad\")])", "CFD-TYPE")]
    [InlineData("optional: Some(123)", "CFD-TYPE")]
    [InlineData("map: { \"same\": None, \"same\": Some(2) }", "CFD-DUPLICATE")]
    [InlineData("map: { \"key\": Some(\"bad\") }", "CFD-TYPE")]
    [InlineData("detail: Detail { count: false }", "CFD-TYPE")]
    [InlineData("detail: { count: 1, count: 2 }", "CFD-DUPLICATE-FIELD")]
    [InlineData("reference: Some(&Row::missing)", "CFD-REFERENCE")]
    [InlineData("reference: Some(1)", "CFD-TYPE")]
    public void InvalidNestedVariantValuesStillFailAtLoadTimeWithOverlaySource(string field, string code)
    {
        var result = CfdLoader.Load(new[] {
            new CfdSource("base.cfd", "row: Row {}"),
            new CfdSource("invalid-overlay.cfd", "language[ja] { row: Row { " + field + " } }"),
        }, CheckedTypes(), data => data.Table<CheckedRow>());
        Assert.False(result.Success);
        Assert.Equal(code, result.Diagnostics[0].Code);
        Assert.Equal("invalid-overlay.cfd", result.Diagnostics[0].Path);
    }

    [Fact]
    public void CrossRecordTemplateErrorsKeepTheTemplateOverlaySource()
    {
        var result = CfdLoader.Load(new[] {
            new CfdSource("base.cfd", "a: Node {} healthy: Node {}"),
            new CfdSource("template-overlay.cfd", "one[v] { healthy: Node { first: f\"{&Node::a.next.safe}\" } }"),
        }, new[] { NodeType() }, data => data.Table<Node>());
        Assert.False(result.Success);
        Assert.Equal("CFD-INTERPOLATION", result.Diagnostics[0].Code);
        Assert.Equal("template-overlay.cfd", result.Diagnostics[0].Path);
    }

    [Fact]
    public void OrdinalsBelongToTheContainingTypeAndDataIdRemainsAnOrdinaryField()
    {
        var shared = new CfdField<int>("number", CfdValues.Int);
        var first = new CfdType<Tuple<int, string>>("First", CfdTypeKind.Data,
            new CfdField[] { shared, new CfdField<string>("id", CfdValues.String) }, null, null,
            reader => Tuple.Create(reader.Get<int>(0), reader.Get<string>(1)));
        var second = new CfdType<Tuple<string, int>>("Second", CfdTypeKind.Data,
            new CfdField[] { new CfdField<string>("id", CfdValues.String), shared }, null, null,
            reader => Tuple.Create(reader.Get<string>(0), reader.Get<int>(1)));
        var values = new List<object>();
        var row = new CfdType<CheckedRow>("Row", CfdTypeKind.Table, new CfdField[] {
            new CfdField<Tuple<int, string>>("first", CfdValues.Object<Tuple<int, string>>("First")),
            new CfdField<Tuple<string, int>>("second", CfdValues.Object<Tuple<string, int>>("Second")),
        }, _ => new CheckedRow(), (_, reader) => {
            values.Add(reader.Get<Tuple<int, string>>(0)); values.Add(reader.Get<Tuple<string, int>>(1));
            Assert.Equal("row", reader.Get<string>("id"));
            Assert.Throws<ArgumentOutOfRangeException>(() => reader.Get<int>(2));
        }, null);
        var result = CfdLoader.Load(new[] { new CfdSource("slots.cfd", "row: Row { first: First { number: 3, id: \"one\" }, second: Second { id: \"two\", number: 4 } }") },
            new CfdType[] { first, second, row }, data => data);
        Assert.True(result.Success, string.Join("; ", result.Diagnostics));
        Assert.Equal(Tuple.Create(3, "one"), values[0]);
        Assert.Equal(Tuple.Create("two", 4), values[1]);
    }

    private class Root { }
    private class Branch : Root { }
    private sealed class Leaf : Branch { }
    private sealed class Unrelated { }
    private sealed class Empty { }
    private sealed class Settings { }

    [Fact]
    public void TableRegistrationIncludesEachAncestorAndKeepsUnrelatedAndEmptyTablesSeparate()
    {
        var fields = Array.Empty<CfdField>();
        var types = new CfdType[] {
            new CfdType<Root>("Root", CfdTypeKind.Table, fields, null, null, null, abstractType: true),
            new CfdType<Branch>("Branch", CfdTypeKind.Table, fields, _ => new Branch(), (_, _) => { }, null, parent: "Root"),
            new CfdType<Leaf>("Leaf", CfdTypeKind.Table, fields, _ => new Leaf(), (_, _) => { }, null, parent: "Branch"),
            new CfdType<Unrelated>("Unrelated", CfdTypeKind.Table, fields, _ => new Unrelated(), (_, _) => { }, null),
            new CfdType<Empty>("Empty", CfdTypeKind.Table, fields, _ => new Empty(), (_, _) => { }, null),
            new CfdType<Settings>("Settings", CfdTypeKind.Singleton, fields, _ => new Settings(), (_, _) => { }, null),
        };
        var result = CfdLoader.Load(new[] { new CfdSource("tables.cfd", "branch: Branch {} same: Leaf {} same: Unrelated {} Settings: Settings {}") }, types, data => data);
        Assert.True(result.Success, string.Join("; ", result.Diagnostics));
        Assert.Equal(2, result.Data.Table<Root>().Count);
        Assert.Equal(2, result.Data.Table<Branch>().Count);
        Assert.Single(result.Data.Table<Leaf>());
        Assert.Single(result.Data.Table<Unrelated>());
        Assert.Empty(result.Data.Table<Empty>());
        var leaf = result.Data.Table<Leaf>()["same"];
        Assert.Same(leaf, result.Data.Table<Branch>()["same"]);
        Assert.Same(leaf, result.Data.Table<Root>()["same"]);
        Assert.Same(result.Data.Singleton<Settings>(), result.Data.Singleton<Settings>());
    }
}
