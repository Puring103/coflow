using Coflow.Loader;

var types = new CfdType[]
{
    new CfdType<Entry>("Entry", CfdTypeKind.Table, new CfdField[]
    {
        new CfdField<string>("name", CfdValues.Dimension("language", CfdValues.String)),
    }, key => new Entry(key), (entry, reader) => entry.Name = reader.Get<string>("name"), construct: null),
};
var result = CfdLoader.Load(new[]
{
    new CfdSource("base.cfd", "sword: Entry { name: \"Sword\" }"),
    new CfdSource("ja.cfd", "language[ja] { sword: Entry { name: \"剣\" } }"),
}, types, data => new ProjectData(data.Table<Entry>()));
if (!result.Success) throw new Exception(string.Join(Environment.NewLine, result.Diagnostics));
if (result.Data.Entries["sword"].For("language", "ja").Name != "剣") throw new Exception("Variant loading failed.");
if (result.Data.Entries["sword"].For("language", "unknown").Name != "Sword") throw new Exception("Fallback failed.");
Console.WriteLine("netstandard2.1 direct CFD loading passed.");

sealed class Entry : ICfdRecord
{
    internal Entry(string id) { Id = id; }
    public CfdReader Reader { get; set; } = null!;
    public string Id { get; }
    public string Name { get; internal set; } = null!;
}
sealed class ProjectData
{
    internal ProjectData(IReadOnlyDictionary<string, Entry> entries) { Entries = entries; }
    public IReadOnlyDictionary<string, Entry> Entries { get; }
}
