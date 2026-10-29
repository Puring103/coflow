using System;
using System.Collections;
using System.Collections.Generic;
using System.Linq;
using System.Text;
using Xunit;

namespace Coflow.Loader.Tests;

public sealed class DescriptorIndexTests
{
    private sealed class Row : ICfdRecord
    {
        public CfdReader Reader { get; set; } = null!;
        internal int[] Values { get; set; } = Array.Empty<int>();
    }

    private sealed class CountedFields : IReadOnlyList<CfdField>
    {
        internal readonly CfdField[] Items;
        internal int Reads;
        internal CountedFields(CfdField[] items) => Items = items;
        public int Count => Items.Length;
        public CfdField this[int index] { get { Reads++; return Items[index]; } }
        public IEnumerator<CfdField> GetEnumerator()
        {
            for (var index = 0; index < Count; index++) yield return this[index];
        }
        IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
    }

    [Theory]
    [InlineData(16)]
    [InlineData(128)]
    public void WideRecordsAndDimensionViewsShareStableDescriptorLayout(int fieldCount)
    {
        var names = Enumerable.Range(0, fieldCount).Select(index => "f" + index).ToArray();
        var fields = new CountedFields(names.Select(name => (CfdField)new CfdField<int>(name, CfdValues.Dimension("language", CfdValues.Int))).ToArray());
        var type = new CfdType<Row>("Row", CfdTypeKind.Table, fields, _ => new Row(),
            (row, reader) => row.Values = names.Select(reader.Get<int>).ToArray(), null);
        Assert.Equal(fieldCount, fields.Reads);
        // 调用方后续修改原容器，不能令字段索引和遍历布局出现不同视图。
        fields.Items[0] = new CfdField<int>("changed", CfdValues.Int);
        var source = new StringBuilder();
        for (var record = 0; record < 64; record++)
        {
            source.Append("r").Append(record).Append(": Row {");
            foreach (var name in names) source.Append(name).Append(": 1,");
            source.Append("}\n");
        }
        source.Append("language[ja] { r0: Row { f0: 9 } } language[en] {}\n");
        var result = CfdLoader.Load(new[] { new CfdSource("rows.cfd", source.ToString()) }, new CfdType[] { type }, data => data);
        Assert.True(result.Success, string.Join("; ", result.Diagnostics));
        var first = result.Data.Table<Row>()["r0"];
        var last = result.Data.Table<Row>()["r63"];
        Assert.Equal(1, first.Values[0]);
        Assert.Equal(9, first.For("language", "ja").Values[0]);
        Assert.Equal(1, last.For("language", "ja").Values[0]);
        Assert.True(first.HasVariant("f0", "ja"));
        Assert.False(last.HasVariant("f0", "ja"));
        Assert.Same(first.Variants("language"), last.Variants("language"));
        Assert.Same(first.For("language", "ja"), first.For("language", "ja").For("language", "ja"));
        Assert.Equal(fieldCount, fields.Reads);
    }

    [Theory]
    [InlineData("r: Row { missing: 1 }", "CFD-FIELD")]
    [InlineData("r: Row { value: 1, value: 2 }", "CFD-SYNTAX-DUPLICATE-FIELD")]
    [InlineData("r: Row {}", "CFD-MISSING")]
    public void IndexedFieldsPreserveInvalidSourceDiagnostics(string source, string code)
    {
        var type = new CfdType<Row>("Row", CfdTypeKind.Table, new CfdField[] { new CfdField<int>("value", CfdValues.Int) },
            _ => new Row(), (row, reader) => row.Values = new[] { reader.Get<int>("value") }, null);
        var result = CfdLoader.Load(new[] { new CfdSource("bad.cfd", source) }, new CfdType[] { type }, data => data);
        Assert.False(result.Success);
        Assert.Contains(result.Diagnostics, diagnostic => diagnostic.Code == code && diagnostic.Path == "bad.cfd");
    }
}
