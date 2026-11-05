using Coflow.Loader;

var types = new CfdType[]
{
    new CfdType<Entry>("Entry", CfdTypeKind.Table, new CfdField[]
    {
        new CfdField<string>("name", CfdValues.String, dimension: "language"),
    }, id=>new Entry { Id=id }, (value,reader)=>{value.Name=reader.Get<string>(0);return value;}),
};
var result = CfdLoader.Load(new[]
{
    new CfdSource("base.cfd", "sword: Entry { name: \"Sword\" }"),
    new CfdSource("ja.cfd", "language[ja] { sword: Entry { name: \"剣\" } }"),
}, types, data => new ProjectData(data.Table<Entry>()));
if (!result.Success) throw new Exception(string.Join(Environment.NewLine, result.Diagnostics));
if (result.Data.Entries["sword"].For("language", "ja").Name != "剣") throw new Exception("Variant loading failed.");
try { result.Data.Entries["sword"].For("language", "unknown"); throw new Exception("Unknown variant must fail."); }
catch (ArgumentException error) when (error.ParamName == "variant") { }
Console.WriteLine("netstandard2.1 direct CFD loading passed.");

sealed class Entry
{
    public string Id { get; internal set; } = "";
    public string Name { get; internal set; } = "";
}
sealed class ProjectData
{
    internal ProjectData(IReadOnlyDictionary<string, Dimension<Entry>> entries) { Entries = entries; }
    public IReadOnlyDictionary<string, Dimension<Entry>> Entries { get; }
}
