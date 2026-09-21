#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public class LocalizedText : CoflowObject
{
    public string Id { get; private set; } = default!;
    public CoflowDimension<string> value { get; private set; } = default!;
    public CoflowDimension<string?> optional { get; private set; } = default!;

    internal LocalizedText(Record record) : base(record)
    {
        Id = ValueCodecs.String(record.Field("id"));
        value = new CoflowDimension<string>(record.Field("value"), ValueCodecs.String);
        optional = new CoflowDimension<string?>(record.Field("optional"), v0 => ValueCodecs.OptionalReference(v0, ValueCodecs.String));
    }
}
}
