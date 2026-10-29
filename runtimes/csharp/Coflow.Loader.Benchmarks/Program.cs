using System.Collections;
using System.Diagnostics;
using System.Text;
using System.Text.Json;
using Coflow.Loader;

// 构造阶段排除解析，全加载阶段包含解析、构造、变体验证及表注册；两者分别报告。
var recordCount = args.Length > 0 ? int.Parse(args[0]) : 500;
var repeats = args.Length > 1 ? int.Parse(args[1]) : 5;
// 时间采样使用生成代码实际传入的数组；计数容器只用于单独的工作量观测。
var countDescriptors = args.Length > 2 && args[2] == "count-descriptors";
foreach (var fieldCount in new[] { 32, 128, 512 })
foreach (var variantCount in new[] { 0, 3 })
{
    var names = Enumerable.Range(0, fieldCount).Select(index => "f" + index).ToArray();
    var descriptors = names.Select(name => (CfdField)new CfdField<int>(name,
        CfdValues.Dimension("language", CfdValues.Int))).ToArray();
    var countedFields = new CountedFields(descriptors);
    IReadOnlyList<CfdField> fields = countDescriptors ? countedFields : descriptors;
    var type = new CfdType<Row>("Row", CfdTypeKind.Table, fields, _ => new Row(),
        (row, reader) => row.Values = names.Select(reader.Get<int>).ToArray(), null);
    long? schemaReads = countDescriptors ? countedFields.Reads : null;
    var types = new CfdType[] { type };
    var text = new StringBuilder();
    for (var record = 0; record < recordCount; record++)
    {
        text.Append('r').Append(record).Append(": Row {");
        foreach (var name in names) text.Append(name).Append(": 1,");
        text.Append("}\n");
    }
    for (var variant = 0; variant < variantCount; variant++)
        text.Append("language[v").Append(variant).Append("] { r0: Row { f0: 2 } }\n");
    var sources = new[] { new CfdSource("rows.cfd", text.ToString()) };
    var documents = CfdParser.ParseAll(sources);
    foreach (var phase in new[] { "construction", "full-load" })
    {
        CfdData Run()
        {
            if (phase == "full-load")
            {
                var loaded = CfdLoader.Load(sources, types, data => data);
                if (!loaded.Success) throw new InvalidOperationException(string.Join("; ", loaded.Diagnostics));
                return loaded.Data;
            }
            var context = new LoadContext(documents, types, null);
            context.Build();
            return new CfdData(types, context.Records);
        }
        Run();
        var times = new List<double>();
        var allocated = new List<long>();
        var descriptorReads = new List<long>();
        for (var repeat = 0; repeat < repeats; repeat++)
        {
            GC.Collect();
            GC.WaitForPendingFinalizers();
            countedFields.Reads = 0;
            var before = GC.GetAllocatedBytesForCurrentThread();
            var watch = Stopwatch.StartNew();
            var data = Run();
            watch.Stop();
            allocated.Add(GC.GetAllocatedBytesForCurrentThread() - before);
            times.Add(watch.Elapsed.TotalMilliseconds);
            descriptorReads.Add(countedFields.Reads);
            if (data.Table<Row>().Count != recordCount || data.Table<Row>()["r0"].Values[0] != 1)
                throw new InvalidOperationException("Unexpected loaded values.");
        }
        Console.WriteLine(JsonSerializer.Serialize(new {
            recordCount, fieldCount, variantCount, phase, countDescriptors,
            schemaDescriptorReads = schemaReads, samplesMs = times, allocatedBytes = allocated,
            sourceDescriptorReads = countDescriptors ? descriptorReads : null,
        }));
    }
}

sealed class Row : ICfdRecord
{
    public CfdReader Reader { get; set; } = null!;
    internal int[] Values = Array.Empty<int>();
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
