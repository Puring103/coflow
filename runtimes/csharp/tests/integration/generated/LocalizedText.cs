#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public class LocalizedText : RuntimeObject
{
    public string Id { get; private set; } = default!;
    public RuntimeDimension<string> value { get; private set; } = default!;
    public RuntimeDimension<string?> optional { get; private set; } = default!;

    internal LocalizedText(Record record) : base(record)
    {
        Id = ValueCodecs.String(record.Field("id"));
        value = new RuntimeDimension<string>(record.Field("value"), ValueCodecs.String);
        optional = new RuntimeDimension<string?>(record.Field("optional"), v0 => ValueCodecs.OptionalReference(v0, ValueCodecs.String));
    }
}
}
