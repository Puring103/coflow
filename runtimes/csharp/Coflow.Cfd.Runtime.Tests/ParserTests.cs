using CoflowRuntime;
using CoflowRuntime.Generated;
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using Xunit;

namespace CoflowRuntime.Tests;

public sealed class ParserTests
{
    [Fact]
    public void ParsesNamespaceUsesAliasesAndQualifiedReferences()
    {
        var document = CfdParser.Parse(new CfdSource("rules.cfd", """
            namespace game::rules;
            use common::items::Item;
            use common::types::Element as Kind;

            Rule {
              combat {
                item: Item { target: &Item::sword },
                direct: &game::items::Item::shield,
                label: f"{&Item::sword.name}",
                element: Kind::Fire,
              }
            }
            """));

        Assert.Equal("game::rules", document.Namespace);
        Assert.Collection(document.Uses,
            item =>
            {
                Assert.Equal("common::items::Item", item.Path);
                Assert.Equal("Item", item.LocalName);
            },
            item =>
            {
                Assert.Equal("common::types::Element", item.Path);
                Assert.Equal("Kind", item.LocalName);
            });
        var fields = document.Records[0].Fields;
        Assert.Equal("Item", Assert.IsType<CfdObjectValue>(fields[0].Value).DeclaredType);
        var direct = Assert.IsType<CfdReferenceValue>(fields[1].Value);
        Assert.Equal("game::items::Item", direct.TypeName);
        Assert.Equal("shield", direct.Key);
        var formatted = Assert.IsType<CfdFormattedStringValue>(fields[2].Value);
        Assert.Equal("Item", Assert.IsType<CfdFormatReference>(formatted.Segments[0]).TypeName);
        Assert.Equal("Kind::Fire", Assert.IsType<CfdScalarValue>(fields[3].Value).Value);
    }

    [Fact]
    public void BindsCfdNamesWithoutRootFallback()
    {
        var document = CfdParser.Parse(new CfdSource("rules.cfd", """
            namespace game::rules;
            use common::items::Item;
            use common::types::Element as Kind;
            Rule {
              combat {
                item: Item { target: &Item::sword },
                direct: &game::items::Item::shield,
                label: f"{&Item::sword.name}",
                element: Kind::Fire,
                flags: Kind::Fire | Kind::Ice,
              }
            }
            """));

        var bound = CfdNameBinder.Bind(new[] { document }, new[]
        {
            "game::rules::Rule",
            "common::items::Item",
            "common::types::Element",
        });
        var record = Assert.Single(bound.Documents).Records[0];
        Assert.Equal("game::rules::Rule", record.DeclaredType);
        var fields = record.Fields;
        var item = Assert.IsType<CfdObjectValue>(fields[0].Value);
        Assert.Equal("common::items::Item", item.DeclaredType);
        Assert.Equal("common::items::Item", Assert.IsType<CfdReferenceValue>(item.Fields[0].Value).TypeName);
        Assert.Equal("game::items::Item", Assert.IsType<CfdReferenceValue>(fields[1].Value).TypeName);
        var formatted = Assert.IsType<CfdFormattedStringValue>(fields[2].Value);
        Assert.Equal("common::items::Item", Assert.IsType<CfdFormatReference>(formatted.Segments[0]).TypeName);
        Assert.Equal("common::types::Element::Fire", Assert.IsType<CfdScalarValue>(fields[3].Value).Value);
        var flags = Assert.IsType<CfdBitExpressionValue>(fields[4].Value).Expression;
        var binary = Assert.IsType<CfdBitExpressionKind.Binary>(flags.Kind);
        Assert.Equal("common::types::Element::Fire", Assert.IsType<CfdBitExpressionKind.Value>(binary.Left.Kind).Text);
        Assert.Equal("common::types::Element::Ice", Assert.IsType<CfdBitExpressionKind.Value>(binary.Right.Kind).Text);
    }

    [Fact]
    public void RejectsUnknownConflictingAndMisorderedUses()
    {
        var unknown = CfdParser.Parse(new CfdSource("unknown.cfd", "use missing::Item; Item { item {} }"));
        var unknownError = Assert.Throws<CfdLoadException>(() =>
            CfdNameBinder.Bind(new[] { unknown }, new[] { "Item" }));
        Assert.Equal("CFD-NAME-UNKNOWN-USE", unknownError.Diagnostics[0].Code);

        var conflict = CfdParser.Parse(new CfdSource("conflict.cfd", """
            namespace game;
            use common::Item;
            game::Rule { item {} }
            """));
        var conflictError = Assert.Throws<CfdLoadException>(() => CfdNameBinder.Bind(
            new[] { conflict },
            new[] { "common::Item", "game::Item", "game::Rule" }));
        Assert.Equal("CFD-NAME-USE-CONFLICT", conflictError.Diagnostics[0].Code);

        foreach (var source in new[]
        {
            "Item { item {} } namespace game;",
            "Item { item {} } use common::Item;",
            "use Item; Item { item {} }",
            "use common::*; Item { item {} }",
            "namespace game::; Item { item {} }",
        })
        {
            var syntax = Assert.Throws<CfdParseException>(() =>
                CfdParser.Parse(new CfdSource("invalid-header.cfd", source)));
            Assert.Contains(syntax.Diagnostics, item => item.Code is "CFD-SYNTAX-HEADER" or "CFD-SYNTAX-007");
        }
    }

    [Fact]
    public void LoadParserPreservesFunctionBodiesWithoutParsingExpressions()
    {
        var document = CfdParser.Parse(new CfdSource("rules.cfd", """
            Rule {
              combat {
                evaluate: fn(value: int) -> int {
                  # braces in comments do not close the body: }
                  var text = "}";
                  if value > 0 { value } else { 0 }
                },
              }
            }
            """));

        var function = Assert.IsType<CfdFunctionValue>(document.Records[0].Fields[0].Value);
        Assert.StartsWith("fn(value: int) -> int", function.Source);
        Assert.Contains("if value > 0", function.Source);
    }

    [Fact]
    public void LoadParserPreservesFunctionsReturningFunctions()
    {
        var document = CfdParser.Parse(new CfdSource("rules.cfd", """
            Rule {
              higherOrder {
                make: fn(scale: int) -> fn(int) -> int {
                  fn(value: int) -> int { value * scale }
                },
              }
            }
            """));

        var function = Assert.IsType<CfdFunctionValue>(document.Records[0].Fields[0].Value);
        Assert.Contains("-> fn(int) -> int", function.Source);
        Assert.Contains("value * scale", function.Source);
    }

    [Theory]
    [InlineData("Rule { item { value: fn(input: int) -> int } }", "CFD-FUNCTION-BODY")]
    [InlineData("Rule { item { value: fn(input: int) -> int { input", "CFD-FUNCTION-BODY")]
    [InlineData("Rule { item { value: fn(input: int -> int { input } } }", "CFD-FUNCTION-SIGNATURE")]
    public void RejectsMalformedFunctionSignaturesAndBodies(string source, string code)
    {
        var error = Assert.Throws<CfdParseException>(() =>
            CfdParser.Parse(new CfdSource("data/invalid-function.cfd", source)));

        Assert.Contains(error.Diagnostics, item => item.Code == code);
    }

    [Fact]
    public void ParsesExplicitOptionAndResultConstructorsAndRejectsNull()
    {
        var document = CfdParser.Parse(new CfdSource("values.cfd",
            "Value { item { none: None, some: Some(4), ok: Ok(\"yes\"), error: Err(7) } }"));
        var fields = document.Records[0].Fields;
        Assert.IsType<CfdNoneValue>(fields[0].Value);
        Assert.IsType<CfdSomeValue>(fields[1].Value);
        Assert.IsType<CfdOkValue>(fields[2].Value);
        Assert.IsType<CfdErrValue>(fields[3].Value);

        var error = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("null.cfd", "Value { item { value: null } }")));
        Assert.Contains(error.Diagnostics, diagnostic => diagnostic.Code == "CFD-SYNTAX-NULL");
    }

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
        Assert.Equal("CFD-SOURCE-MISSING", error.Diagnostics[0].Code);
        Assert.Equal("data/missing.cfd", error.Diagnostics[0].Path);
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
    public void ReportsDuplicateFields()
    {
        var error = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("data/items.cfd", "Item { sword { name: 1, name: 2 } }")));
        Assert.Contains(error.Diagnostics, diagnostic => diagnostic.Code == "CFD-SYNTAX-DUPLICATE-FIELD");
    }

    [Fact]
    public void ReportsDuplicateRecords()
    {
        var duplicate = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("data/items.cfd", "Item { sword {} } Item { sword {} }")));
        Assert.Contains(duplicate.Diagnostics, diagnostic => diagnostic.Code == "CFD-SYNTAX-DUPLICATE-RECORD");
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
        Assert.Contains(unicodeEscape.Diagnostics, diagnostic => diagnostic.Code == "CFD-SYNTAX-006");
    }

    [Fact]
    public void AppliesUnicodeIdentifierRulesToRecordKeys()
    {
        var document = CfdParser.Parse(new CfdSource(
            "data/unicode.cfd",
            "Item { e\u0301 {}, \U00010400key {} }"));
        Assert.Equal("e\u0301", document.Records[0].Key);
        Assert.Equal("\U00010400key", document.Records[1].Key);

        foreach (var key in new[]
        {
            "_", "id", "1item", "item-name", "namespace", "fn", "var", "return", "break",
            "continue", "None", "Some", "Ok", "Err", "Option", "Result", "Host", "alert", "records",
        })
        {
            var error = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
                new CfdSource("data/invalid-key.cfd", $"Item {{ \"{key}\" {{}} }}")));
            Assert.Contains(error.Diagnostics, diagnostic => diagnostic.Code == "CFD-SYNTAX-RECORD-KEY");
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
            Assert.Contains(error.Diagnostics, diagnostic => diagnostic.Code == "CFD-SYNTAX-007");
        }
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
        Assert.Contains(error.Diagnostics, diagnostic =>
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
        Assert.Equal("CFD-REF-CYCLE", cycle.Diagnostics[0].Code);

        var acyclic = CfdParser.Parse(new CfdSource(
            "data/chain.cfd",
            RuntimeFixture("record-references.valid.cfd")));
        var acyclicContext = new CfdLoadContext(
            new[] { acyclic },
            bindings: new ICfdTypeBinding[] { new NodeBinding() });
        var first = acyclicContext.Resolve<Node>("Node", "a");
        Assert.True(first.Next.TryGetValue(out var second));
        Assert.Equal("b", second.Key);
        Assert.False(second.Next.HasValue);
    }

    [Fact]
    public void SharedRuntimeCorpusLoadsComplexValuesLikeTheRustRuntime()
    {
        var document = CfdParser.Parse(new CfdSource(
            "data/complex.cfd", RuntimeFixture("complex-values.valid.cfd")));
        var fields = Assert.Single(document.Records).Fields;
        var context = new CfdLoadContext(new[] { document });

        CfdValueReader.ValidateFields(fields, "samples", "lookup", "optional", "outcome");
        Assert.Equal(new long[] { 1, 2, 3, 5, 8 },
            CfdValueReader.Array(CfdValueReader.Field(fields, "samples"), context,
                static (value, _) => CfdValueReader.Int64(value)));
        var lookup = CfdValueReader.Dictionary(CfdValueReader.Field(fields, "lookup"), context,
            static (value, load) => CfdValueReader.String(value, load),
            static (value, _) => CfdValueReader.Int64(value));
        Assert.Equal(2, lookup["two"]);
        var optional = CfdValueReader.Option(CfdValueReader.Field(fields, "optional"), context, ReadPayload);
        Assert.True(optional.HasValue);
        Assert.True(optional.Value.Enabled);
        Assert.Equal(13, optional.Value.Score);
        var outcome = CfdValueReader.Result(CfdValueReader.Field(fields, "outcome"), context,
            static (value, load) => CfdValueReader.Option(value, load, ReadPayload),
            static (value, load) => CfdValueReader.String(value, load));
        Assert.True(outcome.IsOk);
        Assert.True(outcome.Value.HasValue);
        Assert.False(outcome.Value.Value.Enabled);
        Assert.Equal(21, outcome.Value.Value.Score);
    }

    [Fact]
    public void SharedRuntimeCorpusRejectsUnknownFieldsLikeTheRustRuntime()
    {
        var document = CfdParser.Parse(new CfdSource(
            "data/complex-invalid.cfd", RuntimeFixture("complex-values.invalid.cfd")));
        var fields = Assert.Single(document.Records).Fields;

        var error = Assert.Throws<CfdLoadException>(() =>
            CfdValueReader.ValidateFields(fields, "samples", "lookup", "optional", "outcome"));
        Assert.Equal("CFD-FIELD-UNKNOWN", error.Diagnostics[0].Code);
    }

    [Fact]
    public void ReportsUnknownFieldsAndConversionFailures()
    {
        var document = CfdParser.Parse(new CfdSource(
            "data/item.cfd",
            "Item { sword { name: \"Sword\", extra: 1, count: 999999999999999999999 } }"));
        var fields = document.Records[0].Fields;

        var unknown = Assert.Throws<CfdLoadException>(() => CfdValueReader.ValidateFields(fields, "name", "count"));
        Assert.Equal("CFD-FIELD-UNKNOWN", unknown.Diagnostics[0].Code);
        Assert.NotNull(unknown.Diagnostics[0].Span);

        var count = fields.Single(field => field.Name == "count").Value;
        var overflow = Assert.Throws<CfdLoadException>(() => { _ = CfdValueReader.Int32(count); });
        Assert.Equal("CFD-VALUE-NUMERIC", overflow.Diagnostics[0].Code);

        var invalidEnum = Assert.Throws<CfdLoadException>(() => CfdValueReader.EnumText<TestElement>("Unknown"));
        Assert.Equal("CFD-VALUE-ENUM", invalidEnum.Diagnostics[0].Code);

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
            Assert.Equal("CFD-VALUE-BOOLEAN", error.Diagnostics[0].Code);
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
            Assert.Equal("CFD-VALUE-FORMAT", error.Diagnostics[0].Code);
        }
    }

    [Fact]
    public void RejectsUnknownOrOutOfMaskFlagsAndAcceptsQualifiedEnumNames()
    {
        var qualified = CfdParser.Parse(new CfdSource("data/flags.cfd", "Item { item { value: TestFlags::Fire } }"));
        Assert.Equal(TestFlags.Fire, CfdValueReader.Enum<TestFlags>(qualified.Records[0].Fields[0].Value));

        var legacy = CfdParser.Parse(new CfdSource("data/flags.cfd", "Item { item { value: TestFlags.Fire } }"));
        Assert.Throws<CfdLoadException>(() => CfdValueReader.Enum<TestFlags>(legacy.Records[0].Fields[0].Value));

        var unknown = CfdParser.Parse(new CfdSource("data/flags.cfd", "Item { item { value: Fire|Unknown } }"));
        Assert.Throws<CfdLoadException>(() => CfdValueReader.Enum<TestFlags>(unknown.Records[0].Fields[0].Value));

        var outOfMask = CfdParser.Parse(new CfdSource("data/flags.cfd", "Item { item { value: 8 } }"));
        var error = Assert.Throws<CfdLoadException>(() => CfdValueReader.Enum<TestFlags>(outOfMask.Records[0].Fields[0].Value));
        Assert.Equal("CFD-VALUE-ENUM", error.Diagnostics[0].Code);
    }

    [Fact]
    public void RequiresExplicitSeparatorsAndRejectsMixedArrayEntries()
    {
        var missingComma = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("data/invalid.cfd", "Item { item { first: 1 second: 2 } }")));
        Assert.Contains(missingComma.Diagnostics, diagnostic => diagnostic.Code == "CFD-SYNTAX-009");

        var mixed = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("data/invalid.cfd", "Item { item { values: [one: 1] } }")));
        Assert.Contains(mixed.Diagnostics, diagnostic => diagnostic.Code == "CFD-SYNTAX-011");

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
        Assert.Contains(invalidReference.Diagnostics, diagnostic => diagnostic.Code == "CFD-SYNTAX-RECORD-KEY");

        var document = CfdParser.Parse(new CfdSource(
            "data/object.cfd",
            "Item { item { stats: { hp: 10, hp: 11 } } }"));
        var context = new CfdLoadContext(new[] { document });
        var error = Assert.Throws<CfdLoadException>(() => CfdValueReader.Object(
            document.Records[0].Fields[0].Value,
            context,
            "Stats",
            static (fields, _, _) => fields.Count));
        Assert.Equal("CFD-SYNTAX-DUPLICATE-FIELD", error.Diagnostics[0].Code);

        var complexKey = Assert.Throws<CfdParseException>(() => CfdParser.Parse(
            new CfdSource("data/invalid.cfd", "Item { item { values: { [one]: 1 } } }")));
        Assert.Contains(complexKey.Diagnostics, diagnostic => diagnostic.Code == "CFD-SYNTAX-007");
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
    public void ParsesDeepCompositeValuesWithinTheSupportedRuntimeRange()
    {
        const int depth = 128;
        var nested = string.Concat(Enumerable.Repeat("Some(", depth)) + "1" +
            string.Concat(Enumerable.Repeat(")", depth));
        var document = CfdParser.Parse(new CfdSource(
            "data/deep.cfd", $"Item {{ item {{ value: {nested} }} }}"));

        CfdValueNode value = document.Records[0].Fields[0].Value;
        for (var level = 0; level < depth; level++)
            value = Assert.IsType<CfdSomeValue>(value).Value;
        Assert.Equal("1", Assert.IsType<CfdScalarValue>(value).Value);
    }

    [Fact]
    public void ParsesLargeRecordsAndCollectionsWithoutDroppingValues()
    {
        const int count = 1_000;
        var fields = string.Join(",", Enumerable.Range(0, count).Select(index => $"field{index}: {index}"));
        var values = string.Join(",", Enumerable.Range(0, count));
        var document = CfdParser.Parse(new CfdSource(
            "data/large.cfd", $"Item {{ item {{ {fields}, values: [{values}] }} }}"));

        var record = Assert.Single(document.Records);
        Assert.Equal(count + 1, record.Fields.Count);
        Assert.Equal(count, Assert.IsType<CfdArrayValue>(record.Fields[^1].Value).Items.Count);
    }

    [Fact]
    public void ReportsMultipleIndependentSyntaxErrorsInOneDocument()
    {
        var error = Assert.Throws<CfdParseException>(() => CfdParser.Parse(new CfdSource(
            "data/multiple-errors.cfd", "Item { one { value: null, value: 1 } one { other: null } }")));

        Assert.True(error.Diagnostics.Count >= 4);
        Assert.Equal(2, error.Diagnostics.Count(item => item.Code == "CFD-SYNTAX-NULL"));
        Assert.Contains(error.Diagnostics, item => item.Code == "CFD-SYNTAX-DUPLICATE-FIELD");
        Assert.Contains(error.Diagnostics, item => item.Code == "CFD-SYNTAX-DUPLICATE-RECORD");
        Assert.All(error.Diagnostics, item =>
        {
            Assert.Equal("data/multiple-errors.cfd", item.Path);
            Assert.NotNull(item.Span);
        });
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

    private static Payload ReadPayload(CfdValueNode node, CfdLoadContext context) =>
        CfdValueReader.Object(node, context, "Payload", static (fields, _, _) =>
        {
            CfdValueReader.ValidateFields(fields, "enabled", "score");
            return new Payload(
                CfdValueReader.Boolean(CfdValueReader.Field(fields, "enabled")),
                CfdValueReader.Int64(CfdValueReader.Field(fields, "score")));
        });

    private sealed record Payload(bool Enabled, long Score);

    private sealed class Node
    {
        public Node(string key, Option<Node> next)
        {
            Key = key;
            Next = next;
        }

        public string Key { get; }
        public Option<Node> Next { get; }
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
                next is null
                    ? Option<Node>.None
                    : CfdValueReader.Option(
                        next,
                        context,
                        static (value, loadContext) =>
                            CfdValueReader.Reference<Node>(value, loadContext, "Node")));
        }
    }

    [Flags]
    private enum TestFlags
    {
        Fire = 1,
        Ice = 2,
    }
}
