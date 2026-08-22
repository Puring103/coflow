using Coflow.Cfd.Runtime;
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using Xunit;

namespace Coflow.Cfd.Runtime.Tests;

public sealed class ParserTests
{
    [Fact]
    public void SharedCfdParserCorpusHasTheExpectedOutcomes()
    {
        var fixtureDirectory = Path.Combine(AppContext.BaseDirectory, "Fixtures");
        var fixtures = Directory.GetFiles(fixtureDirectory, "*.cfd").OrderBy(path => path).ToArray();
        Assert.NotEmpty(fixtures);

        foreach (var path in fixtures)
        {
            var name = Path.GetFileName(path);
            var expectedValid = name.EndsWith(".valid.cfd", StringComparison.Ordinal);
            Assert.True(expectedValid || name.EndsWith(".invalid.cfd", StringComparison.Ordinal),
                $"fixture name must declare its expected outcome: {name}");
            var error = Record.Exception(() => CfdParser.Parse(new CfdSource(name, File.ReadAllText(path))));
            Assert.Equal(expectedValid, error is null);
        }
    }

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
        Assert.Equal("Item", document.Records[0].GroupType);
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
              "display-name": "line\n中",
              note: "quoted \"text\"",
            }
            """));

        Assert.Equal("display-name", document.Records[0].Fields[0].Name);
        Assert.Equal("line\n中", ((CfdStringValue)document.Records[0].Fields[0].Value).Value);
        Assert.Equal("quoted \"text\"", ((CfdStringValue)document.Records[0].Fields[1].Value).Value);

        var unicodeEscape = Assert.Throws<CfdParseException>(() => CfdParser.Parse(new CfdSource(
            "data/invalid-escape.cfd",
            """Item { item { value: "\u4e2d" } }""")));
        Assert.Contains(unicodeEscape.Errors, diagnostic => diagnostic.Code == "CFD-SYNTAX-006");
    }

    [Fact]
    public void AppliesUnicodeIdentifierRulesToRecordKeys()
    {
        var document = CfdParser.Parse(new CfdSource(
            "data/unicode.cfd",
            "Item { e\u0301 {}, \U00010400key {} }"));
        Assert.Equal("e\u0301", document.Records[0].Key);
        Assert.Equal("\U00010400key", document.Records[1].Key);

        foreach (var key in new[] { "_", "id", "1item", "item-name" })
        {
            var error = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
                new CfdSource("data/invalid-key.cfd", $"Item {{ \"{key}\" {{}} }}")));
            Assert.Contains(error.Errors, diagnostic => diagnostic.Code == "CFD-SYNTAX-RECORD-KEY");
        }
    }

    [Fact]
    public void RejectsQuotedTypeNames()
    {
        foreach (var source in new[]
        {
            "item: \"Item\" {}",
            "\"Item\" { item {} }",
            "Item { item: \"Weapon\" {} }",
        })
        {
            var error = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
                new CfdSource("data/quoted-type.cfd", source)));
            Assert.Contains(error.Errors, diagnostic => diagnostic.Code == "CFD-SYNTAX-007");
        }
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
    public void CountsStructuralDepthAndNodesLikeTheRustCfdParser()
    {
        const string source = "Item { item { values: [1, 2, 3] } }";

        var accepted = CfdParser.Parse(
            new CfdSource("data/items.cfd", source),
            new CfdLoadOptions { MaxDepth = 2, MaxNodes = 7 });
        Assert.Single(accepted.Records);

        var depth = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("data/items.cfd", source),
            new CfdLoadOptions { MaxDepth = 1, MaxNodes = 7 }));
        Assert.Contains(depth.Errors, diagnostic => diagnostic.Code == "CFD-LIMIT-DEPTH");
        Assert.DoesNotContain(depth.Errors, diagnostic => diagnostic.Code == "CFD-LIMIT-NODES");

        var nodes = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("data/items.cfd", source),
            new CfdLoadOptions { MaxDepth = 2, MaxNodes = 6 }));
        Assert.DoesNotContain(nodes.Errors, diagnostic => diagnostic.Code == "CFD-LIMIT-DEPTH");
        Assert.Contains(nodes.Errors, diagnostic => diagnostic.Code == "CFD-LIMIT-NODES");

        var groupedRecord = CfdParser.Parse(
            new CfdSource("data/empty.cfd", "Item { item {} }"),
            new CfdLoadOptions { MaxDepth = 1, MaxNodes = 2 });
        Assert.Single(groupedRecord.Records);
    }

    [Fact]
    public void CountsBitExpressionNodesAndParenthesisDepth()
    {
        const string source = "Item { item { flags: (Fire | Ice) ^ Lightning } }";

        var accepted = CfdParser.Parse(
            new CfdSource("data/flags.cfd", source),
            new CfdLoadOptions { MaxDepth = 2, MaxNodes = 6 });
        Assert.Single(accepted.Records);

        var depth = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("data/flags.cfd", source),
            new CfdLoadOptions { MaxDepth = 1, MaxNodes = 6 }));
        Assert.Contains(depth.Errors, diagnostic => diagnostic.Code == "CFD-LIMIT-DEPTH");

        var nodes = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("data/flags.cfd", source),
            new CfdLoadOptions { MaxDepth = 2, MaxNodes = 5 }));
        Assert.Contains(nodes.Errors, diagnostic => diagnostic.Code == "CFD-LIMIT-NODES");
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
    public void RejectsRecordReferenceCyclesAndAcceptsAcyclicChainsLikeTheRustRuntime()
    {
        var cyclic = CfdParser.Parse(new CfdSource(
            "data/cycle.cfd",
            RuntimeFixture("record-reference-cycle.invalid.cfd")));
        var cyclicContext = new CfdLoadContext(
            new[] { cyclic },
            bindings: new ICfdTypeBinding[] { new NodeBinding() });
        var cycle = Assert.Throws<CfdLoadException>(() => cyclicContext.Resolve<Node>("Node", "a"));
        Assert.Equal("CFD-REF-CYCLE", cycle.Errors[0].Code);

        var acyclic = CfdParser.Parse(new CfdSource(
            "data/chain.cfd",
            RuntimeFixture("record-references.valid.cfd")));
        var acyclicContext = new CfdLoadContext(
            new[] { acyclic },
            bindings: new ICfdTypeBinding[] { new NodeBinding() });
        var first = acyclicContext.Resolve<Node>("Node", "a");
        Assert.Equal("b", first.Next?.Key);
        Assert.Null(first.Next?.Next);
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

    [Fact]
    public void RestrictsBooleansAndScalarConversionsToTheirCfdNodeKinds()
    {
        var document = CfdParser.Parse(new CfdSource("data/bools.cfd", "Item { item { value: true, other: false } }"));
        Assert.True(CfdValueReader.Boolean(document.Records[0].Fields[0].Value));
        Assert.False(CfdValueReader.Boolean(document.Records[0].Fields[1].Value));

        foreach (var text in new[] { "1", "0", "yes", "no", "y", "n", "TRUE", "False" })
        {
            var node = CfdParser.Parse(new CfdSource("data/bool.cfd", $"Item {{ item {{ value: {text} }} }}"))
                .Records[0].Fields[0].Value;
            var error = Assert.Throws<CfdLoadException>(() => CfdValueReader.Boolean(node));
            Assert.Equal("CFD-VALUE-BOOLEAN", error.Errors[0].Code);
        }

        var quoted = CfdParser.Parse(new CfdSource("data/quoted.cfd", "Item { item { value: \"1\" } }"))
            .Records[0].Fields[0].Value;
        Assert.Throws<CfdLoadException>(() => CfdValueReader.Int32(quoted));
    }

    [Fact]
    public void PreservesFormattedStringsAndEvaluatesFlagExpressions()
    {
        var document = CfdParser.Parse(new CfdSource(
            "data/values.cfd",
            "Item { item { name: \"Sword\", text: f\"value={name}\", nested: f\"<{text}>\", flags: (Fire | Ice) ^ Ice } }"));
        var fields = document.Records[0].Fields;
        var formatted = Assert.IsType<CfdFormattedStringValue>(fields[1].Value);
        Assert.Contains("name", formatted.Source);
        var context = new CfdLoadContext(new[] { document });
        using (context.EnterRecord("Item", "item"))
        {
            Assert.Equal("value=Sword", CfdValueReader.String(formatted, context));
            Assert.Equal("<value=Sword>", CfdValueReader.String(fields[2].Value, context));
        }
        Assert.Equal(TestFlags.Fire, CfdValueReader.Enum<TestFlags>(fields[3].Value));
    }

    [Fact]
    public void RejectsFormattedStringReferenceCycles()
    {
        var document = CfdParser.Parse(new CfdSource("data/cycle.cfd", "Item { item { text: f\"{text}\" } }"));
        var context = new CfdLoadContext(new[] { document });
        using (context.EnterRecord("Item", "item"))
        {
            var error = Assert.Throws<CfdLoadException>(() => CfdValueReader.String(
                document.Records[0].Fields[0].Value, context));
            Assert.Equal("CFD-VALUE-FORMAT", error.Errors[0].Code);
        }
    }

    [Fact]
    public void RejectsUnknownOrOutOfMaskFlagsAndAcceptsQualifiedEnumNames()
    {
        var qualified = CfdParser.Parse(new CfdSource("data/flags.cfd", "Item { item { value: TestFlags.Fire } }"));
        Assert.Equal(TestFlags.Fire, CfdValueReader.Enum<TestFlags>(qualified.Records[0].Fields[0].Value));

        var unknown = CfdParser.Parse(new CfdSource("data/flags.cfd", "Item { item { value: Fire|Unknown } }"));
        Assert.Throws<CfdLoadException>(() => CfdValueReader.Enum<TestFlags>(unknown.Records[0].Fields[0].Value));

        var outOfMask = CfdParser.Parse(new CfdSource("data/flags.cfd", "Item { item { value: 8 } }"));
        var error = Assert.Throws<CfdLoadException>(() => CfdValueReader.Enum<TestFlags>(outOfMask.Records[0].Fields[0].Value));
        Assert.Equal("CFD-VALUE-ENUM", error.Errors[0].Code);
    }

    [Fact]
    public void RequiresExplicitSeparatorsAndRejectsMixedArrayEntries()
    {
        var missingComma = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("data/invalid.cfd", "Item { item { first: 1 second: 2 } }")));
        Assert.Contains(missingComma.Errors, diagnostic => diagnostic.Code == "CFD-SYNTAX-009");

        var mixed = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("data/invalid.cfd", "Item { item { values: [one: 1] } }")));
        Assert.Contains(mixed.Errors, diagnostic => diagnostic.Code == "CFD-SYNTAX-011");

        var spaced = CfdParser.Parse(new CfdSource(
            "data/spaced.cfd",
            "Item { first { values: [1 , 2] , map: { one: 1 , two: 2 } } , second {} }"));
        Assert.Equal(2, spaced.Records.Count);
    }

    [Fact]
    public void AppliesRecordKeyRulesToReferencesAndBareObjectFields()
    {
        var invalidReference = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("data/invalid.cfd", "Item { item { target: &id } }")));
        Assert.Contains(invalidReference.Errors, diagnostic => diagnostic.Code == "CFD-SYNTAX-RECORD-KEY");

        var document = CfdParser.Parse(new CfdSource(
            "data/object.cfd",
            "Item { item { stats: { hp: 10, hp: 11 } } }"));
        var context = new CfdLoadContext(new[] { document });
        var error = Assert.Throws<CfdLoadException>(() => CfdValueReader.Object(
            document.Records[0].Fields[0].Value,
            context,
            "Stats",
            static (fields, _, _) => fields.Count));
        Assert.Equal("CFD-SYNTAX-DUPLICATE-FIELD", error.Errors[0].Code);

        var complexKey = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("data/invalid.cfd", "Item { item { values: { [one]: 1 } } }")));
        Assert.Contains(complexKey.Errors, diagnostic => diagnostic.Code == "CFD-SYNTAX-007");
    }

    [Fact]
    public void InterpretsBareBlocksAsObjectsWhenTheGeneratedReaderSuppliesTheExpectedType()
    {
        var document = CfdParser.Parse(new CfdSource(
            "data/object.cfd",
            "Item { item { stats: { hp: 10, armor: 2 } } }"));
        var node = document.Records[0].Fields[0].Value;
        var context = new CfdLoadContext(new[] { document });
        var fields = CfdValueReader.Object(node, context, "Stats",
            static (values, _, _) => values.Select(field => field.Name).ToArray());
        Assert.Equal(new[] { "hp", "armor" }, fields);
    }

    [Fact]
    public void ReaderDiagnosticsInheritTheOwningRecordSourcePath()
    {
        var document = CfdParser.Parse(new CfdSource(
            "data/path.cfd",
            "Node { root { next: 7 } }"));
        var context = new CfdLoadContext(new[] { document }, bindings: new[] { new NodeBinding() });

        var error = Assert.Throws<CfdLoadException>(() => context.Resolve<Node>("Node", "root"));

        Assert.Equal("data/path.cfd", error.Diagnostics[0].Path);
        Assert.NotNull(error.Diagnostics[0].Span);
    }

    private enum TestElement
    {
        Fire,
        Ice,
    }

    private static string RuntimeFixture(string name) =>
        File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "RuntimeFixtures", name));

    private sealed class Node
    {
        public Node(string key, Node? next)
        {
            Key = key;
            Next = next;
        }

        public string Key { get; }
        public Node? Next { get; }
    }

    private sealed class NodeBinding : ICfdTypeBinding
    {
        public string DeclaredType => "Node";
        public IReadOnlyList<string> AssignableTypes { get; } = new[] { "Node" };
        public string? ObjectFieldType(string fieldName) => null;
        public string? ReferenceFieldType(string fieldName) => fieldName == "next" ? "Node" : null;

        public object Read(CfdRecordNode record, CfdLoadContext context)
        {
            CfdValueReader.ValidateFields(record.Fields, "next");
            var next = CfdValueReader.FindField(record.Fields, "next");
            return new Node(
                record.Key,
                next is null or CfdNullValue
                    ? null
                    : CfdValueReader.Reference<Node>(next, context, "Node"));
        }
    }

    [Flags]
    private enum TestFlags
    {
        Fire = 1,
        Ice = 2,
    }
}
