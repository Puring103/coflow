using System.Diagnostics;
using System.Runtime.CompilerServices;
using System.Text;
using System.Text.Json;
using Coflow.Loader;
const int Count = 500, Repeats = 2000;
var types = new CfdType[] { new CfdType<Row>("Row", CfdTypeKind.Table, new CfdField[] {
 new CfdField<int>("value", CfdValues.Int, dimension: "balance"),
 new CfdField<string>("name", CfdValues.String, dimension: "language"),
 new CfdField<string>("label", CfdValues.String, CfdDefaults.Template("f\"{self.name}/{self.value}\"")),
 new CfdField<IReadOnlyList<int>>("numbers", CfdValues.List(CfdValues.Int), CfdDefaults.Array(CfdDefaults.Int(1),CfdDefaults.Int(2))),
}, reader=>new Row(reader)) };
foreach(var density in new[]{"none","sparse","dense"}) {
 var source=new StringBuilder();
 for(int r=0;r<Count;r++) source.Append($"r{r}: Row {{ value: {r+1}, name: \"N{r}\" }}\n");
 foreach(var dimension in new[]{"balance","language"}) {
  source.Append(dimension+"[v] {\n");
  for(int r=0;r<(density=="dense"?Count:density=="sparse"?1:0);r++)
   source.Append(dimension=="balance"?$"r{r}: Row {{ value: {r+2} }}\n":$"r{r}: Row {{ name: \"V{r}\" }}\n");
  source.Append("}\n");
 }
 var sources=new[]{new CfdSource("matrix.cfd",source.ToString())};
 foreach(var kind in new[]{"int","string","template","list"}) {
  for(int sample=0;sample<9;sample++) {
   var loaded=CfdLoader.Load(sources,types,data=>data.Table<Row>());
   if(!loaded.Success) throw new Exception(string.Join(";",loaded.Diagnostics));
   var rows=loaded.Data.Values.Select(row=>row.For("language","v").For("balance","v")).ToArray();
   // 先验证并预热所有被测 getter，模板场景专门测量缓存读取。
   long expected=0;
   for(int r=0;r<Count;r++) {
    int value=r+1+((density=="dense"||density=="sparse"&&r==0)?1:0);
    string name=(density=="dense"||density=="sparse"&&r==0?"V":"N")+r;
    if(rows[r].Value!=value||rows[r].Name!=name||rows[r].Label!=name+"/"+value||rows[r].Numbers.Count!=2) throw new Exception("incorrect context values");
    expected+=kind switch {"int"=>rows[r].Value,"string"=>rows[r].Name.Length,"template"=>rows[r].Label.Length,_=>rows[r].Numbers.Count};
   }
   var result=Measure(rows,kind,Repeats,expected*Repeats);
   if(sample>=2) Console.WriteLine(JsonSerializer.Serialize(new {density,kind,result.readMs,result.allocated,reads=Count*Repeats}));
   GC.KeepAlive(loaded);
  }
 }
}
[MethodImpl(MethodImplOptions.NoInlining)]
static (double readMs,long allocated) Measure(Row[] rows,string kind,int repeats,long expected) {
 GC.Collect();GC.WaitForPendingFinalizers();GC.Collect();
 var sw=new Stopwatch();long before=GC.GetAllocatedBytesForCurrentThread(),sum=0;
 sw.Start();
 switch(kind) {
  case "int": for(int i=0;i<repeats;i++) foreach(var row in rows) sum+=row.Value;break;
  case "string": for(int i=0;i<repeats;i++) foreach(var row in rows) sum+=row.Name.Length;break;
  case "template": for(int i=0;i<repeats;i++) foreach(var row in rows) sum+=row.Label.Length;break;
  default: for(int i=0;i<repeats;i++) foreach(var row in rows) sum+=row.Numbers.Count;break;
 }
 sw.Stop();long allocated=GC.GetAllocatedBytesForCurrentThread()-before;
 if(sum!=expected) throw new Exception("wrong checksum");
 return(sw.Elapsed.TotalMilliseconds,allocated);
}
sealed class Row : ICfdRecord {
 internal Row(CfdReader reader)=>Reader=reader;
 public CfdReader Reader {get;}
 internal int Value=>Reader.Get<int>(0);
 internal string Name=>Reader.Get<string>(1);
 internal string Label=>Reader.Get<string>(2);
 internal IReadOnlyList<int> Numbers=>Reader.Get<IReadOnlyList<int>>(3);
}
