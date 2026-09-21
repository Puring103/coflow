#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public class Hero : global::Game.Config.Character
{
    public int level { get; private set; } = default!;

    internal Hero(Record record) : base(record)
    {
        level = ValueCodecs.Int(record.Field("level"));
    }
}
}
