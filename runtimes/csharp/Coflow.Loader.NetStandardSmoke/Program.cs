using Coflow.Loader;

var types = new CfdType[]
{
    new CfdType<Entry, Entry>("Entry", new CfdField[]
    {
        new CfdField<DimensionValue<string>>("name", CfdValues.Dimension("language", CfdValues.String)),
    }, key => new Entry(key), (entry, reader) => entry.Name = reader.Get<DimensionValue<string>>("name"), reader => new Entry(null!) { Name = reader.Get<DimensionValue<string>>("name") }),
};
var result = CfdLoader.Load(new[]
{
    new CfdSource("base.cfd", "sword: Entry { name: \"Sword\" }"),
    new CfdSource("ja.cfd", "language[ja] { sword: Entry { name: \"剣\" } }"),
}, types, data => new ProjectData(data.Records<Entry>()));
if (!result.Success) throw new Exception(string.Join(Environment.NewLine, result.Diagnostics));
if (result.Data.Entries["sword"].Name.For("ja") != "剣") throw new Exception("Variant loading failed.");
if (result.Data.Entries["sword"].Name.For("unknown") != "Sword") throw new Exception("Fallback failed.");
Console.WriteLine("netstandard2.1 direct CFD loading passed.");

sealed class Entry
{
    internal Entry(string id) { Id = id; }
    public string Id { get; }
    public DimensionValue<string> Name { get; internal set; } = null!;
}
sealed class ProjectData
{
    internal ProjectData(IReadOnlyDictionary<string, Entry> entries) { Entries = entries; }
    public IReadOnlyDictionary<string, Entry> Entries { get; }
}
