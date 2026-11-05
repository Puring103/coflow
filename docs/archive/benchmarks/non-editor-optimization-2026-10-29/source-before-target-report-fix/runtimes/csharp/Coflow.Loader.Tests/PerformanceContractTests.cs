using System;
using System.Collections.Generic;
using System.Linq;
using System.Runtime.CompilerServices;
using System.Text;
using System.Threading.Tasks;
using Coflow.Loader;
using Xunit;

namespace Coflow.Loader.Tests;

public sealed class PerformanceContractTests
{
    private readonly struct Payload
    {
        internal Payload(string text) { Text = text; }
        internal string Text { get; }
    }

    private sealed class Row : ICfdRecord
    {
        public CfdReader Reader { get; set; } = null!;
        internal string Name { get; set; } = null!;
        internal int Price { get; set; }
        internal string Label { get; set; } = null!;
        internal IReadOnlyList<int> Numbers { get; set; } = null!;
        internal IReadOnlyList<Payload> Payloads { get; set; } = null!;
        internal Option<Row> Next { get; set; }
    }

    private static readonly CfdType[] Types =
    {
        new CfdType<Payload>("Payload", CfdTypeKind.Data, new CfdField[] {
            new CfdField<string>("text", CfdValues.String, CfdDefaults.Template("f\"{&Row::b.name}\"")),
        }, null, null, reader => new Payload(reader.Get<string>("text"))),
        new CfdType<Row>("Row", CfdTypeKind.Table, new CfdField[] {
            new CfdField<string>("name", CfdValues.Dimension("language", CfdValues.String)),
            new CfdField<int>("price", CfdValues.Dimension("balance", CfdValues.Int), CfdDefaults.Int(1)),
            new CfdField<string>("label", CfdValues.String, CfdDefaults.Template("f\"{self.name}/{self.price}\"")),
            new CfdField<IReadOnlyList<int>>("numbers", CfdValues.Dimension("language", CfdValues.List(CfdValues.Int)), CfdDefaults.Array(CfdDefaults.Int(1), CfdDefaults.Int(2))),
            new CfdField<IReadOnlyList<Payload>>("payloads", CfdValues.List(CfdValues.Object<Payload>("Payload")), CfdDefaults.Array()),
            new CfdField<Option<Row>>("next", CfdValues.Optional(CfdValues.Reference<Row>("Row")), CfdDefaults.None()),
        }, _ => new Row(), (row, reader) => {
            row.Name = reader.Get<string>("name"); row.Price = reader.Get<int>("price");
            row.Label = reader.Get<string>("label"); row.Numbers = reader.Get<IReadOnlyList<int>>("numbers");
            row.Payloads = reader.Get<IReadOnlyList<Payload>>("payloads"); row.Next = reader.Get<Option<Row>>("next");
        }, null),
    };

    private static LoadResult<Table<Row>> Load(string records = "a: Row { name: \"A\", next: &Row::b } b: Row { name: \"B\", next: &Row::a }", string overlays = "language[ja] { a: Row { name: \"甲\" } b: Row { name: \"乙\" } } balance[v] { a: Row { price: 2 } }")
        => CfdLoader.Load(new[] { new CfdSource("base.cfd", records), new CfdSource("overlays.cfd", overlays) }, Types, data => data.Table<Row>());

    [Fact]
    public void CachedSelectionsAndDimensionMetadataDoNotAllocate()
    {
        var result = Load();
        Assert.True(result.Success, string.Join("; ", result.Diagnostics));
        var a = result.Data["a"];
        var ja = a.For("language", "ja");
        var combined = ja.For("balance", "v");
        var fallback = a.For("language", "missing");
        var names = a.Variants("language");
        for (var i = 0; i < 1024; i++)
        {
            a.For("language", "ja"); ja.For("balance", "v");
            a.For("language", "missing"); a.Variants("language"); a.HasVariant("name", "ja");
        }
        var before = GC.GetAllocatedBytesForCurrentThread();
        var matched = 0;
        for (var i = 0; i < 10000; i++)
        {
            if (ReferenceEquals(ja, a.For("language", "ja"))) matched++;
            if (ReferenceEquals(combined, ja.For("balance", "v"))) matched++;
            if (ReferenceEquals(fallback, a.For("language", "missing"))) matched++;
            if (ReferenceEquals(names, a.Variants("language"))) matched++;
            if (a.HasVariant("name", "ja")) matched++;
        }
        var allocated = GC.GetAllocatedBytesForCurrentThread() - before;
        Assert.Equal(50000, matched);
        Assert.Equal(0, allocated);
        Assert.Equal("甲/2", combined.Label);
        Assert.Equal("A/1", fallback.Label);
        Assert.Same(combined, a.For("balance", "v").For("language", "ja"));
        Assert.Throws<NotSupportedException>(() => ((IList<string>)names)[0] = "changed");
    }

    [Fact]
    public void ConcurrentCachedAndNewSelectionOrdersKeepOneViewAndCyclicIdentity()
    {
        var result = Load();
        Assert.True(result.Success);
        var a = result.Data["a"];
        var values = new Row[256];
        Parallel.For(0, values.Length, i => values[i] = i % 2 == 0
            ? a.For("language", "ja").For("balance", "v")
            : a.For("balance", "v").For("language", "ja"));
        foreach (var value in values)
        {
            Assert.Same(values[0], value);
            Assert.Same(value, value.Next.Value.Next.Value);
            Assert.Equal("甲/2", value.Label);
        }
        Assert.Equal("A/1", a.Label);
    }

    [Fact]
    public void AnonymousObjectsInsideCollectionsReevaluateTheirDefaultTemplates()
    {
        var result = Load("a: Row { name: \"A\", payloads: [{}, Payload {}] } b: Row { name: \"B\" }");
        Assert.True(result.Success, string.Join("; ", result.Diagnostics));
        var a = result.Data["a"];
        Assert.All(a.Payloads, payload => Assert.Equal("B", payload.Text));
        Assert.All(a.For("language", "ja").Payloads, payload => Assert.Equal("乙", payload.Text));
        Assert.All(a.For("language", "missing").Payloads, payload => Assert.Equal("B", payload.Text));
        Assert.All(a.Payloads, payload => Assert.Equal("B", payload.Text));
    }

    [Fact]
    public void NestedOverlayFailureRetainsTheOverlaySourceAndSpan()
    {
        var result = Load(overlays: "language[ja] { a: Row { numbers: [1, \"wrong\"] } }");
        Assert.False(result.Success);
        var diagnostic = Assert.Single(result.Diagnostics);
        Assert.Equal("CFD-TYPE", diagnostic.Code);
        Assert.Equal("overlays.cfd", diagnostic.Path);
        Assert.True(diagnostic.Span!.Value.StartColumn > 1);
    }

    [Fact]
    public void DuplicateAnonymousObjectFieldsStillPrecedeUnknownFieldDiagnostics()
    {
        var result = Load("a: Row { name: \"A\", payloads: [{ ghost: 1, text: \"one\", text: \"two\" }] } b: Row { name: \"B\" }", "");
        Assert.False(result.Success);
        Assert.Equal("CFD-DUPLICATE-FIELD", Assert.Single(result.Diagnostics).Code);
        Assert.Equal("base.cfd", result.Diagnostics[0].Path);
    }

    private sealed class WideRow { internal long[] Values { get; set; } = null!; }

    [Fact]
    public void WideSchemasResolveReorderedUnicodeFieldsAcrossFiles()
    {
        var names = Enumerable.Range(0, 257).Select(index => index % 2 == 0 ? "字段" + index : "field" + index).ToArray();
        var fields = names.Select(name => (CfdField)new CfdField<long>(name, CfdValues.Long)).ToArray();
        var type = new CfdType<WideRow>("宽表", CfdTypeKind.Table, fields, _ => new WideRow(),
            (row, reader) => row.Values = names.Select(reader.Get<long>).ToArray(), null);
        string Source(string id, int offset) => id + ": 宽表 { " + string.Join(", ", names.Select((name, index) => name + ": " + (index + offset)).Reverse()) + " }";
        var result = CfdLoader.Load(new[] { new CfdSource("one.cfd", Source("one", 0)), new CfdSource("two.cfd", Source("two", 1000)) },
            new CfdType[] { type }, data => data.Table<WideRow>());
        Assert.True(result.Success, string.Join("; ", result.Diagnostics));
        Assert.Equal(Enumerable.Range(0, names.Length).Select(value => (long)value), result.Data["one"].Values);
        Assert.Equal(Enumerable.Range(0, names.Length).Select(value => (long)value + 1000), result.Data["two"].Values);
    }

    private class BaseRow { }
    private sealed class DerivedRow : BaseRow { }

    [Fact]
    public void TableBucketsPreserveClrAssignableMembershipAndSharedObjectIdentity()
    {
        var types = new CfdType[] {
            new CfdType<BaseRow>("Base", CfdTypeKind.Table, Array.Empty<CfdField>(), _ => new BaseRow(), (_, _) => { }, null),
            new CfdType<DerivedRow>("Other", CfdTypeKind.Table, Array.Empty<CfdField>(), _ => new DerivedRow(), (_, _) => { }, null),
        };
        var result = CfdLoader.Load(new[] { new CfdSource("tables.cfd", "base: Base {} other: Other {}") }, types,
            data => Tuple.Create(data.Table<BaseRow>(), data.Table<DerivedRow>()));
        Assert.True(result.Success);
        Assert.Equal(2, result.Data.Item1.Count);
        Assert.Same(result.Data.Item2["other"], result.Data.Item1["other"]);
    }

    [Fact]
    public void DirectTableQueriesPreserveExceptionsReadOnlyViewsAndIdentity()
    {
        var result = Load();
        Assert.True(result.Success);
        var table = result.Data;
        Assert.Equal(2, table.Count);
        Assert.True(table.ContainsKey("a"));
        Assert.False(table.ContainsKey("missing"));
        Assert.True(table.TryGetValue("a", out var a));
        Assert.Same(table["a"], a);
        Assert.False(table.TryGetValue("missing", out var missing));
        Assert.Null(missing);
        Assert.Throws<KeyNotFoundException>(() => table["missing"]);
        Assert.Throws<ArgumentNullException>(() => table[null!]);
        Assert.Throws<ArgumentNullException>(() => table.ContainsKey(null!));
        Assert.Throws<ArgumentNullException>(() => table.TryGetValue(null!, out _));
        Assert.Equal(new[] { "a", "b" }, table.Keys);
        Assert.Same(a, table.Values.First());
        Assert.Same(a, table.First().Value);
        Assert.Throws<NotSupportedException>(() => ((ICollection<string>)table.Keys).Add("c"));
        Assert.Throws<NotSupportedException>(() => ((ICollection<Row>)table.Values).Clear());
        Assert.Same(a, table["a"]);
    }

    [Fact]
    public void SharedNamesKeepExactUtf16IdentityAcrossOddAndEvenLengths()
    {
        var pool = new CfdNamePool();
        var names = new[] { "", "a", "ab", "a\0", "a\0b", "ab\0", "\U00010400x", "\U00010401x", "é", "e\u0301" };
        var shared = names.Select(name => pool.GetOrAdd(name.AsSpan())).ToArray();
        for (var index = 0; index < names.Length; index++)
        {
            Assert.Equal(names[index], shared[index]);
            Assert.Same(shared[index], pool.GetOrAdd(new string(names[index].ToCharArray()).AsSpan()));
        }
        Assert.Equal(names.Length, shared.Distinct(StringComparer.Ordinal).Count());
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    private static (Table<Row> Data, WeakReference Record, WeakReference Field, WeakReference Fields) LoadTrackedRootSyntax()
    {
        var documents = CfdParser.ParseAll(new[] {
            new CfdSource("基础.cfd", "a: Row { name: \"A\", payloads: [{}, Payload {}], next: &Row::b } b: Row { name: \"B\", next: &Row::a }"),
            new CfdSource("覆盖.cfd", "language[ja] { a: Row { name: \"甲\" } b: Row { name: \"乙\" } } balance[v] { a: Row { price: 2 } }"),
        });
        var root = documents[0].Records[0];
        var tracked = (Record: new WeakReference(root), Field: new WeakReference(root.Fields[0]), Fields: new WeakReference(root.Fields));
        var context = new LoadContext(documents, Types, null);
        context.Build();
        return (new CfdData(Types, context.Records).Table<Row>(), tracked.Record, tracked.Field, tracked.Fields);
    }

    [Fact]
    public void LoadedDataReleasesRootSyntaxWhilePreservingLaterDimensionMaterialization()
    {
        var loaded = LoadTrackedRootSyntax();
        GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect();
        Assert.False(loaded.Record.IsAlive);
        Assert.False(loaded.Field.IsAlive);
        Assert.False(loaded.Fields.IsAlive);
        var a = loaded.Data["a"];
        Assert.Equal("a", a.Reader.Get<string>("id"));
        Assert.Equal("A/1", a.Label);
        Assert.Same(a, a.Next.Value.Next.Value);
        // 回收的是顶层包装；缺省模板、嵌套对象来源与组合视图仍需完整可用。
        var combined = a.For("language", "ja").For("balance", "v");
        Assert.Equal("甲/2", combined.Label);
        Assert.All(combined.Payloads, payload => Assert.Equal("乙", payload.Text));
        Assert.Same(combined, combined.Next.Value.Next.Value);
        Assert.Same(combined, a.For("balance", "v").For("language", "ja"));
        Assert.Equal("A/1", a.For("language", "missing").Label);
        GC.KeepAlive(loaded.Data);
    }

    [Theory]
    [InlineData(1)]
    [InlineData(2)]
    [InlineData(3)]
    public void UntypedIdentityIndexNeverResurrectsAnAmbiguousId(int roots)
    {
        var names = new[] { "First", "Second", "Third" }.Take(roots).ToArray();
        var types = names.Select(name => (CfdType)new CfdType<Row>(name, CfdTypeKind.Table,
            Array.Empty<CfdField>(), _ => new Row(), (_, _) => { }, null)).ToArray();
        var documents = CfdParser.ParseAll(new[] { new CfdSource("ids.cfd", string.Join(" ", names.Select(name => "same: " + name + " {}"))) });
        var context = new LoadContext(documents, types, null);
        var span = new CfdSpan(3, 4, 3, 12);
        for (var index = 0; index < names.Length; index++)
            Assert.Same(context.Records[index], context.Find(names[index], "same", "reference.cfd", span));
        if (roots == 1) Assert.Same(context.Records[0], context.Find(null, "same", "reference.cfd", span));
        else
        {
            var error = Assert.Throws<CfdLoadException>(() => context.Find(null, "same", "reference.cfd", span));
            var diagnostic = Assert.Single(error.Diagnostics);
            Assert.Equal("CFD-REFERENCE", diagnostic.Code);
            Assert.Equal("reference.cfd", diagnostic.Path);
            Assert.Equal(span, diagnostic.Span);
        }
        Assert.Throws<CfdLoadException>(() => context.Find(null, "absent", "reference.cfd", span));
        Assert.Throws<CfdLoadException>(() => context.Find("Missing", "same", "reference.cfd", span));
    }

    [Fact]
    public void OverlayValidationStillPrecedesUnknownBaseFields()
    {
        var result = Load("a: Row { name: \"A\", ghost: 1 } b: Row { name: \"B\" }", "language[ja] { a: Row { ghost: 2 } }");
        Assert.False(result.Success);
        var diagnostic = Assert.Single(result.Diagnostics);
        Assert.Equal("CFD-DIMENSION", diagnostic.Code);
        Assert.Equal("overlays.cfd", diagnostic.Path);
    }

    [Fact]
    public void SingletonIdentityAndMissingFieldsKeepRecordSourceSpans()
    {
        const string source = "\r\nwrong: Global { ghost: 1 }";
        var parsed = CfdParser.Parse(new CfdSource("单例.cfd", source));
        var singleton = new CfdType<Row>("Global", CfdTypeKind.Singleton,
            new CfdField[] { new CfdField<string>("name", CfdValues.String) }, _ => new Row(), (_, _) => { }, null);
        var result = CfdLoader.Load(new[] { new CfdSource("单例.cfd", source) }, new[] { singleton }, data => data.Singleton<Row>());
        Assert.False(result.Success);
        var diagnostic = Assert.Single(result.Diagnostics);
        Assert.Equal("CFD-SINGLETON", diagnostic.Code);
        Assert.Equal("单例.cfd", diagnostic.Path);
        Assert.Equal(parsed.Records[0].Span, diagnostic.Span);

        const string missingSource = "\r\na: Row {}";
        var missing = Load(missingSource, "");
        var missingDiagnostic = Assert.Single(missing.Diagnostics);
        Assert.Equal("CFD-MISSING", missingDiagnostic.Code);
        Assert.Equal("base.cfd", missingDiagnostic.Path);
        Assert.Equal(CfdParser.Parse(new CfdSource("base.cfd", missingSource)).Records[0].Span, missingDiagnostic.Span);
    }

    [Fact]
    public void VariantOnlyLongReferenceChainsStillEnforceTheConstructionBudget()
    {
        var type = new CfdType<Row>("Row", CfdTypeKind.Table, new CfdField[] {
            new CfdField<Option<Row>>("next", CfdValues.Dimension("links", CfdValues.Optional(CfdValues.Reference<Row>("Row"))), CfdDefaults.None()),
        }, _ => new Row(), (row, reader) => row.Next = reader.Get<Option<Row>>("next"), null);
        var text = new StringBuilder();
        for (var i = 0; i < 300; i++) text.Append('r').Append(i).Append(": Row {} ");
        text.Append("links[long] {");
        for (var i = 0; i < 299; i++) text.Append('r').Append(i).Append(": Row { next: &Row::r").Append(i + 1).Append(" } ");
        text.Append('}');
        var result = CfdLoader.Load(new[] { new CfdSource("chain.cfd", text.ToString()) }, new CfdType[] { type }, data => data.Table<Row>());
        Assert.False(result.Success);
        Assert.Equal("CFD-LIMIT", Assert.Single(result.Diagnostics).Code);
        Assert.Equal("chain.cfd", result.Diagnostics[0].Path);
    }

    [Fact]
    public void Exactly256ReferenceLevelsPreserveCyclicIdentityOnTheDefaultThread()
    {
        var type = new CfdType<Row>("Row", CfdTypeKind.Table, new CfdField[] {
            new CfdField<Option<Row>>("next", CfdValues.Dimension("links", CfdValues.Optional(CfdValues.Reference<Row>("Row"))), CfdDefaults.None()),
        }, _ => new Row(), (row, reader) => row.Next = reader.Get<Option<Row>>("next"), null);
        var text = new StringBuilder();
        for (var i = 0; i < 256; i++) text.Append('r').Append(i).Append(": Row {} ");
        text.Append("links[long] {");
        for (var i = 0; i < 256; i++) text.Append('r').Append(i).Append(": Row { next: &Row::r").Append((i + 1) % 256).Append(" } ");
        text.Append('}');
        var result = CfdLoader.Load(new[] { new CfdSource("chain.cfd", text.ToString()) }, new CfdType[] { type }, data => data.Table<Row>());
        Assert.True(result.Success, string.Join("; ", result.Diagnostics));
        var start = result.Data["r0"].For("links", "long");
        var cursor = start;
        for (var i = 0; i < 256; i++) cursor = cursor.Next.Value;
        Assert.Same(start, cursor);
    }

    [Theory]
    [InlineData(256, true)]
    [InlineData(257, false)]
    public void DeepTemplatesRetainTheExactConstructionLimitOnTheDefaultThread(int count, bool success)
    {
        var type = new CfdType<Row>("Row", CfdTypeKind.Table, new CfdField[] {
            new CfdField<string>("name", CfdValues.String),
        }, _ => new Row(), (row, reader) => row.Name = reader.Get<string>("name"), null);
        var text = new StringBuilder();
        for (var i = 0; i < count; i++)
        {
            text.Append('r').Append(i).Append(": Row { name: ");
            if (i + 1 == count) text.Append("\"end\"");
            else text.Append("f\"{&Row::r").Append(i + 1).Append(".name}\"");
            text.Append(" } ");
        }
        var result = CfdLoader.Load(new[] { new CfdSource("templates.cfd", text.ToString()) }, new CfdType[] { type }, data => data.Table<Row>());
        Assert.Equal(success, result.Success);
        if (success) Assert.All(result.Data.Values, row => Assert.Equal("end", row.Name));
        else Assert.Equal("CFD-LIMIT", Assert.Single(result.Diagnostics).Code);
    }

    private sealed class LinkedPayload { internal Option<Row> Next { get; set; } }
    private sealed class CollectionRow { internal object Items { get; set; } = null!; }

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public void SuspendedCollectionsConstructEveryCompletedInlineObjectOnlyOnce(bool dictionary)
    {
        var constructed = 0;
        var payload = CfdValues.Object<LinkedPayload>("Payload");
        CfdField field = dictionary
            ? new CfdField<IReadOnlyDictionary<string, LinkedPayload>>("items", CfdValues.Dictionary(CfdValues.String, payload))
            : new CfdField<IReadOnlyList<IReadOnlyList<LinkedPayload>>>("items", CfdValues.List(CfdValues.List(payload)));
        var types = new CfdType[] {
            new CfdType<LinkedPayload>("Payload", CfdTypeKind.Data, new CfdField[] {
                new CfdField<Option<Row>>("next", CfdValues.Optional(CfdValues.Reference<Row>("Row")), CfdDefaults.None()),
            }, null, null, reader => { constructed++; return new LinkedPayload { Next = reader.Get<Option<Row>>("next") }; }),
            new CfdType<CollectionRow>("Collection", CfdTypeKind.Table, new[] { field }, _ => new CollectionRow(),
                (row, reader) => row.Items = dictionary ? reader.Get<IReadOnlyDictionary<string, LinkedPayload>>("items") : reader.Get<IReadOnlyList<IReadOnlyList<LinkedPayload>>>("items"), null),
            new CfdType<Row>("Row", CfdTypeKind.Table, new CfdField[] {
                new CfdField<Option<Row>>("next", CfdValues.Optional(CfdValues.Reference<Row>("Row")), CfdDefaults.None()),
            }, _ => new Row(), (row, reader) => row.Next = reader.Get<Option<Row>>("next"), null),
        };
        var text = new StringBuilder(dictionary
            ? "root: Collection { items: { \"one\": {}, \"two\": {}, \"three\": {}, \"four\": { next: &Row::r0 } } } "
            : "root: Collection { items: [[{}, {}], [{}, { next: &Row::r0 }]] } ");
        for (var i = 0; i < 96; i++) text.Append('r').Append(i).Append(": Row { next: ").Append(i == 95 ? "None" : "&Row::r" + (i + 1)).Append(" } ");
        var result = CfdLoader.Load(new[] { new CfdSource("collections.cfd", text.ToString()) }, types, data => data.Table<CollectionRow>());
        Assert.True(result.Success, string.Join("; ", result.Diagnostics));
        Assert.Equal(4, constructed);
        var linked = dictionary ? ((IReadOnlyDictionary<string, LinkedPayload>)result.Data["root"].Items)["four"]
            : ((IReadOnlyList<IReadOnlyList<LinkedPayload>>)result.Data["root"].Items)[1][1];
        var cursor = linked.Next;
        var length = 0;
        while (cursor.IsSome) { length++; cursor = cursor.Value.Next; }
        Assert.Equal(96, length);
    }

    private sealed class MixedReferenceRow { internal Option<MixedReferenceRow> Next; internal object Items = null!; }
    private sealed class MixedReferencePayload { internal Option<MixedReferenceRow> Next; }

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public void CollectionSuspensionPreservesExplicitParentFramesAndCompletedObjects(bool dictionary)
    {
        var constructed = 0;
        var payload = CfdValues.Object<MixedReferencePayload>("Payload");
        var reference = CfdValues.Optional(CfdValues.Reference<MixedReferenceRow>("Row"));
        CfdField items = dictionary
            ? new CfdField<IReadOnlyDictionary<string, MixedReferencePayload>>("items", CfdValues.Dictionary(CfdValues.String, payload), CfdDefaults.Dictionary())
            : new CfdField<IReadOnlyList<MixedReferencePayload>>("items", CfdValues.List(payload), CfdDefaults.Array());
        var types = new CfdType[] {
            new CfdType<MixedReferencePayload>("Payload", CfdTypeKind.Data,
                new CfdField[] { new CfdField<Option<MixedReferenceRow>>("next", reference, CfdDefaults.None()) }, null, null,
                reader => { constructed++; return new MixedReferencePayload { Next = reader.Get<Option<MixedReferenceRow>>("next") }; }),
            new CfdType<MixedReferenceRow>("Row", CfdTypeKind.Table,
                new CfdField[] { new CfdField<Option<MixedReferenceRow>>("next", reference, CfdDefaults.None()), items },
                _ => new MixedReferenceRow(), (row, reader) => {
                    row.Next = reader.Get<Option<MixedReferenceRow>>("next");
                    row.Items = dictionary ? reader.Get<IReadOnlyDictionary<string, MixedReferencePayload>>("items") : reader.Get<IReadOnlyList<MixedReferencePayload>>("items");
                }, null),
        };
        var text = new StringBuilder();
        for (var index = 0; index < 137; index++)
        {
            text.Append('r').Append(index).Append(": Row { next: ")
                .Append(index == 40 || index == 136 ? "None" : "&Row::r" + (index + 1));
            if (index == 40) text.Append(dictionary
                ? ", items: { \"one\": {}, \"two\": {}, \"three\": {}, \"four\": { next: &Row::r41 } }"
                : ", items: [{}, {}, {}, { next: &Row::r41 }]");
            text.Append(" } ");
        }
        var result = CfdLoader.Load(new[] { new CfdSource("mixed-collections.cfd", text.ToString()) }, types, data => data.Table<MixedReferenceRow>());
        Assert.True(result.Success, string.Join("; ", result.Diagnostics));
        Assert.Equal(4, constructed);
        var parent = result.Data["r0"];
        for (var index = 0; index < 40; index++) parent = parent.Next.Value;
        var last = dictionary ? ((IReadOnlyDictionary<string, MixedReferencePayload>)parent.Items)["four"]
            : ((IReadOnlyList<MixedReferencePayload>)parent.Items)[3];
        Assert.Same(result.Data["r41"], last.Next.Value);
        var cursor = last.Next;
        var length = 0;
        while (cursor.IsSome) { length++; cursor = cursor.Value.Next; }
        Assert.Equal(96, length);
    }

    [Fact]
    public void DeepReferenceErrorsKeepDepthFirstDiagnosticOrder()
    {
        var type = new CfdType<Row>("Row", CfdTypeKind.Table, new CfdField[] {
            new CfdField<Option<Row>>("next", CfdValues.Optional(CfdValues.Reference<Row>("Row")), CfdDefaults.None()),
            new CfdField<int>("price", CfdValues.Int),
        }, _ => new Row(), (row, reader) => { row.Next = reader.Get<Option<Row>>("next"); row.Price = reader.Get<int>("price"); }, null);
        var text = new StringBuilder();
        for (var i = 0; i < 96; i++) text.Append('r').Append(i).Append(": Row { next: ").Append(i == 95 ? "&Row::absent" : "&Row::r" + (i + 1)).Append(", price: \"wrong\" } ");
        var result = CfdLoader.Load(new[] { new CfdSource("errors.cfd", text.ToString()) }, new CfdType[] { type }, data => data.Table<Row>());
        Assert.False(result.Success);
        Assert.Equal("CFD-REFERENCE", Assert.Single(result.Diagnostics).Code);
        Assert.Contains("absent", result.Diagnostics[0].Message);
    }

    [Theory]
    [InlineData(256, true)]
    [InlineData(257, false)]
    public void ExplicitReferencesAndSuspendedTemplatesShareOneLogicalDepthBudget(int count, bool success)
    {
        var type = new CfdType<Row>("Row", CfdTypeKind.Table, new CfdField[] {
            new CfdField<Option<Row>>("next", CfdValues.Optional(CfdValues.Reference<Row>("Row")), CfdDefaults.None()),
            new CfdField<string>("name", CfdValues.String),
        }, _ => new Row(), (row, reader) => { row.Next = reader.Get<Option<Row>>("next"); row.Name = reader.Get<string>("name"); }, null);
        var text = new StringBuilder();
        for (var index = 0; index < count; index++)
        {
            text.Append('r').Append(index).Append(": Row { next: ")
                .Append(index < 40 ? "&Row::r" + (index + 1) : "None").Append(", name: ");
            if (index >= 40 && index + 1 < count) text.Append("f\"{&Row::r").Append(index + 1).Append(".name}\"");
            else text.Append("\"end\"");
            text.Append(" } ");
        }
        var result = CfdLoader.Load(new[] { new CfdSource("mixed.cfd", text.ToString()) }, new CfdType[] { type }, data => data.Table<Row>());
        Assert.Equal(success, result.Success);
        if (success) Assert.All(result.Data.Values, row => Assert.Equal("end", row.Name));
        else Assert.Equal("CFD-LIMIT", Assert.Single(result.Diagnostics).Code);
    }

    [Fact]
    public void ATemplateReadingAPendingAncestorFieldKeepsTheCycleDiagnostic()
    {
        var type = new CfdType<Row>("Row", CfdTypeKind.Table, new CfdField[] {
            new CfdField<Row>("next", CfdValues.Reference<Row>("Row")),
            new CfdField<string>("name", CfdValues.String),
        }, _ => new Row(), (_, _) => { }, null);
        var result = CfdLoader.Load(new[] { new CfdSource("parent.cfd",
            "a: Row { next: &Row::b, name: \"A\" } b: Row { next: &Row::a, name: f\"{&Row::a.next}\" }") },
            new CfdType[] { type }, data => data.Table<Row>());
        Assert.False(result.Success);
        var diagnostic = Assert.Single(result.Diagnostics);
        Assert.Equal("CFD-INTERPOLATION-CYCLE", diagnostic.Code);
        Assert.Contains("next", diagnostic.Message);
    }

    [Fact]
    public void DepthLimitPrecedesTheMissingTargetOfThe257thReference()
    {
        var type = new CfdType<Row>("Row", CfdTypeKind.Table, new CfdField[] {
            new CfdField<Row>("next", CfdValues.Reference<Row>("Row")),
        }, _ => new Row(), (_, _) => { }, null);
        var text = new StringBuilder();
        for (var index = 0; index < 257; index++)
            text.Append('r').Append(index).Append(": Row { next: &Row::").Append(index == 256 ? "absent" : "r" + (index + 1)).Append(" } ");
        var result = CfdLoader.Load(new[] { new CfdSource("limit.cfd", text.ToString()) }, new CfdType[] { type }, data => data.Table<Row>());
        Assert.False(result.Success);
        Assert.Equal("CFD-LIMIT", Assert.Single(result.Diagnostics).Code);
    }

    private sealed class NestedReferenceRow { internal Option<Option<NestedReferenceRow>> Next; }

    [Fact]
    public void ScheduledReferenceDependenciesRespectEveryOptionAndConstantLayer()
    {
        var type = new CfdType<NestedReferenceRow>("Row", CfdTypeKind.Table, new CfdField[] {
            new CfdField<Option<Option<NestedReferenceRow>>>("next", CfdValues.Dimension("links",
                CfdValues.Optional(CfdValues.Optional(CfdValues.Reference<NestedReferenceRow>("Row"))))),
        }, _ => new NestedReferenceRow(), (row, reader) => row.Next = reader.Get<Option<Option<NestedReferenceRow>>>("next"), null);
        var constants = new Dictionary<string, CfdValueNode> {
            ["first"] = CfdDefaults.Scalar("second"),
            ["second"] = CfdDefaults.Some(CfdDefaults.Scalar("third")),
            ["third"] = CfdDefaults.Some(CfdDefaults.Scalar("fourth")),
            ["fourth"] = CfdDefaults.Reference("Row", "b"),
        };
        var result = CfdLoader.Load(new[] { new CfdSource("constants.cfd",
            "a: Row { next: first } b: Row { next: Some(None) } c: Row { next: None }") },
            new CfdType[] { type }, data => data.Table<NestedReferenceRow>(), constants);
        Assert.True(result.Success, string.Join("; ", result.Diagnostics));
        Assert.Same(result.Data["b"], result.Data["a"].Next.Value.Value);
        Assert.True(result.Data["b"].Next.IsSome);
        Assert.False(result.Data["b"].Next.Value.IsSome);
        Assert.False(result.Data["c"].Next.IsSome);
    }

    [Fact]
    public void WrappedConstantAliasesReevaluateTemplatesInEachDimensionContext()
    {
        var type = new CfdType<Row>("Row", CfdTypeKind.Table, new CfdField[] {
            new CfdField<string>("name", CfdValues.Dimension("language", CfdValues.String)),
            new CfdField<string>("label", CfdValues.Dimension("language", CfdValues.String), CfdDefaults.Scalar("first")),
        }, _ => new Row(), (row, reader) => { row.Name = reader.Get<string>("name"); row.Label = reader.Get<string>("label"); }, null);
        var constants = new Dictionary<string, CfdValueNode> {
            ["first"] = CfdDefaults.Scalar("second"), ["second"] = CfdDefaults.Template("f\"{self.name}\""),
        };
        var result = CfdLoader.Load(new[] { new CfdSource("aliases.cfd", "a: Row { name: \"A\" } language[ja] { a: Row { name: \"甲\" } }") },
            new CfdType[] { type }, data => data.Table<Row>(), constants);
        Assert.True(result.Success, string.Join("; ", result.Diagnostics));
        Assert.Equal("A", result.Data["a"].Label);
        Assert.Equal("甲", result.Data["a"].For("language", "ja").Label);
    }

    [Fact]
    public void IndependentlyLoadedContextsRetainTheirOwnSelectionsAndSources()
    {
        var first = Load();
        var second = Load(overlays: "language[ja] { a: Row { name: \"別\" } }");
        Assert.True(first.Success); Assert.True(second.Success);
        var oldView = first.Data["a"].For("language", "ja");
        var newView = second.Data["a"].For("language", "ja");
        Assert.Equal("甲", oldView.Name);
        Assert.Equal("別", newView.Name);
        Assert.NotSame(oldView, newView);
        Assert.Same(oldView, first.Data["a"].For("language", "ja"));
    }
}
