using Coflow.Cfd.Runtime;
using System;
using System.Linq;
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
            item: Item {
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

    [Fact]
    public void RejectsDuplicateIdentityAcrossDocuments()
    {
        var documents = CfdParser.ParseAll(new[]
        {
            new CfdSource("data/one.cfd", "Item { sword {} }"),
            new CfdSource("data/two.cfd", "Item { sword {} }"),
        });

        var error = Assert.Throws<CfdLoadException>(() => new CfdLoadContext(documents));
        Assert.Contains(error.Errors, diagnostic =>
            diagnostic.Code == "CFD-SYNTAX-DUPLICATE-RECORD" && diagnostic.Path == "data/two.cfd");
    }

    [Fact]
    public void ReportsUnknownFieldsAndConversionFailures()
    {
        var document = CfdParser.Parse(new CfdSource(
            "data/item.cfd",
            "Item { sword { name: \"Sword\", extra: 1, count: 999999999999999999999 } }"));
        var fields = document.Records[0].Fields;

        var unknown = Assert.Throws<CfdLoadException>(() => CfdValueReader.ValidateFields(fields, "name", "count"));
        Assert.Equal("CFD-FIELD-UNKNOWN", unknown.Errors[0].Code);
        Assert.NotNull(unknown.Errors[0].Span);

        var count = fields.Single(field => field.Name == "count").Value;
        var overflow = Assert.Throws<CfdLoadException>(() => { _ = CfdValueReader.Int32(count); });
        Assert.Equal("CFD-VALUE-NUMERIC", overflow.Errors[0].Code);

        var invalidEnum = Assert.Throws<CfdLoadException>(() => CfdValueReader.EnumText<TestElement>("Unknown"));
        Assert.Equal("CFD-VALUE-ENUM", invalidEnum.Errors[0].Code);

        var flags = CfdParser.Parse(new CfdSource("data/flags.cfd", "Item { sword { value: Fire|Ice } }"));
        var flagsValue = flags.Records[0].Fields[0].Value;
        Assert.Equal(TestFlags.Fire | TestFlags.Ice, CfdValueReader.Enum<TestFlags>(flagsValue));
    }

    private enum TestElement
    {
        Fire,
        Ice,
    }

    [Flags]
    private enum TestFlags
    {
        Fire = 1,
        Ice = 2,
    }
}
