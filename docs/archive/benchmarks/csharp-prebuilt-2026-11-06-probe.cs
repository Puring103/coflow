using System.Diagnostics;
using System.Text;
using System.Text.Json;
using Coflow.Loader;
const int Count=500, Repeats=2000;
var types=new CfdType[]{new CfdType<Row>("Row",CfdTypeKind.Table,new CfdField[]{
 new CfdField<int>("value",CfdValues.Int),
 new CfdField<string>("name",CfdValues.String,dimension:"language"),
 new CfdField<string>("label",CfdValues.String,CfdDefaults.Template("f\"prefix {self.name}\"")),
 new CfdField<IReadOnlyList<string>>("labels",CfdValues.List(CfdValues.String),CfdDefaults.Array(CfdDefaults.Template("f\"prefix {self.name}\""))),
 new CfdField<IReadOnlyDictionary<string,string>>("map",CfdValues.Dictionary(CfdValues.String,CfdValues.String),CfdDefaults.Dictionary(new CfdDefaultEntry(CfdDefaults.String("key"),CfdDefaults.Template("f\"prefix {self.name}\""))))
},_=>new Row(),(value,reader)=>{value.Value=reader.Get<int>(0);value.Name=reader.Get<string>(1);value.Label=reader.Get<string>(2);value.Labels=reader.Get<IReadOnlyList<string>>(3);value.Map=reader.Get<IReadOnlyDictionary<string,string>>(4);return value;})};
var text=new StringBuilder();
for(int i=0;i<Count;i++)text.Append($"r{i}: Row {{ value: {i+1}, name: \"N{i}\" }}\n");
text.Append("language[ja] { r0: Row { name: \"translated\" } }");
var loaded=CfdLoader.Load(new[]{new CfdSource("read.cfd",text.ToString())},types,data=>data);
if(!loaded.Success)throw new Exception(string.Join(";",loaded.Diagnostics));
var dimensions=Enumerable.Range(0,Count).Select(i=>loaded.Data.Table<Row>()["r"+i]).ToArray();
var rows=dimensions.Select(row=>row.For("language","ja")).ToArray();
for(int i=0;i<Count;i++) {
 var expected="prefix "+(i==0?"translated":"N"+i);
 if(rows[i].Value!=i+1||rows[i].Label!=expected||rows[i].Labels[0]!=expected||rows[i].Map["key"]!=expected)throw new Exception("incorrect values");
}
foreach(var kind in new[]{"scalar","template","list-template","dictionary-template","for"}) {
 long expected=Repeats*rows.Sum(row=>(long)(kind=="scalar"||kind=="for"?row.Value:row.Label.Length));
 for(int sample=0;sample<9;sample++) {
  long sum=0;var before=GC.GetAllocatedBytesForCurrentThread();var start=Stopwatch.GetTimestamp();
  switch(kind) {
   case "scalar": for(int j=0;j<Repeats;j++)foreach(var row in rows)sum+=row.Value;break;
   case "template": for(int j=0;j<Repeats;j++)foreach(var row in rows)sum+=row.Label.Length;break;
   case "list-template": for(int j=0;j<Repeats;j++)foreach(var row in rows)sum+=row.Labels[0].Length;break;
   case "dictionary-template": for(int j=0;j<Repeats;j++)foreach(var row in rows)sum+=row.Map["key"].Length;break;
   default: for(int j=0;j<Repeats;j++)foreach(var row in dimensions)sum+=row.For("language","ja").Value;break;
  }
  double ms=(Stopwatch.GetTimestamp()-start)*1000.0/Stopwatch.Frequency;long allocated=GC.GetAllocatedBytesForCurrentThread()-before;
  if(sum!=expected)throw new Exception("incorrect checksum");
  if(sample>=2)Console.WriteLine(JsonSerializer.Serialize(new{kind,reads=Count*Repeats,readMs=ms,allocated,checksum=sum}));
 }
}
sealed class Row { internal int Value;internal string Name="";internal string Label="";internal IReadOnlyList<string> Labels=null!;internal IReadOnlyDictionary<string,string> Map=null!; }
