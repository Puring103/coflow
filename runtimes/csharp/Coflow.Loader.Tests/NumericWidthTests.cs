using System;
using System.Collections.Generic;
using Coflow.Loader;
using Xunit;

namespace Coflow.Loader.Tests;

public sealed class NumericWidthTests
{
    private sealed class Numbers : ICfdRecord
    {
        public CfdReader Reader { get; set; } = null!;
        internal int I;
        internal long L;
        internal float F;
        internal double D;
    }

    private static readonly CfdType[] Types = {
        new CfdType<Numbers>("Numbers", CfdTypeKind.Table, new CfdField[] {
            new CfdField<int>("i", CfdValues.Int, CfdDefaults.Int(0)),
            new CfdField<long>("l", CfdValues.Long, CfdDefaults.Long(0)),
            new CfdField<float>("f", CfdValues.Float, CfdDefaults.Float(0)),
            new CfdField<double>("d", CfdValues.Double, CfdDefaults.Double(0)),
        }, _ => new Numbers(), (value, reader) => {
            value.I = reader.Get<int>("i"); value.L = reader.Get<long>("l");
            value.F = reader.Get<float>("f"); value.D = reader.Get<double>("d");
        }, construct: null)
    };

    [Fact]
    public void ScalarsRespectDeclaredWidthAndPrecision()
    {
        var result = CfdLoader.Load(new[] { new CfdSource("values.cfd", "a: Numbers { i: 2147483647, l: 9223372036854775807, f: 0.1, d: 0.1 }") }, Types, data => data.Table<Numbers>()["a"]);
        Assert.True(result.Success);
        Assert.Equal(int.MaxValue, result.Data.I);
        Assert.Equal(long.MaxValue, result.Data.L);
        Assert.Equal(0.1f, result.Data.F);
        Assert.Equal(0.1d, result.Data.D);
        Assert.NotEqual(result.Data.D, (double)result.Data.F);
    }

    [Theory]
    [InlineData("i: 2147483648")]
    [InlineData("i: -2147483649")]
    [InlineData("f: 1e100")]
    [InlineData("i: WIDE")]
    [InlineData("f: PRECISE")]
    public void RejectsOverflowAndNarrowingConstants(string fields)
    {
        var constants = new Dictionary<string, CfdValueNode> {
            ["WIDE"] = CfdDefaults.Long(1), ["PRECISE"] = CfdDefaults.Double(0.1)
        };
        var result = CfdLoader.Load(new[] { new CfdSource("values.cfd", "a: Numbers { " + fields + " }") }, Types, data => data.Table<Numbers>()["a"], constants);
        Assert.False(result.Success);
    }
}
