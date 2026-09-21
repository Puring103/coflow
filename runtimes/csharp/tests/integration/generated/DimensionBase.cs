#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public class DimensionBase : RuntimeObject
{
    public string Id { get; private set; } = default!;
    public RuntimeDimension<string> name { get; private set; } = default!;
    public RuntimeDimension<string> hint { get; private set; } = default!;

    internal DimensionBase(Record record) : base(record)
    {
        Id = ValueCodecs.String(record.Field("id"));
        name = new RuntimeDimension<string>(record.Field("name"), ValueCodecs.String);
        hint = new RuntimeDimension<string>(record.Field("hint"), ValueCodecs.String);
    }
}
}
