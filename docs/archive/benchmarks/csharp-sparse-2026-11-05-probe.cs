using System.Diagnostics;
using System.Runtime.CompilerServices;
using System.Text;
using System.Text.Json;
using Coflow.Loader;
const int Records = 500, Fields = 128, Variants = 12;
var fields = Enumerable.Range(0, Fields).Select(i => (CfdField)new CfdField<int>("f" + i,
#if LEGACY
CfdValues.Dimension("language", CfdValues.Int)
#else
CfdValues.Int, dimension: "language"
#endif
)).ToArray();
var type = new CfdType<Row>("Row", CfdTypeKind.Table, fields,
#if LEGACY
_ => new Row(), (row, reader) => row.Values = Enumerable.Range(0, Fields).Select(reader.Get<int>).ToArray(), null
#else
reader => new Row(reader)
#endif
);
var text = new StringBuilder();
for (var r = 0; r < Records; r++) {
 text.Append($"r{r}: Row {{");
 for (var f=0; f<Fields; f++) text.Append($"f{f}: 1,");
 text.Append("}\n");
}
for (var v=0; v<Variants; v++) text.Append($"language[v{v}] {{ r0: Row {{ f0: 2 }} }}\n");
var sources = new[]{new CfdSource("wide.cfd",text.ToString())};
var types=new CfdType[]{type};
for (int i=0;i<9;i++) { var sample=Run(sources,types,Records,Variants); if(i>=2) Console.WriteLine(JsonSerializer.Serialize(sample)); }
[MethodImpl(MethodImplOptions.NoInlining)]
static object Run(CfdSource[] sources,CfdType[] types,int count,int variants) {
 long before=GC.GetTotalMemory(true), allocated=GC.GetAllocatedBytesForCurrentThread();
 var sw=Stopwatch.StartNew();
 var loaded=CfdLoader.Load(sources,types,data=>data.Table<Row>());
 sw.Stop(); var loadMs=sw.Elapsed.TotalMilliseconds; var loadAllocated=GC.GetAllocatedBytesForCurrentThread()-allocated;
 if(!loaded.Success) throw new Exception(string.Join(";",loaded.Diagnostics));
 var rows=loaded.Data.Values.ToArray();
 var variantNames=Enumerable.Range(0,variants).Select(v=>"v"+v).ToArray();
 sw.Restart();
 foreach(var variant in variantNames) foreach(var row in rows) _=row.For("language",variant);
 sw.Stop(); var viewMs=sw.Elapsed.TotalMilliseconds;
 long retained=GC.GetTotalMemory(true)-before;
 var selected=rows.Select(row=>row.For("language","v0")).ToArray();
 sw.Restart(); long sum=0;
 for(int repeat=0;repeat<2000;repeat++) foreach(var row in selected) sum+=row.Read(0);
 sw.Stop(); var readMs=sw.Elapsed.TotalMilliseconds;
 if(sum!=1002000) throw new Exception("wrong values "+sum);
 GC.KeepAlive(loaded);
 return new {loadMs,loadAllocated,viewMs,retained,readMs,reads=1000000};
}
sealed class Row : ICfdRecord {
#if LEGACY
 public CfdReader Reader {get;set;}=null!;
 internal int[] Values=null!;
 internal int Read(int ordinal)=>Values[ordinal];
#else
 internal Row(CfdReader reader)=>Reader=reader;
 public CfdReader Reader {get;}
 internal int Read(int ordinal)=>Reader.Get<int>(ordinal);
#endif
}
