#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public class DimensionBase : CoflowObject
{
    public string Id { get; private set; } = default!;
    public CoflowDimension<string> name { get; private set; } = default!;
    public CoflowDimension<string> hint { get; private set; } = default!;

    internal DimensionBase(Record record) : base(record)
    {
        Id = ValueCodecs.String(record.Field("id"));
        name = new CoflowDimension<string>(record.Field("name"), ValueCodecs.String);
        hint = new CoflowDimension<string>(record.Field("hint"), ValueCodecs.String);
    }
}
}
