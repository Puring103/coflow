using Coflow.Cfd.Runtime;
using Xunit;

namespace Coflow.Cfd.Runtime.Tests;

public sealed class ParserTests
{
    [Fact]
    public void ParsesGroupedRecordsAndNestedValues()
    {
        var document = CfdParser.Parse(new CfdSource("data/items.cfd", """
            Item {
              sword { name: "Fire Sword", tags: ["weapon", "melee"], target: &shield }
              shield { name: "Shield", stats: { armor: 10 } }
            }
            """));

        Assert.Equal(2, document.Records.Count);
        Assert.Equal("sword", document.Records[0].Key);
        Assert.IsType<CfdArrayValue>(document.Records[0].Fields[1].Value);
        Assert.IsType<CfdReferenceValue>(document.Records[0].Fields[2].Value);
    }

    [Fact]
    public void ReportsStableMissingSourceError()
    {
        var error = Assert.Throws<CfdLoadException>(() => CfdLoader.LoadDocuments(
            new DelegateCfdTextLoader(_ => null), new[] { "data/missing.cfd" }));
        Assert.Equal("CFD-SOURCE-MISSING", error.Errors[0].Code);
        Assert.Equal("data/missing.cfd", error.Errors[0].Path);
    }

    [Fact]
    public void ParsesExplicitAndBareObjectValues()
    {
        var document = CfdParser.Parse(new CfdSource("data/items.cfd", "sword: Item { stats: Stats { hp: 10 }, weights: { Fire: 2, Ice: 1 } }"));

        Assert.Single(document.Records);
        Assert.IsType<CfdObjectValue>(document.Records[0].Fields[0].Value);
        Assert.IsType<CfdDictionaryValue>(document.Records[0].Fields[1].Value);
    }

    [Fact]
    public void ReportsDuplicateFieldsAndRecordLimits()
    {
        var error = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("data/items.cfd", "Item { sword { name: 1, name: 2 } }")));
        Assert.Contains(error.Errors, diagnostic => diagnostic.Code == "CFD-SYNTAX-DUPLICATE-FIELD");

        var limited = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("data/items.cfd", "Item { sword {} shield {} }"),
            new CfdLoadOptions { MaxRecords = 1 }));
        Assert.Contains(limited.Errors, diagnostic => diagnostic.Code == "CFD-LIMIT-RECORDS");
    }

    [Fact]
    public void ReportsDuplicateRecordsAndSourceByteLimits()
    {
        var duplicate = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("data/items.cfd", "Item { sword {} } Item { sword {} }")));
        Assert.Contains(duplicate.Errors, diagnostic => diagnostic.Code == "CFD-SYNTAX-DUPLICATE-RECORD");

        var limited = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("data/items.cfd", "Item { sword {} }"),
            new CfdLoadOptions { MaxSourceBytes = 4 }));
        Assert.Contains(limited.Errors, diagnostic => diagnostic.Code == "CFD-LIMIT-SOURCE");
    }

    [Fact]
    public void ParsesCommentsEscapesUnicodeAndQuotedKeys()
    {
        var document = CfdParser.Parse(new CfdSource("data/items.cfd", """
            # a comment
            Item {
              "display-name": "line\n\u4e2d"
              note: "quoted \"text\""
            }
            """));

        Assert.Equal("display-name", document.Records[0].Fields[0].Name);
        Assert.Equal("line\n中", ((CfdStringValue)document.Records[0].Fields[0].Value).Value);
        Assert.Equal("quoted \"text\"", ((CfdStringValue)document.Records[0].Fields[1].Value).Value);
    }

    [Fact]
    public void ReportsDepthAndNodeLimitsWithStableCodes()
    {
        var error = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("data/items.cfd", "Item { item { values: [1, 2, 3] } }"),
            new CfdLoadOptions { MaxDepth = 1, MaxNodes = 2 }));
        Assert.Contains(error.Errors, diagnostic => diagnostic.Code == "CFD-LIMIT-DEPTH");
        Assert.Contains(error.Errors, diagnostic => diagnostic.Code == "CFD-LIMIT-NODES");
    }
}
