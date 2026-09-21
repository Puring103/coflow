#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public sealed class RuntimeSettings : RuntimeObject
{
    public string Id { get; private set; } = default!;
    public bool enabled { get; private set; } = default!;

    internal RuntimeSettings(Record record) : base(record)
    {
        Id = ValueCodecs.String(record.Field("id"));
        enabled = ValueCodecs.Bool(record.Field("enabled"));
    }
}
}
