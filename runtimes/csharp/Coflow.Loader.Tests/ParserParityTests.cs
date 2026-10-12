using System;
using System.IO;
using Coflow.Loader;
using Xunit;

namespace Coflow.Loader.Tests;

public sealed class ParserParityTests
{
    [Fact]
    public void SharedParserCorpusHasTheSameAcceptedAndRejectedInputs()
    {
        var fixtures = Directory.GetFiles(Path.Combine(AppContext.BaseDirectory, "parser-parity"), "*.cfd");
        Assert.NotEmpty(fixtures);
        foreach (var path in fixtures)
        {
            var expected = path.EndsWith(".valid.cfd", StringComparison.Ordinal);
            var error = Record.Exception(() => CfdParser.Parse(new CfdSource(path, File.ReadAllText(path))));
            if (expected) Assert.Null(error);
            else Assert.IsType<CfdParseException>(error);
        }
    }

    [Theory]
    [InlineData(true)]
    [InlineData(false)]
    public void EveryRecursiveConstructorUsesTheDepthLimit(bool optional)
    {
        // 使用有限输入验证限额，不用耗尽进程栈来检验递归保护。
        var value = optional ? "None" : "Read";
        for (var depth = 0; depth < 300; depth++) value = optional ? "Some(" + value + ")" : "(" + value + ")";
        var error = Assert.Throws<CfdParseException>(() => CfdParser.ParseValue(new CfdSource("depth.cfd", value)));
        Assert.Equal("CFD-LIMIT", error.Diagnostics[0].Code);
    }
}
