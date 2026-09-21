#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public class Profile : CoflowObject
{
    public string title { get; private set; } = default!;
    public global::Game.Config.Stats stats { get; private set; } = default!;
    public global::Game.Config.Character? owner { get; private set; } = default!;

    internal Profile(Record record) : base(record)
    {
        title = ValueCodecs.String(record.Field("title"));
        stats = new global::Game.Config.Stats(record.Field("stats"));
        owner = ValueCodecs.OptionalReference(record.Field("owner"), v0 => v0.Resolve<global::Game.Config.Character>());
    }

    public Profile(string title, global::Game.Config.Stats stats, global::Game.Config.Character? owner) : this(new Record(Projection.Data(global::Game.Config.Generated.ContractIdentity, "Profile", new[] { "title", "stats", "owner" }, new[] { Projection.From(title), Projection.From(stats), Projection.From(owner) }))) { }
}
}
