using System.Collections;
using System.Reflection;
using System.Security.Cryptography;
using System.Text;
using Coflow.Loader;

static class Fixtures
{
    internal static Fixture Wide(int recordCount, int fieldCount, int variantCount, bool dense, bool countDescriptors)
    {
        var names = Enumerable.Range(0, fieldCount).Select(index => "f" + index).ToArray();
        var descriptors = names.Select(name => (CfdField)new CfdField<int>(name, CfdValues.Dimension("language", CfdValues.Int))).ToArray();
        var counted = new CountedFields(descriptors);
        IReadOnlyList<CfdField> fields = countDescriptors ? counted : descriptors;
        var type = new CfdType<Row>("Row", CfdTypeKind.Table, fields, _ => new Row(),
            (row, reader) => row.Values = names.Select(reader.Get<int>).ToArray(), null);
        var text = new StringBuilder();
        for (var record = 0; record < recordCount; record++)
        {
            text.Append('r').Append(record).Append(": Row {");
            foreach (var name in names) text.Append(name).Append(": 1,");
            text.Append("}\n");
        }
        for (var variant = 0; variant < variantCount; variant++)
        {
            if (!dense)
            {
                text.Append("language[v").Append(variant).Append("] { r0: Row { f0: 2 } }\n");
                continue;
            }
            text.Append("language[v").Append(variant).Append("] {\n");
            for (var record = 0; record < recordCount; record++)
            {
                text.Append('r').Append(record).Append(": Row {");
                foreach (var name in names) text.Append(name).Append(": 2,");
                text.Append("}\n");
            }
            text.Append("}\n");
        }
        return new Fixture("wide", recordCount, fieldCount, variantCount, dense ? "dense" : "sparse",
            variantCount * (dense ? recordCount : 1), variantCount * (dense ? recordCount * fieldCount : 1),
            new CfdType[] { type }, new[] { new CfdSource("rows.cfd", text.ToString()) }, data =>
            {
                var rows = data.Table<Row>();
                Check(rows.Count == recordCount, "wide record count");
                for (var index = 0; index < recordCount; index++)
                {
                    var values = rows["r" + index].Values;
                    Check(values.Length == fieldCount && values.All(value => value == 1), "wide base field values");
                }
            }, countDescriptors ? counted : null);
    }

    internal static Fixture Realistic(int recordCount, int variantCount, bool dense)
    {
        var types = new CfdType[]
        {
            new CfdType<Stats>("Stats", CfdTypeKind.Data, new CfdField[]
            {
                new CfdField<int>("hp", CfdValues.Int),
                new CfdField<string>("label", CfdValues.String),
            }, null, null, reader => new Stats(reader.Get<int>("hp"), reader.Get<string>("label"))),
            new CfdType<Entry>("Entry", CfdTypeKind.Table, new CfdField[]
            {
                new CfdField<string>("name", CfdValues.Dimension("language", CfdValues.String)),
                new CfdField<int>("count", CfdValues.Int),
                new CfdField<Entry>("peer", CfdValues.Reference<Entry>("Entry")),
                new CfdField<string>("description", CfdValues.String),
                new CfdField<Stats>("stats", CfdValues.Object<Stats>("Stats")),
                new CfdField<IReadOnlyList<string>>("tags", CfdValues.List(CfdValues.String)),
                new CfdField<Option<string>>("note", CfdValues.Optional(CfdValues.String)),
                new CfdField<IReadOnlyDictionary<string, int>>("prices", CfdValues.Dictionary(CfdValues.String, CfdValues.Int)),
                new CfdField<string>("caption", CfdValues.String),
            }, _ => new Entry(), (entry, reader) =>
            {
                entry.Name = reader.Get<string>("name");
                entry.Count = reader.Get<int>("count");
                entry.Peer = reader.Get<Entry>("peer");
                entry.Description = reader.Get<string>("description");
                entry.Stats = reader.Get<Stats>("stats");
                entry.Tags = reader.Get<IReadOnlyList<string>>("tags");
                entry.Note = reader.Get<Option<string>>("note");
                entry.Prices = reader.Get<IReadOnlyDictionary<string, int>>("prices");
                entry.Caption = reader.Get<string>("caption");
            }, null),
        };
        var texts = new[] { new StringBuilder(), new StringBuilder() };
        for (var index = 0; index < recordCount; index++)
        {
            var peer = PeerIndex(index, recordCount);
            texts[index % 2].Append('r').Append(index).Append(": Entry { ")
                .Append("name: \"条目").Append(index).Append("\", count: ").Append(index + 1)
                .Append(", peer: &Entry::r").Append(peer)
                .Append(", description: \"文案").Append(index).Append("\\nsecond line \\\"quoted\\\"\"")
                .Append(", stats: Stats { hp: ").Append(index + 10).Append(", label: f\"HP {self.hp}\" }")
                .Append(", tags: [\"标签\", f\"{&Entry::r").Append(peer).Append(".name}\"]")
                .Append(", note: Some(\"note\"), prices: { \"base\": ").Append(index).Append(", \"bonus\": ").Append(index + 1).Append(" }")
                .Append(", caption: f\"{self.name}/{&Entry::r").Append(peer).Append(".name}/{self.stats.label}\" }\n");
        }
        var overlays = new StringBuilder();
        for (var variant = 0; variant < variantCount; variant++)
        {
            overlays.Append("language[v").Append(variant).Append("] {\n");
            for (var index = 0; index < (dense ? recordCount : 1); index++)
                overlays.Append('r').Append(index).Append(": Entry { name: \"译文").Append(variant).Append('_').Append(index).Append("\" }\n");
            overlays.Append("}\n");
        }
        var sources = new[]
        {
            // 覆盖先于基础文件，引用配对跨两个文件；输入顺序不依赖记录声明顺序。
            new CfdSource("language.cfd", overlays.ToString()),
            new CfdSource("even.cfd", texts[0].ToString()),
            new CfdSource("odd.cfd", texts[1].ToString()),
        };
        var fixture = new Fixture("realistic", recordCount, 9, variantCount, dense ? "dense" : "sparse",
            variantCount * (dense ? recordCount : 1), variantCount * (dense ? recordCount : 1), types, sources, data =>
            {
                var rows = Entries(data, recordCount);
                for (var index = 0; index < rows.Length; index++) VerifyEntry(rows, index, false, dense);
            });
        fixture.CreateViews = (data, cached, iterations) => new EntryViews(data, recordCount, dense, cached, iterations);
        return fixture;
    }

    internal static Fixture ManyTables(int recordCount, int typeCount)
    {
        var markers = new[] { typeof(int), typeof(long), typeof(string), typeof(Guid), typeof(DateTime), typeof(bool), typeof(float), typeof(double) };
        var create = typeof(Fixtures).GetMethod(nameof(CreateTable), BindingFlags.NonPublic | BindingFlags.Static)!;
        var registrations = new TableRegistration[typeCount];
        // CLR 泛型封闭与反射只发生在夹具准备阶段，计时内仍使用强类型构造回调。
        for (var index = 0; index < typeCount; index++)
        {
            var marker = typeof(ValueTuple<,>).MakeGenericType(markers[index / markers.Length], markers[index % markers.Length]);
            registrations[index] = (TableRegistration)create.MakeGenericMethod(marker).Invoke(null, new object[] { index, typeCount, recordCount })!;
        }
        var text = new StringBuilder();
        for (var index = 0; index < recordCount; index++)
            text.Append('r').Append(index).Append(": Row").Append(index % typeCount).Append(" { value: ").Append(index).Append(" }\n");
        return new Fixture("many-tables", recordCount, 1, 0, "none", 0, 0,
            registrations.Select(item => item.Type).ToArray(), new[] { new CfdSource("tables.cfd", text.ToString()) },
            data => { foreach (var registration in registrations) registration.Verify(data); });
    }

    private static TableRegistration CreateTable<TMarker>(int typeIndex, int typeCount, int recordCount)
    {
        var type = new CfdType<TypedRow<TMarker>>("Row" + typeIndex, CfdTypeKind.Table,
            new CfdField[] { new CfdField<int>("value", CfdValues.Int) }, _ => new TypedRow<TMarker>(),
            (row, reader) => row.Value = reader.Get<int>("value"), null);
        return new TableRegistration(type, data =>
        {
            var rows = data.Table<TypedRow<TMarker>>();
            var expectedCount = recordCount / typeCount + (typeIndex < recordCount % typeCount ? 1 : 0);
            Check(rows.Count == expectedCount, "typed table record count");
            for (var index = typeIndex; index < recordCount; index += typeCount)
                Check(rows["r" + index].Value == index, "typed table record value");
        });
    }

    private static Entry[] Entries(CfdData data, int count)
    {
        var table = data.Table<Entry>();
        Check(table.Count == count, "realistic record count");
        return Enumerable.Range(0, count).Select(index => table["r" + index]).ToArray();
    }

    private static int PeerIndex(int index, int count) => index % 2 == 1 ? index - 1 : index + 1 < count ? index + 1 : index;
    private static string Name(int index, bool projected, bool dense) => projected && (dense || index == 0) ? "译文0_" + index : "条目" + index;
    private static void VerifyEntry(Entry[] rows, int index, bool projected, bool dense)
    {
        var row = rows[index];
        var peer = PeerIndex(index, rows.Length);
        Check(row.Name == Name(index, projected, dense) && row.Count == index + 1, "realistic scalar values");
        Check(ReferenceEquals(row.Peer, rows[peer]), "cross-file cyclic reference identity");
        Check(row.Description == "文案" + index + "\nsecond line \"quoted\"", "escaped string");
        Check(row.Stats.Hp == index + 10 && row.Stats.Label == "HP " + (index + 10), "nested object template");
        Check(row.Tags.Count == 2 && row.Tags[0] == "标签" && row.Tags[1] == Name(peer, projected, dense), "nested list template");
        Check(row.Note.IsSome && row.Note.Value == "note", "optional string");
        Check(row.Prices.Count == 2 && row.Prices["base"] == index && row.Prices["bonus"] == index + 1, "nested dictionary");
        Check(row.Caption == Name(index, projected, dense) + "/" + Name(peer, projected, dense) + "/HP " + (index + 10), "cross-record template");
    }

    internal static void Check(bool condition, string label)
    {
        if (!condition) throw new InvalidOperationException("Benchmark validation failed: " + label);
    }

    private sealed record TableRegistration(CfdType Type, Action<CfdData> Verify);
    private sealed class TypedRow<TMarker> { internal int Value; }
    private sealed class Row : ICfdRecord
    {
        public CfdReader Reader { get; set; } = null!;
        internal int[] Values = Array.Empty<int>();
    }
    private readonly record struct Stats(int Hp, string Label);
    private sealed class Entry : ICfdRecord
    {
        public CfdReader Reader { get; set; } = null!;
        internal string Name = null!;
        internal int Count;
        internal Entry Peer = null!;
        internal string Description = null!;
        internal Stats Stats;
        internal IReadOnlyList<string> Tags = null!;
        internal Option<string> Note;
        internal IReadOnlyDictionary<string, int> Prices = null!;
        internal string Caption = null!;
    }

    private sealed class EntryViews : ViewWorkload
    {
        private readonly CfdData _data;
        private readonly Entry[] _roots;
        private readonly Entry[] _results;
        private readonly Entry[]? _expected;
        private readonly bool _dense;
        private readonly int _iterations;
        internal EntryViews(CfdData data, int recordCount, bool dense, bool cached, int iterations)
        {
            _data = data; _roots = Entries(data, recordCount); _results = new Entry[recordCount]; _dense = dense; _iterations = iterations;
            if (cached) _expected = _roots.Select(row => row.For("language", "v0")).ToArray();
        }
        internal override void Run()
        {
            for (var iteration = 0; iteration < _iterations; iteration++)
            for (var index = 0; index < _roots.Length; index++)
                _results[index] = _roots[index].For("language", "v0");
        }
        internal override void Verify()
        {
            for (var index = 0; index < _roots.Length; index++)
            {
                VerifyEntry(_roots, index, false, _dense);
                VerifyEntry(_results, index, true, _dense);
                Check(!ReferenceEquals(_roots[index], _results[index]), "independent view identity");
                Check(ReferenceEquals(_results[index], _expected?[index] ?? _roots[index].For("language", "v0")), "cached view identity");
            }
            GC.KeepAlive(_data);
        }
    }
}

sealed class Fixture
{
    private IReadOnlyList<CfdDocument>? _documents;
    internal Fixture(string name, int recordCount, int fieldCount, int variantCount, string overrideDensity,
        int overrideRecordCount, int overrideFieldCount, CfdType[] types, CfdSource[] sources, Action<CfdData> verifyData, CountedFields? countedFields = null)
    {
        Name = name; RecordCount = recordCount; FieldCount = fieldCount; VariantCount = variantCount; OverrideDensity = overrideDensity;
        OverrideRecordCount = overrideRecordCount; OverrideFieldCount = overrideFieldCount; Types = types; Sources = sources; VerifyData = verifyData;
        CountedFields = countedFields; SchemaDescriptorReads = countedFields?.Reads;
        SourceSha256 = Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(string.Join("\0", sources.SelectMany(source => new[] { source.Path, source.Text })))));
    }
    internal string Name { get; }
    internal int RecordCount { get; }
    internal int FieldCount { get; }
    internal int VariantCount { get; }
    internal string OverrideDensity { get; }
    internal int OverrideRecordCount { get; }
    internal int OverrideFieldCount { get; }
    internal CfdType[] Types { get; }
    internal CfdSource[] Sources { get; }
    internal string SourceSha256 { get; }
    internal CountedFields? CountedFields { get; }
    internal long? SchemaDescriptorReads { get; }
    internal IReadOnlyList<CfdDocument> Documents => _documents ??= CfdParser.ParseAll(Sources);
    internal Action<CfdData> VerifyData { get; }
    internal Func<CfdData, bool, int, ViewWorkload> CreateViews { get; set; } = (_, _, _) => throw new InvalidOperationException("Fixture has no dimension view workload.");
    internal void VerifyDocuments(IReadOnlyList<CfdDocument> documents)
    {
        Fixtures.Check(documents.Count == Sources.Length, "parsed document count");
        Fixtures.Check(documents.Sum(document => document.Records.Count) == RecordCount, "parsed record count");
        Fixtures.Check(documents.Sum(document => document.Records.Sum(record => record.Fields.Count)) == RecordCount * FieldCount, "parsed field count");
        Fixtures.Check(documents.Sum(document => document.Dimensions.Count) == VariantCount, "parsed variant count");
        Fixtures.Check(documents.Sum(document => document.Dimensions.Sum(block => block.Records.Count)) == OverrideRecordCount, "parsed override count");
        Fixtures.Check(documents.Sum(document => document.Dimensions.Sum(block => block.Records.Sum(record => record.Fields.Count))) == OverrideFieldCount, "parsed override field count");
    }
}

abstract class ViewWorkload
{
    internal abstract void Run();
    internal abstract void Verify();
}

sealed class CountedFields(CfdField[] items) : IReadOnlyList<CfdField>
{
    internal long Reads;
    public int Count => items.Length;
    public CfdField this[int index] { get { Reads++; return items[index]; } }
    public IEnumerator<CfdField> GetEnumerator()
    {
        for (var index = 0; index < Count; index++) yield return this[index];
    }
    IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
}
