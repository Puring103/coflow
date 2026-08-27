using CoflowRuntime;
using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Xunit;

namespace CoflowRuntime.Tests;

public sealed class CoflowModuleTests
{
    [Fact]
    public void GeneratedRegistryCombinesMultipleFragmentsAndFreezesOnFirstLoad()
    {
        var registry = new CoflowGeneratedContractRegistry();
        var items = new TestModule(new ItemMetadata());
        var settings = new TestModule(new SettingsMetadata());
        registry.Register(items);
        registry.Register(settings);
        registry.Register(items);

        var resolved = registry.RequireContract();

        Assert.Equal(2, resolved.Types.Count);
        Assert.Throws<InvalidOperationException>(() =>
            registry.Register(new TestModule(new RuleMetadata())));
    }

    [Fact]
    public void MultipleGeneratedContractFragmentsLoadAsOneFixedContract()
    {
        var contract = new CoflowCompositeGeneratedContract(new ICoflowGeneratedContract[]
        {
            new TestModule(new ItemMetadata()),
            new TestModule(new SettingsMetadata()),
        });

        using var module = Coflow.LoadData(new[]
        {
            "Item { one { name: \"One\" } } settings: Settings { value: 42 }",
        }, contract);

        Assert.Equal("One", module.Table<Item>().Get("one").Value.Name);
        Assert.Equal(42, module.Singleton<Settings>().Value);
    }

    [Fact]
    public void CompositeGeneratedContractRejectsDuplicateGeneratedNames()
    {
        var contract = new CoflowCompositeGeneratedContract(new ICoflowGeneratedContract[]
        {
            new TestModule(new ItemMetadata()),
            new TestModule(new ItemMetadata()),
        });

        var error = Assert.Throws<CoflowLoadException>(() =>
            Coflow.LoadData(Array.Empty<string>(), contract));

        Assert.Contains(error.Diagnostics,
            diagnostic => diagnostic.Code == "COFLOW-METADATA-DUPLICATE-NAME");
    }

    [Fact]
    public void LoadDataBuildsStronglyTypedTablesAndReturnsOptionFromGet()
    {
        using var data = Coflow.LoadData(new[]
        {
            "Item { sword { name: \"Sword\" } shield { name: \"Shield\" } }",
        }, new TestModule(new ItemMetadata()));

        Assert.False(data.FunctionsCompiled);
        Assert.Equal(2, data.Table<Item>().Count);
        Assert.Equal("Sword", data.Table<Item>().Get("sword").Value.Name);
        Assert.False(data.Table<Item>().Get("missing").HasValue);
    }

    [Fact]
    public void EnumKeyTablesOnlyAcceptTheirAssociatedGeneratedEnum()
    {
        using var data = Coflow.LoadData(new[]
        {
            "EnumItem { Read { name: \"Readable\" } }",
        }, new TestModule(new EnumItemMetadata()));

        var table = data.Table<EnumItem>();
        Assert.Equal("Readable", table.Get(TestAccess.Read).Value.Name);
        Assert.False(table.Get(TestAccess.Write).HasValue);
        Assert.Throws<ArgumentException>(() => table.Get("Read"));
        Assert.Throws<ArgumentException>(() => table.Get(DayOfWeek.Monday));
    }

    [Fact]
    public void BaseTablesIncludeAssignableDerivedRecordsInSourceOrder()
    {
        using var data = Coflow.LoadData(new[]
        {
            "CoinReward { first { amount: 4 } }",
            "CoinReward { second { amount: 7 } }",
        }, new TestModule(new RewardMetadata(), new CoinRewardMetadata()));

        var rewards = data.Table<Reward>();
        Assert.Equal(new[] { "first", "second" }, rewards.Select(item => item.Id));
        Assert.Equal(7, Assert.IsType<CoinReward>(rewards.Get("second").Value).Amount);
    }

    [Fact]
    public void GenerationStorageUsesRowsWindowsAndHeapHandles()
    {
        using var data = Coflow.LoadData(new[]
        {
            "ValueRecord { value { optional: Some(9), outcome: Err(3) } }",
        }, new TestModule(new ValueRecordMetadata()));

        var table = Assert.Single(data.Storage.Tables);
        Assert.Equal(4, table.Layout.Width);
        Assert.Equal(new[] { 2, 2 }, table.Layout.Fields.Select(field => field.Width));
        Assert.Equal(CoflowSlotKind.Bool, table.Rows[0].Kind);
        Assert.Equal(CoflowSlotKind.Int64, table.Rows[1].Kind);
        Assert.Equal(CoflowSlotKind.Bool, table.Rows[2].Kind);
        Assert.Equal(CoflowSlotKind.Int64, table.Rows[3].Kind);
        Assert.Equal(0, data.Storage.Heap.Count);
    }

    [Fact]
    public void CompiledRecordFieldReadsUseTheGenerationRow()
    {
        var metadata = new RuleMetadata();
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { stored { offset: 12, evaluate: fn(value: int) -> int { offset + value } } }",
        }, new TestModule(metadata));
        var readsAfterCompilation = metadata.GetFieldCalls;

        Assert.Equal(17, data.Table<Rule>().Get("stored").Value.Evaluate(5));
        Assert.Equal(readsAfterCompilation, metadata.GetFieldCalls);
        Assert.Contains(metadata.LastRule!._evaluate.CompiledProgram!.Instructions,
            instruction => instruction.Code == CoflowOpCode.LoadField);
    }

    [Fact]
    public void RecordReferencesUseGenerationRelativeIdsAndVmReadsTheTargetRow()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Target { first { value: 4 } } Link { second { value: 9, next: &first, evaluate: fn(input: int) -> int { next.value + input } } }",
        }, new TestModule(new TargetMetadata(), new LinkMetadata()));

        var table = Assert.Single(data.Storage.Tables,
            item => item.Layout.Metadata.DeclaredType == "Link");
        var next = table.Layout.Field("next");
        Assert.Equal(1, next.Width);
        Assert.Equal(CoflowSlotKind.Record, table.Rows[next.Offset].Kind);
        Assert.Equal(7, data.Table<Link>().Get("second").Value.Evaluate(3));
    }

    [Fact]
    public void PolymorphicReferencesReuseTheConcreteRecordInstance()
    {
        using var data = Coflow.LoadData(new[]
        {
            "RewardHolder { holder { selected: &coin } } CoinReward { coin { amount: 8 } }",
        }, new TestModule(
            new RewardHolderMetadata(), new RewardMetadata(), new CoinRewardMetadata()));

        var selected = data.Table<RewardHolder>().Get("holder").Value.Selected;
        var concrete = data.Table<CoinReward>().Get("coin").Value;
        Assert.Same(concrete, selected);
        var holderStorage = Assert.Single(data.Storage.Tables,
            item => item.Layout.Metadata.DeclaredType == "RewardHolder");
        Assert.Equal(CoflowSlotKind.Record, holderStorage.Rows[0].Kind);
    }

    [Fact]
    public void NestedOptionReferencesEncodeTheirPayloadAsRecordIds()
    {
        using var data = Coflow.LoadData(new[]
        {
            "OptionalRewardHolder { holder { selected: Some(&coin) } } CoinReward { coin { amount: 8 } }",
        }, new TestModule(
            new OptionalRewardHolderMetadata(), new RewardMetadata(), new CoinRewardMetadata()));

        var holder = data.Table<OptionalRewardHolder>().Get("holder").Value;
        Assert.Same(data.Table<CoinReward>().Get("coin").Value, holder.Selected.Value);
        var storage = Assert.Single(data.Storage.Tables,
            item => item.Layout.Metadata.DeclaredType == "OptionalRewardHolder");
        Assert.Equal(CoflowSlotKind.Bool, storage.Rows[0].Kind);
        Assert.Equal(CoflowSlotKind.Record, storage.Rows[1].Kind);
    }

    [Fact]
    public void LoadDataBuildsSingletonsAndRejectsMissingSingletons()
    {
        var module = new TestModule(new SettingsMetadata());
        using var data = Coflow.LoadData(new[] { "settings: Settings { value: 42 }" }, module);
        Assert.Equal(42, data.Singleton<Settings>().Value);

        var error = Assert.Throws<CoflowLoadException>(() => Coflow.LoadData(Array.Empty<string>(), module));
        Assert.Contains(error.Diagnostics, diagnostic => diagnostic.Code == "CFD-SINGLETON-COUNT");
    }

    [Fact]
    public void LoadDataSkipsFunctionChecksAndKeepsFunctionsUnavailable()
    {
        using var data = Coflow.LoadData(new[]
        {
            "Rule { double { evaluate: fn(value: int) -> int { missing(value) + unknown } } }",
        }, new TestModule(new RuleMetadata()));

        var rule = data.Table<Rule>().Get("double").Value;
        Assert.Throws<CoflowFunctionNotCompiledException>(() => rule.Evaluate(2));
    }

    [Fact]
    public void OrdinaryRequiredFunctionsMayBeSkippedByLoadDataButMustCompileFromCfd()
    {
        var contract = new TestModule(new RuleMetadata(requireEvaluate: true));
        using var dataOnly = Coflow.LoadData(new[] { "Rule { missing { } }" }, contract);
        Assert.Throws<CoflowFunctionNotCompiledException>(() =>
            dataOnly.Table<Rule>().Get("missing").Value.Evaluate(1));

        var error = Assert.Throws<CoflowLoadException>(() =>
            Coflow.LoadAndCompile(new[] { "Rule { missing { } }" }, contract));
        Assert.Contains(error.Diagnostics, diagnostic => diagnostic.Code == "COFLOW-FUNCTION-MISSING");
    }

    [Fact]
    public void LoadAndCompileExecutesCfdBytecodeWithShortCircuiting()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { calculate { evaluate: fn(value: int) -> int { var adjusted: int = value + 2; if adjusted > 0 { adjusted * 3 } else { 0 } }, enabled: fn(value: int) -> bool { value == 0 || 10 // value > 1 } } }",
        }, new TestModule(new RuleMetadata()));

        Assert.True(data.FunctionsCompiled);
        var rule = data.Table<Rule>().Get("calculate").Value;
        Assert.Equal(18, rule.Evaluate(4));
        Assert.Equal(0, rule.Evaluate(-3));
        Assert.True(rule.Enabled(0));
    }

    [Fact]
    public void LoadAndCompileRejectsFunctionSignatureMismatch()
    {
        var error = Assert.Throws<CoflowLoadException>(() => Coflow.LoadAndCompile(new[]
        {
            "Rule { invalid { evaluate: fn(value: float) -> int { 1 } } }",
        }, new TestModule(new RuleMetadata())));

        Assert.Contains(error.Diagnostics, diagnostic => diagnostic.Code == "COFLOW-FUNCTION-SIGNATURE");
    }

    [Fact]
    public void FunctionDiagnosticsPointInsideTheFunctionBody()
    {
        var error = Assert.Throws<CoflowLoadException>(() => Coflow.LoadAndCompile(new[]
        {
            """
            Rule {
              invalid {
                evaluate: fn(value: int) -> int {
                  missing_name
                }
              }
            }
            """,
        }, new TestModule(new RuleMetadata())));

        var diagnostic = Assert.Single(error.Diagnostics,
            item => item.Code == "COFLOW-FUNCTION-NAME");
        Assert.Equal("source[0]", diagnostic.Path);
        Assert.NotNull(diagnostic.Span);
        Assert.Equal(5, diagnostic.Span.Value.StartLine);
        Assert.Equal(diagnostic.Span.Value.StartLine, diagnostic.Span.Value.EndLine);
    }

    [Fact]
    public void VmWrapsIntegerOverflowAsCoflowFault()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            """
            Rule {
              overflow {
                evaluate: fn(value: int) -> int {
                  value + 1
                }
              }
            }
            """,
        }, new TestModule(new RuleMetadata()));

        var error = Assert.Throws<CoflowFaultException>(() =>
            data.Table<Rule>().Get("overflow").Value.Evaluate(long.MaxValue));
        Assert.Equal("evaluate", error.Function.FieldName);
        Assert.Equal("source[0]", error.SourcePath);
        Assert.NotNull(error.SourceSpan);
        Assert.Equal(4, error.SourceSpan.Value.StartLine);
        Assert.Equal(new[] { "evaluate" }, error.CallStack.Select(frame => frame.FieldName));
    }

    [Fact]
    public void VmFaultUsesTheFailingNestedFunctionInstructionSpan()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            """
            Rule {
              base {
                evaluate: fn(value: int) -> int {
                  value + 1
                }
              }
              caller {
                evaluate: fn(value: int) -> int {
                  &Rule::base.evaluate(value) + 0
                }
              }
            }
            """,
        }, new TestModule(new RuleMetadata()));

        var error = Assert.Throws<CoflowFaultException>(() =>
            data.Table<Rule>().Get("caller").Value.Evaluate(long.MaxValue));

        Assert.Equal("base", error.Function.RecordKey);
        Assert.Equal(4, error.SourceSpan?.StartLine);
        Assert.Equal(new[] { "base", "caller" },
            error.CallStack.Select(frame => frame.RecordKey));
    }

    [Fact]
    public void HostSingletonCanOnlyBeBoundAfterLoadAndCompile()
    {
        var module = new TestModule(new HostServicesMetadata());
        using var uncompiled = Coflow.LoadData(Array.Empty<string>(), module);
        var unavailable = uncompiled.Singleton<HostServices>();
        Assert.Throws<CoflowHostNotBoundException>(() => unavailable.Environment);
        Assert.Equal(HostBindError.FunctionsNotCompiled,
            unavailable.Bind("test", _ => { }).Error);

        using var compiled = Coflow.LoadAndCompile(Array.Empty<string>(), module);
        var services = compiled.Singleton<HostServices>();
        string? logged = null;
        Assert.Equal(Unit.Value, services.Bind("development", text => logged = text).Value);
        Assert.Equal("development", services.Environment);
        services.Log("ready");
        Assert.Equal("ready", logged);
        Assert.Equal(HostBindError.AlreadyBound,
            services.Bind("other", _ => { }).Error);
    }

    [Fact]
    public void CfdCannotDeclareAHostRecord()
    {
        var error = Assert.Throws<CoflowLoadException>(() => Coflow.LoadData(
            new[] { "services: HostServices { environment: \"invalid\" }" },
            new TestModule(new HostServicesMetadata())));
        Assert.Contains(error.Diagnostics, diagnostic => diagnostic.Code == "CFD-HOST-RECORD");
    }

    [Fact]
    public void LoadDataMaterializesExplicitOptionAndResultValues()
    {
        using var data = Coflow.LoadData(new[]
        {
            "ValueRecord { ok { optional: Some(4), outcome: Ok(\"yes\") } error { optional: None, outcome: Err(7) } }",
        }, new TestModule(new ValueRecordMetadata()));

        var ok = data.Table<ValueRecord>().Get("ok").Value;
        Assert.Equal(4, ok.Optional.Value);
        Assert.Equal("yes", ok.Outcome.Value);
        var error = data.Table<ValueRecord>().Get("error").Value;
        Assert.False(error.Optional.HasValue);
        Assert.Equal(7, error.Outcome.Error);
    }

    [Fact]
    public void VmConstructsOptionAndResultReturnValues()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { some { choose: fn(value: int) -> Option<int> { Some(value + 1) }, validate: fn(value: int) -> Result<int, string> { Ok(value) } } empty { choose: fn(value: int) -> Option<int> { None }, validate: fn(value: int) -> Result<int, string> { Err(\"invalid\") } } }",
        }, new TestModule(new RuleMetadata()));

        var some = data.Table<Rule>().Get("some").Value;
        Assert.Equal(5, some.Choose(4).Value);
        Assert.Equal(4, some.Validate(4).Value);
        var none = data.Table<Rule>().Get("empty").Value;
        Assert.False(none.Choose(4).HasValue);
        Assert.Equal("invalid", none.Validate(4).Error);
    }

    [Fact]
    public void VmExecutesAssignmentLoopsAndEarlyReturn()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { flow { evaluate: fn(value: int) -> int { var current = 0; var total = 0; while current < value { current += 1; if current == 2 { continue; }; total += current; if total > 10 { break; }; } if total == 0 { return -1; }; total } } }",
        }, new TestModule(new RuleMetadata()));

        var rule = data.Table<Rule>().Get("flow").Value;
        Assert.Equal(-1, rule.Evaluate(0));
        Assert.Equal(8, rule.Evaluate(4));
        Assert.Equal(13, rule.Evaluate(10));
    }

    [Fact]
    public void VmInfersOptionAcrossIfBranches()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { flow { choose: fn(value: int) -> Option<int> { if value > 0 { Some(value) } else { None } } } }",
        }, new TestModule(new RuleMetadata()));

        Assert.Equal(3, data.Table<Rule>().Get("flow").Value.Choose(3).Value);
        Assert.False(data.Table<Rule>().Get("flow").Value.Choose(0).HasValue);
    }

    [Fact]
    public void VmUsesExplicitFramesForRecursiveRecordFunctions()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { recursive { evaluate: fn(value: int) -> int { if value <= 0 { 0 } else { evaluate(value - 1) + 1 } } } }",
        }, new TestModule(new RuleMetadata()));

        Assert.Equal(2000, data.Table<Rule>().Get("recursive").Value.Evaluate(2000));
    }

    [Fact]
    public void VmEliminatesTailRecursiveFrames()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { recursive { evaluate: fn(value: int) -> int { if value <= 0 { 0 } else { evaluate(value - 1) } } } }",
        }, new TestModule(new RuleMetadata()));

        Assert.Equal(0, data.Table<Rule>().Get("recursive").Value.Evaluate(10_000));
    }

    [Fact]
    public void VmBuildsCollectionsAndReturnsOptionFromIndexes()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { array { choose: fn(value: int) -> Option<int> { [value, value + 1][1] } } dictionary { choose: fn(value: int) -> Option<int> { {1: value}[1] } } missing { choose: fn(value: int) -> Option<int> { [value][4] } } }",
        }, new TestModule(new RuleMetadata()));

        Assert.Equal(5, data.Table<Rule>().Get("array").Value.Choose(4).Value);
        Assert.Equal(4, data.Table<Rule>().Get("dictionary").Value.Choose(4).Value);
        Assert.False(data.Table<Rule>().Get("missing").Value.Choose(4).HasValue);
    }

    [Fact]
    public void CompilerRejectsUnsupportedDictionaryKeyTypes()
    {
        var error = Assert.Throws<CoflowLoadException>(() => Coflow.LoadAndCompile(new[]
        {
            "Rule { invalid { evaluate: fn(value: int) -> int { var mapping = {true: value}; 0 } } }",
        }, new TestModule(new RuleMetadata())));

        Assert.Contains(error.Diagnostics, diagnostic =>
            diagnostic.Code == "COFLOW-FUNCTION-TYPE" &&
            diagnostic.Message.Contains("dictionary keys", StringComparison.Ordinal));
    }

    [Fact]
    public void VmStopsFunctionsThatExceedTheInstructionBudget()
    {
        using var budget = CoflowVm.OverrideInstructionLimitForCurrentThread(100);
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { infinite { evaluate: fn(value: int) -> int { while true { } 0 } } }",
        }, new TestModule(new RuleMetadata()));

        var error = Assert.Throws<CoflowFaultException>(() =>
            data.Table<Rule>().Get("infinite").Value.Evaluate(0));
        Assert.Contains("instruction budget", error.Message);
    }

    [Fact]
    public void LinearCollectionBuiltinsChargeTheInstructionBudget()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { total { evaluate: fn(value: int) -> int { [1, 2, 3, 4, 5, 6, 7, 8].sum() } } }",
        }, new TestModule(new RuleMetadata()));
        using var budget = CoflowVm.OverrideInstructionLimitForCurrentThread(15);

        var error = Assert.Throws<CoflowFaultException>(() =>
            data.Table<Rule>().Get("total").Value.Evaluate(0));

        Assert.Contains("instruction budget", error.Message);
    }

    [Fact]
    public void CompilerResolvesFieldsFromTheOwningGeneratedRecord()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { configured { offset: 7, evaluate: fn(value: int) -> int { value + offset } } }",
        }, new TestModule(new RuleMetadata()));

        Assert.Equal(12, data.Table<Rule>().Get("configured").Value.Evaluate(5));
    }

    [Fact]
    public void CompilerResolvesCrossRecordFieldsAndFunctions()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { base { offset: 3, evaluate: fn(value: int) -> int { value * 2 } } caller { evaluate: fn(value: int) -> int { &Rule::base.evaluate(value) + &base.offset } } }",
        }, new TestModule(new RuleMetadata()));

        Assert.Equal(13, data.Table<Rule>().Get("caller").Value.Evaluate(5));
    }

    [Fact]
    public void VmIndirectlyCallsFunctionValuesStoredInLocals()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { base { evaluate: fn(value: int) -> int { value * 2 } } caller { evaluate: fn(value: int) -> int { var operation = &base.evaluate; operation(value) + 1 } } }",
        }, new TestModule(new RuleMetadata()));

        Assert.Equal(11, data.Table<Rule>().Get("caller").Value.Evaluate(5));
    }

    [Fact]
    public void PersistentFunctionReferencesRemainCallableInsideVmCollections()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { base { evaluate: fn(value: int) -> int { value * 2 } } caller { evaluate: fn(value: int) -> int { var operations = [&Rule::base.evaluate]; match operations[0] { Some(operation) => operation(value), None => 0, } } } }",
        }, new TestModule(new RuleMetadata()));

        Assert.Equal(8, data.Table<Rule>().Get("caller").Value.Evaluate(4));
    }

    [Fact]
    public void VmIteratesArraysAndDictionariesInOrder()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { array { evaluate: fn(value: int) -> int { var total = 0; for item, index in [2, 4, 6] { total += item + index; } total } } dict { evaluate: fn(value: int) -> int { var total = 0; for key, item in {1: 2, 3: 4} { total += key * item; } total } } }",
        }, new TestModule(new RuleMetadata()));

        Assert.Equal(15, data.Table<Rule>().Get("array").Value.Evaluate(0));
        Assert.Equal(14, data.Table<Rule>().Get("dict").Value.Evaluate(0));
    }

    [Fact]
    public void VmPropagatesOptionAndResultFromTheCurrentFrame()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { option { choose: fn(value: int) -> Option<int> { var found = {1: value}[value]?; Some(found + 1) } } result { validate: fn(value: int) -> Result<int, string> { var checked = if value > 0 { Ok(value) } else { Err(\"invalid\") }?; Ok(checked + 1) } } }",
        }, new TestModule(new RuleMetadata()));

        var option = data.Table<Rule>().Get("option").Value;
        Assert.Equal(2, option.Choose(1).Value);
        Assert.False(option.Choose(2).HasValue);
        var result = data.Table<Rule>().Get("result").Value;
        Assert.Equal(3, result.Validate(2).Value);
        Assert.Equal("invalid", result.Validate(0).Error);
    }

    [Fact]
    public void VmMatchesOptionAndResultExhaustively()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { option { evaluate: fn(value: int) -> int { match Some(value) { Some(found) => found + 1, None => 0, } } } result { evaluate: fn(value: int) -> int { match if value > 0 { Ok(value) } else { Err(\"bad\") } { Ok(found) => found, Err(error) => 0, } } } }",
        }, new TestModule(new RuleMetadata()));

        Assert.Equal(4, data.Table<Rule>().Get("option").Value.Evaluate(3));
        Assert.Equal(3, data.Table<Rule>().Get("result").Value.Evaluate(3));
        Assert.Equal(0, data.Table<Rule>().Get("result").Value.Evaluate(0));
    }

    [Fact]
    public void VmMatchesFloatAndNegativeScalarPatterns()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { scalarMatch { evaluate: fn(value: int) -> int { match float(value) { -2.0 => 7, 1.0 => 8, _ => 0, } } } }",
        }, new TestModule(new RuleMetadata()));

        var rule = data.Table<Rule>().Get("scalarMatch").Value;
        Assert.Equal(7, rule.Evaluate(-2));
        Assert.Equal(8, rule.Evaluate(1));
        Assert.Equal(0, rule.Evaluate(3));
    }

    [Fact]
    public void ReloadAtomicallyPublishesDataAndFunctions()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { external { offset: 1, evaluate: fn(value: int) -> int { value + offset } } }",
        },
            new TestModule(new RuleMetadata()));
        var old = data.Table<Rule>().Get("external").Value;

        var reloaded = data.Reload(
            "Rule { external { offset: 2, evaluate: fn(value: int) -> int { value + offset } } }");

        Assert.True(reloaded.IsOk);
        Assert.Equal(2, reloaded.Value.Generation);
        Assert.Equal(6, old.Evaluate(5));
        var current = data.Table<Rule>().Get("external").Value;
        Assert.Equal(2, current.Offset);
        Assert.Equal(7, current.Evaluate(5));
    }

    [Fact]
    public void ReloadFailureKeepsThePublishedGeneration()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { external { offset: 1, evaluate: fn(value: int) -> int { value + offset } } }",
        },
            new TestModule(new RuleMetadata()));

        var failed = data.Reload(
            "Rule { replacement { offset: 9, evaluate: fn(value: float) -> int { 1 } } }");

        Assert.True(failed.IsErr);
        Assert.Contains(failed.Error.Diagnostics, item => item.Code == "COFLOW-FUNCTION-SIGNATURE");
        Assert.Equal(1, data.Table<Rule>().Get("external").Value.Offset);
        Assert.False(data.Table<Rule>().Get("replacement").HasValue);
    }

    [Fact]
    public void CombinedModulesCompileCrossModuleCallsAndReloadOneChildAtomically()
    {
        var contract = new TestModule(new RuleMetadata(requireEvaluate: true));
        using var parent = Coflow.LoadData(new[]
        {
            "Rule { parent { evaluate: fn(value: int) -> int { &Rule::child.evaluate(value) + 1 } } }",
        }, contract);
        using var child = Coflow.LoadData(new[]
        {
            "Rule { child { evaluate: fn(value: int) -> int { value * 2 } } }",
        }, contract);
        using var combinedData = Coflow.Combine(parent, child);
        Assert.False(combinedData.FunctionsCompiled);
        using var root = combinedData.Compile();
        Assert.Equal(7, root.Table<Rule>().Get("parent").Value.Evaluate(3));

        var reload = root.Reload(child,
            "Rule { child { evaluate: fn(value: int) -> int { value * 3 } } }");

        Assert.True(reload.IsOk);
        Assert.Equal(10, root.Table<Rule>().Get("parent").Value.Evaluate(3));
        Assert.True(root.Table<Rule>().Get("child").HasValue);
    }

    [Fact]
    public void CompositeChildCanBeLoadedAndReloadedRepeatedlyAsOneModule()
    {
        var contract = new TestModule(new RuleMetadata(requireEvaluate: true));
        using var left = Coflow.LoadData(new[]
        {
            "Rule { left { evaluate: fn(value: int) -> int { value + 1 } } }",
        }, contract);
        using var right = Coflow.LoadData(new[]
        {
            "Rule { right { evaluate: fn(value: int) -> int { value + 2 } } }",
        }, contract);
        using var child = CoflowModule.Combine(new[] { left, right });
        using var parent = Coflow.LoadData(new[]
        {
            "Rule { parent { evaluate: fn(value: int) -> int { &Rule::left.evaluate(value) + &Rule::right.evaluate(value) } } }",
        }, contract);
        using var combined = CoflowModule.Combine(new[] { child, parent });
        using var root = combined.Compile();

        Assert.Equal(9, root.Table<Rule>().Get("parent").Value.Evaluate(3));
        Assert.True(root.Reload(child,
            "Rule { left { evaluate: fn(value: int) -> int { value * 2 } } right { evaluate: fn(value: int) -> int { value * 3 } } }").IsOk);
        Assert.Equal(15, root.Table<Rule>().Get("parent").Value.Evaluate(3));

        Assert.True(root.Reload(child,
            "Rule { left { evaluate: fn(value: int) -> int { value * 4 } } right { evaluate: fn(value: int) -> int { value * 5 } } }").IsOk);
        Assert.Equal(27, root.Table<Rule>().Get("parent").Value.Evaluate(3));

        var duplicate = Assert.Throws<ArgumentException>(() => Coflow.Combine(root, child));
        Assert.Contains("more than once", duplicate.Message);
    }

    [Fact]
    public void CombineRejectsDuplicateModuleProvenance()
    {
        var contract = new TestModule(new ItemMetadata());
        using var module = Coflow.LoadData(new[] { "Item { one { name: \"One\" } }" }, contract);

        var error = Assert.Throws<ArgumentException>(() =>
            CoflowModule.Combine(new[] { module, module }));

        Assert.Contains("more than once", error.Message);
    }

    [Fact]
    public void ReloadCreatesANewHostGenerationAndTransfersStronglyTypedBindings()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { host { evaluate: fn(value: int) -> int { &HostServices.environment.len() } } }",
        }, new TestModule(new RuleMetadata(), new HostServicesMetadata()));
        var previousHost = data.Singleton<HostServices>();
        var logged = new List<string>();
        Assert.True(previousHost.Bind("development", logged.Add).IsOk);
        var previousRule = data.Table<Rule>().Get("host").Value;

        var reload = data.Reload(new[]
        {
            "Rule { host { evaluate: fn(value: int) -> int { &HostServices.environment.len() + value } } }",
        });

        Assert.True(reload.IsOk);
        var currentHost = data.Singleton<HostServices>();
        Assert.NotSame(previousHost, currentHost);
        Assert.Equal("development", currentHost.Environment);
        currentHost.Log("current");
        previousHost.Log("previous");
        Assert.Equal(new[] { "current", "previous" }, logged);
        Assert.Equal(12, data.Table<Rule>().Get("host").Value.Evaluate(1));
        Assert.Equal(11, previousRule.Evaluate(0));
    }

    [Fact]
    public async Task ReloadRetiresAnUnboundHostBeforeAWaitingBindCanSucceed()
    {
        using var transferEntered = new ManualResetEventSlim();
        using var releaseTransfer = new ManualResetEventSlim();
        var metadata = new HostServicesMetadata(() =>
        {
            transferEntered.Set();
            Assert.True(releaseTransfer.Wait(TimeSpan.FromSeconds(5)));
        });
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { host { evaluate: fn(value: int) -> int { value } } }",
        }, new TestModule(new RuleMetadata(), metadata));
        var previousHost = data.Singleton<HostServices>();

        var reload = Task.Run(() => data.Reload(
            "Rule { host { evaluate: fn(value: int) -> int { value + 1 } } }"));
        Assert.True(transferEntered.Wait(TimeSpan.FromSeconds(5)));

        using var bindStarted = new ManualResetEventSlim();
        var bind = Task.Run(() =>
        {
            bindStarted.Set();
            return previousHost.Bind("stale", _ => { });
        });
        Assert.True(bindStarted.Wait(TimeSpan.FromSeconds(5)));
        Assert.False(bind.IsCompleted);

        releaseTransfer.Set();
        Assert.True((await reload).IsOk);
        Assert.Equal(HostBindError.GenerationRetired, (await bind).Error);
        Assert.True(data.Singleton<HostServices>().Bind("current", _ => { }).IsOk);
    }

    [Fact]
    public async Task ActiveVmCallKeepsItsGenerationAcrossReload()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { active { offset: 1, evaluate: fn(value: int) -> int { &HostServices.log(\"wait\"); offset } } }",
        }, new TestModule(new RuleMetadata(), new HostServicesMetadata()));
        using var entered = new ManualResetEventSlim();
        using var release = new ManualResetEventSlim();
        var calls = 0;
        Assert.True(data.Singleton<HostServices>().Bind("development", _ =>
        {
            if (Interlocked.Increment(ref calls) != 1) return;
            entered.Set();
            release.Wait(TimeSpan.FromSeconds(5));
        }).IsOk);
        var previous = data.Table<Rule>().Get("active").Value;
        var activeCall = Task.Run(() => previous.Evaluate(0));
        Assert.True(entered.Wait(TimeSpan.FromSeconds(5)));

        var reload = data.Reload(
            "Rule { active { offset: 2, evaluate: fn(value: int) -> int { &HostServices.log(\"wait\"); offset } } }");
        Assert.True(reload.IsOk);
        release.Set();

        Assert.Equal(1, await activeCall);
        Assert.Equal(2, data.Table<Rule>().Get("active").Value.Evaluate(0));
    }

    [Fact]
    public void VmAnonymousFunctionsCaptureOuterLocalsByValue()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { closure { evaluate: fn(value: int) -> int { var scale = 3; var operation = fn(input: int) -> int { input * scale }; scale = 4; operation(value) } } }",
        }, new TestModule(new RuleMetadata()));

        Assert.Equal(15, data.Table<Rule>().Get("closure").Value.Evaluate(5));
    }

    [Fact]
    public void VmExecutesScalarAndCollectionBuiltins()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { builtins { evaluate: fn(value: int) -> int { [value, 2, 3].sum() + \"😀\".len() }, enabled: fn(value: int) -> bool { [1, 2, 3].isStrictlySorted() && \"coflow\".startsWith(\"co\") && {1: 2}.containsKey(1) } } }",
        }, new TestModule(new RuleMetadata()));

        var rule = data.Table<Rule>().Get("builtins").Value;
        Assert.Equal(10, rule.Evaluate(4));
        Assert.True(rule.Enabled(0));
    }

    [Fact]
    public void VmExecutesTheCompleteBuiltinMethodMatrix()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { matrix { evaluate: fn(value: int) -> int { [3, 1, 2].min() + [3, 1, 2].max() + [1, 2, 3].filter(fn(item: int) -> bool { item > 1 }).sum() + {1: 4, 2: 5}.keys().sum() + {1: 4, 2: 5}.values().sum() + int(9.0.sqrt()) + (-2).abs() }, enabled: fn(value: int) -> bool { [1, 2, 3].contains(2) && [1, 2, 3].isUnique() && [1, 2, 3].isSorted() && [1, 2, 3].intersects([3, 4]) && [1, 2].isDisjoint([3, 4]) && [1, 2].isSubsetOf([1, 2, 3]) && [1, 2, 3].isSupersetOf([2, 3]) && \"coflow\".contains(\"flow\") && \"coflow\".endsWith(\"flow\") && \"   \".isBlank() && \"abc123\".matches(\"[a-z]+[0-9]+\") && {1: 2}.contains(1) && {1: 2}.containsValue(2) && 4.0.isFinite() && 4.0.approxEqual(4.01, 0.02) }, choose: fn(value: int) -> Option<int> { [1, 2, 3].find(fn(item: int) -> bool { item == value }) } } }",
        }, new TestModule(new RuleMetadata()));

        var rule = data.Table<Rule>().Get("matrix").Value;
        Assert.Equal(26, rule.Evaluate(0));
        Assert.True(rule.Enabled(0));
        Assert.Equal(2, rule.Choose(2).Value);
        Assert.False(rule.Choose(8).HasValue);
    }

    [Fact]
    public void CompilerRequiresALiteralRegexPattern()
    {
        var error = Assert.Throws<CoflowLoadException>(() => Coflow.LoadAndCompile(new[]
        {
            "Rule { invalid { enabled: fn(value: int) -> bool { var pattern = \"a\"; \"abc\".matches(pattern) } } }",
        }, new TestModule(new RuleMetadata())));

        Assert.Contains(error.Diagnostics, diagnostic => diagnostic.Code == "COFLOW-FUNCTION-BUILTIN");
    }

    [Theory]
    [InlineData("(?=abc)")]
    [InlineData("(a)\\\\1")]
    public void CompilerRejectsRegexConstructsUnsupportedByRust(string pattern)
    {
        var error = Assert.Throws<CoflowLoadException>(() => Coflow.LoadAndCompile(new[]
        {
            $"Rule {{ invalid {{ enabled: fn(value: int) -> bool {{ \"abc\".matches(\"{pattern}\") }} }} }}",
        }, new TestModule(new RuleMetadata())));

        Assert.Contains(error.Diagnostics, diagnostic => diagnostic.Code == "COFLOW-FUNCTION-BUILTIN");
    }

    [Fact]
    public void VmAcceptsRustNamedCaptureSyntax()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { regex { enabled: fn(value: int) -> bool { \"abc\".matches(\"(?P<word>[a-z]+)\") } } }",
        }, new TestModule(new RuleMetadata()));

        Assert.True(data.Table<Rule>().Get("regex").Value.Enabled(0));
    }

    [Fact]
    public void CompilerRejectsExpressionsBeyondTheNestingLimit()
    {
        var operators = new string('!', 129);
        var error = Assert.Throws<CoflowLoadException>(() => Coflow.LoadAndCompile(new[]
        {
            $"Rule {{ invalid {{ enabled: fn(value: int) -> bool {{ {operators}true }} }} }}",
        }, new TestModule(new RuleMetadata())));

        Assert.Contains(error.Diagnostics, diagnostic => diagnostic.Code == "COFLOW-FUNCTION-LIMIT");
    }

    [Theory]
    [InlineData("var entries: [int] = []; var ignored = entries.min();")]
    [InlineData("var ignored = 4.0.approxEqual(4.0, -1.0);")]
    public void InvalidBuiltinRuntimeDomainsProduceVmFaults(string statements)
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            $"Rule {{ invalid {{ evaluate: fn(value: int) -> int {{ {statements} 0 }} }} }}",
        }, new TestModule(new RuleMetadata()));

        Assert.Throws<CoflowFaultException>(() => data.Table<Rule>().Get("invalid").Value.Evaluate(0));
    }

    [Fact]
    public void VmExecutesHigherOrderCollectionMethodsWithClosures()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { higher { evaluate: fn(value: int) -> int { var scale = 2; [1, 2, 3].map(fn(item: int) -> int { item * scale }).sum() + [1, 2, 3].fold(0, fn(total: int, item: int) -> int { total + item }) }, enabled: fn(value: int) -> bool { [1, 2, 3].any(fn(item: int) -> bool { item == value }) && [1, 2, 3].all(fn(item: int) -> bool { item > 0 }) } } }",
        }, new TestModule(new RuleMetadata()));

        var rule = data.Table<Rule>().Get("higher").Value;
        Assert.Equal(18, rule.Evaluate(0));
        Assert.True(rule.Enabled(2));
        Assert.False(rule.Enabled(5));
    }

    [Fact]
    public void VmExecutesNumericConversionsPowersAndBitOperators()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { operators { evaluate: fn(value: int) -> int { 2 ** 3 ** 2 + int(float(value)) + ((~0 & 7) << 1) } } }",
        }, new TestModule(new RuleMetadata()));

        Assert.Equal(530, data.Table<Rule>().Get("operators").Value.Evaluate(4));
    }

    [Fact]
    public void VmExecutesContinuousComparisons()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { range { enabled: fn(value: int) -> bool { 0 <= value < 10 } } }",
        }, new TestModule(new RuleMetadata()));

        var rule = data.Table<Rule>().Get("range").Value;
        Assert.True(rule.Enabled(5));
        Assert.False(rule.Enabled(-1));
        Assert.False(rule.Enabled(10));
    }

    [Fact]
    public void VmIteratesExclusiveAndInclusiveNumericRanges()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { ranges { evaluate: fn(value: int) -> int { var total = 0; for item in 1..4 { total += item; } for item in 4..=4 { total += item; } total } } }",
        }, new TestModule(new RuleMetadata()));

        Assert.Equal(10, data.Table<Rule>().Get("ranges").Value.Evaluate(0));
    }

    [Fact]
    public void CompilerResolvesGeneratedEnumsAndFlagOperations()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { enums { enabled: fn(value: int) -> bool { (TestAccess::Read | TestAccess::Write) == TestAccess(3) && TestAccess::Write > TestAccess::Read } } }",
        }, new TestModule(new ICoflowEnumMetadata[] { new TestAccessMetadata() }, new RuleMetadata()));

        Assert.True(data.Table<Rule>().Get("enums").Value.Enabled(0));
    }

    [Fact]
    public void VmConstructsGeneratedObjectsAndAppliesDefaults()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { object { evaluate: fn(value: int) -> int { var stats = Stats { hp: value }; stats.hp + stats.attack } } }",
        }, new TestModule(new RuleMetadata(), new StatsMetadata()));

        Assert.Equal(17, data.Table<Rule>().Get("object").Value.Evaluate(7));
    }

    [Fact]
    public void VmInterpolatesExpressionsAndEscapedBracesInStrings()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { text { describe: fn(value: int) -> string { \"value={value + 1}; literal={{name}}; option={Some(value)}; list={[value, 2]}; object={Stats { hp: value }}\" } } }",
        }, new TestModule(new RuleMetadata(), new StatsMetadata()));

        Assert.Equal(
            "value=4; literal={name}; option=Some(3); list=[3, 2]; object=Stats { hp: 3, attack: 10 }",
            data.Table<Rule>().Get("text").Value.Describe(3));
    }

    [Fact]
    public void CompilerRejectsFunctionInterpolation()
    {
        var error = Assert.Throws<CoflowLoadException>(() => Coflow.LoadAndCompile(new[]
        {
            "Rule { text { describe: fn(value: int) -> string { \"{evaluate}\" } } }",
        }, new TestModule(new RuleMetadata())));

        Assert.Contains(error.Diagnostics,
            diagnostic => diagnostic.Code == "COFLOW-FUNCTION-INTERPOLATION");
    }

    [Fact]
    public void CompilerRejectsInterpolationOfInlineObjectsContainingFunctions()
    {
        var error = Assert.Throws<CoflowLoadException>(() => Coflow.LoadAndCompile(new[]
        {
            "Rule { text { describe: fn(value: int) -> string { \"{callbacks}\" } } }",
        }, new TestModule(new RuleMetadata(), new StatsMetadata(includeFunctionField: true))));

        Assert.Contains(error.Diagnostics,
            diagnostic => diagnostic.Code == "COFLOW-FUNCTION-INTERPOLATION");
    }

    [Fact]
    public void NestedFunctionValuesExecuteInsideTheCurrentVmCallStack()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { nested { handlers: [fn(value: int) -> int { if value <= 0 { 0 } else { match handlers[0] { Some(handler) => handler(value - 1) + 1, None => 0, } } }], evaluate: fn(value: int) -> int { match handlers[0] { Some(handler) => handler(value), None => 0, } } } }",
        }, new TestModule(new RuleMetadata()));

        var rule = data.Table<Rule>().Get("nested").Value;
        Assert.Equal(5, rule.Evaluate(5));
        Assert.Equal(3, rule.Handlers[0](3));
    }

    [Fact]
    public void FunctionValuesUseContravariantParametersAndCovariantResults()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { variance { evaluate: fn(value: int) -> int { var transform: fn(CoinReward) -> Reward = fn(reward: Reward) -> CoinReward { match reward { CoinReward coin => coin, } }; var current = selected; if current is CoinReward { transform(current).$id.len() } else { 0 } } } }",
        }, new TestModule(new RuleMetadata(), new RewardMetadata(), new CoinRewardMetadata()));

        Assert.Equal(4, data.Table<Rule>().Get("variance").Value.Evaluate(0));
    }

    [Fact]
    public void FunctionVarianceAppliesRecursivelyAndDuringArrayInference()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { variance { evaluate: fn(value: int) -> int { var factory: fn(Reward) -> fn(CoinReward) -> Reward = fn(reward: Reward) -> fn(Reward) -> CoinReward { fn(inner: Reward) -> CoinReward { match inner { CoinReward coin => coin, } } }; var operations = [fn(reward: Reward) -> CoinReward { match reward { CoinReward coin => coin, } }, fn(reward: CoinReward) -> Reward { reward }]; var current = selected; if current is CoinReward { var total = 0; for operation in operations { total += operation(current).$id.len(); } factory(current)(current).$id.len() + total } else { 0 } } } }",
        }, new TestModule(new RuleMetadata(), new RewardMetadata(), new CoinRewardMetadata()));

        Assert.Equal(12, data.Table<Rule>().Get("variance").Value.Evaluate(0));
    }

    [Fact]
    public void CompilerRejectsFunctionVarianceInTheUnsafeDirection()
    {
        var error = Assert.Throws<CoflowLoadException>(() => Coflow.LoadAndCompile(new[]
        {
            "Rule { variance { evaluate: fn(value: int) -> int { var transform: fn(Reward) -> CoinReward = fn(reward: CoinReward) -> Reward { reward }; 0 } } }",
        }, new TestModule(new RuleMetadata(), new RewardMetadata(), new CoinRewardMetadata())));

        Assert.Contains(error.Diagnostics,
            diagnostic => diagnostic.Code == "COFLOW-FUNCTION-TYPE");
    }

    [Fact]
    public void CompilerRejectsFunctionValueComparison()
    {
        var error = Assert.Throws<CoflowLoadException>(() => Coflow.LoadAndCompile(new[]
        {
            "Rule { invalid { evaluate: fn(value: int) -> int { if evaluate == evaluate { 1 } else { 0 } } } }",
        }, new TestModule(new RuleMetadata())));

        Assert.Contains(error.Diagnostics, diagnostic =>
            diagnostic.Code == "COFLOW-FUNCTION-TYPE" &&
            diagnostic.Message.Contains("cannot be compared", StringComparison.Ordinal));
    }

    [Fact]
    public void VmUsesStructuralEqualityForCftValues()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { equality { enabled: fn(value: int) -> bool { var outcomeA: Result<[int], string> = Ok([1, 2]); var outcomeB: Result<[int], string> = Ok([1, 2]); [1, 2] == [1, 2] && {1: [2, 3]} == {1: [2, 3]} && Some([1, 2]) == Some([1, 2]) && outcomeA == outcomeB && Stats { hp: 1, attack: 2 } == Stats { hp: 1, attack: 2 } && selected == selected } } }",
        }, new TestModule(new RuleMetadata(), new StatsMetadata(),
            new RewardMetadata(), new CoinRewardMetadata()));

        Assert.True(data.Table<Rule>().Get("equality").Value.Enabled(0));
    }

    [Fact]
    public void VmFloatEqualityUsesIeeeNanSemantics()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { nan { enabled: fn(value: int) -> bool { var invalid = (-1.0).sqrt(); invalid != invalid && [invalid] != [invalid] && Some(invalid) != Some(invalid) } } }",
        }, new TestModule(new RuleMetadata()));

        Assert.True(data.Table<Rule>().Get("nan").Value.Enabled(0));
    }

    [Fact]
    public void CompilerRejectsNestedFunctionValueComparison()
    {
        var error = Assert.Throws<CoflowLoadException>(() => Coflow.LoadAndCompile(new[]
        {
            "Rule { invalid { evaluate: fn(value: int) -> int { if [evaluate] == [evaluate] { 1 } else { 0 } } } }",
        }, new TestModule(new RuleMetadata())));

        Assert.Contains(error.Diagnostics, diagnostic =>
            diagnostic.Code == "COFLOW-FUNCTION-TYPE" &&
            diagnostic.Message.Contains("cannot be compared", StringComparison.Ordinal));
    }

    [Theory]
    [InlineData("Stats { unknown: value }")]
    [InlineData("Stats { hp: value, hp: 2 }")]
    [InlineData("Stats { attack: value }")]
    public void CompilerRejectsInvalidObjectConstructors(string expression)
    {
        var error = Assert.Throws<CoflowLoadException>(() => Coflow.LoadAndCompile(new[]
        {
            $"Rule {{ object {{ evaluate: fn(value: int) -> int {{ var stats = {expression}; 0 }} }} }}",
        }, new TestModule(new RuleMetadata(), new StatsMetadata())));

        Assert.Contains(error.Diagnostics,
            diagnostic => diagnostic.Code == "COFLOW-FUNCTION-OBJECT");
    }

    [Fact]
    public void CompilerNarrowsGeneratedTypesInIsBranches()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { narrowing { evaluate: fn(value: int) -> int { var reward = selected; if reward is CoinReward { reward.amount } else { 0 } } } }",
        }, new TestModule(new RuleMetadata(), new RewardMetadata(), new CoinRewardMetadata()));

        Assert.Equal(4, data.Table<Rule>().Get("narrowing").Value.Evaluate(0));
    }

    [Fact]
    public void CompilerChecksExhaustiveSealedTypeMatches()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { matching { evaluate: fn(value: int) -> int { match selected { CoinReward reward => reward.amount, } } } }",
        }, new TestModule(new RuleMetadata(), new RewardMetadata(), new CoinRewardMetadata()));

        Assert.Equal(4, data.Table<Rule>().Get("matching").Value.Evaluate(0));
    }

    [Fact]
    public void HostFieldsAreReadAtExecutionAfterStronglyTypedBinding()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { host { evaluate: fn(value: int) -> int { &HostServices.environment.len() } } }",
        }, new TestModule(new RuleMetadata(), new HostServicesMetadata()));
        var services = data.Singleton<HostServices>();
        Assert.True(services.Bind("development", _ => { }).IsOk);

        Assert.Equal(11, data.Table<Rule>().Get("host").Value.Evaluate(0));
    }

    [Fact]
    public void VmClosuresAreAdaptedForStronglyTypedHostFunctionParameters()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { host { evaluate: fn(value: int) -> int { &HostServices.adjust(value, fn(item: int) -> int { item * 3 }) } } }",
        }, new TestModule(new RuleMetadata(), new HostServicesMetadata()));
        var services = data.Singleton<HostServices>();
        Assert.True(services.Bind("test", _ => { },
            (value, operation) => operation(value + 1) + 2).IsOk);

        Assert.Equal(14, data.Table<Rule>().Get("host").Value.Evaluate(3));
    }

    [Fact]
    public void ReentrantVmClosureFaultKeepsTheHostAndCfdCallChain()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { reentrant { evaluate: fn(value: int) -> int { &HostServices.adjust(value, fn(input: int) -> int { 1 / (input - input) }) } } }",
        }, new TestModule(new RuleMetadata(), new HostServicesMetadata()));
        Assert.True(data.Singleton<HostServices>().Bind(
            "development", _ => { }, static (value, operation) => operation(value)).IsOk);

        var error = Assert.Throws<CoflowFaultException>(() =>
            data.Table<Rule>().Get("reentrant").Value.Evaluate(4));

        Assert.Contains(error.CallStack, frame => frame.FieldName == "adjust");
        Assert.Contains(error.CallStack, frame => frame.FieldName == "evaluate");
    }

    [Fact]
    public void ReentrantVmCallsShareOneInstructionBudget()
    {
        using var budget = CoflowVm.OverrideInstructionLimitForCurrentThread(200);
        const string source =
            "Rule { reentrant { evaluate: fn(value: int) -> int { &HostServices.adjust(value, fn(input: int) -> int { input + 1 }) } } }";

        using var baseline = Coflow.LoadAndCompile(new[] { source },
            new TestModule(new RuleMetadata(), new HostServicesMetadata()));
        Assert.True(baseline.Singleton<HostServices>().Bind(
            "baseline", _ => { }, static (value, operation) => operation(value)).IsOk);
        Assert.Equal(2, baseline.Table<Rule>().Get("reentrant").Value.Evaluate(1));

        using var repeated = Coflow.LoadAndCompile(new[] { source },
            new TestModule(new RuleMetadata(), new HostServicesMetadata()));
        Assert.True(repeated.Singleton<HostServices>().Bind(
            "repeated", _ => { }, static (value, operation) =>
            {
                for (var index = 0; index < 100; index++) value = operation(value);
                return value;
            }).IsOk);

        var error = Assert.Throws<CoflowFaultException>(() =>
            repeated.Table<Rule>().Get("reentrant").Value.Evaluate(1));
        Assert.Contains("instruction budget", error.Message);
        Assert.Contains(error.CallStack, frame => frame.FieldName == "adjust");
    }

    [Fact]
    public void ReloadAndDisposeDoNotRetainOldGenerationRecords()
    {
        var (data, retiredRecords) = ReloadManyAndCaptureOldRecords();
        CollectGarbage();
        Assert.All(retiredRecords, reference => Assert.False(reference.IsAlive));
        data.Dispose();

        var disposedRecord = DisposeAndCaptureRecord();
        CollectGarbage();
        Assert.False(disposedRecord.IsAlive);

        var (closureModule, retiredClosure) = ReloadAndCaptureOldClosure();
        CollectGarbage();
        Assert.False(retiredClosure.IsAlive);
        closureModule.Dispose();
    }

    [Fact]
    public void DisposeRetiresUnboundHostObjectsAlreadyReturnedToTheCaller()
    {
        var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { current { evaluate: fn(value: int) -> int { value } } }",
        }, new TestModule(new RuleMetadata(), new HostServicesMetadata()));
        var host = data.Singleton<HostServices>();

        data.Dispose();

        Assert.Equal(HostBindError.GenerationRetired,
            host.Bind("disposed", _ => { }).Error);
    }

    [Fact]
    public void NamespacesAndUsesApplyInsideCompiledFunctions()
    {
        var module = new TestModule(
            new ICoflowEnumMetadata[] { new TestAccessMetadata("common::TestAccess") },
            new RuleMetadata("game::rules::Rule"));
        using var data = Coflow.LoadAndCompile(new[]
        {
            """
            namespace game::rules;
            use common::TestAccess as Access;

            Rule {
              target {
                offset: 7,
                evaluate: fn(value: int) -> int {
                  if Access::Read == Access::Read {
                    &Rule::target.offset + value
                  } else {
                    0
                  }
                },
                enabled: fn(value: int) -> bool {
                  match Access::Read {
                    Access::Read => true,
                    Access::Write => false,
                  }
                },
              }
            }
            """,
        }, module);

        Assert.Equal(12, data.Table<Rule>().Get("target").Value.Evaluate(5));
        Assert.True(data.Table<Rule>().Get("target").Value.Enabled(0));
    }

    [Fact]
    public void FunctionIdentifiersFollowUnicodeXidRules()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { unicode { evaluate: fn(𐐀: int) -> int { var e\u0301 = 𐐀 + 1; e\u0301 } } }",
        }, new TestModule(new RuleMetadata()));

        Assert.Equal(5, data.Table<Rule>().Get("unicode").Value.Evaluate(4));
    }

    [Fact]
    public void CompilerProvidesContextAndRecordMetadataStrings()
    {
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { configured { evaluate: fn(value: int) -> int { ($id + $path + $type + $field + $function + selected.$id + selected.$path).len() } } }",
        }, new TestModule(new RuleMetadata(), new RewardMetadata(), new CoinRewardMetadata()));

        var expected = "configuredRule::configuredRuleevaluateevaluatecoinCoinReward::coin";
        Assert.Equal(expected.Length,
            data.Table<Rule>().Get("configured").Value.Evaluate(0));
    }

    [Fact]
    public void CompilerResolvesGeneratedConstantsThroughNamespaceAndUse()
    {
        var module = new TestModule(
            Array.Empty<ICoflowEnumMetadata>(),
            new[]
            {
                new CoflowConstant("common::BASE", typeof(long), 3L),
                new CoflowConstant("game::rules::LOCAL", typeof(long), 4L),
            },
            new RuleMetadata("game::rules::Rule"));
        using var data = Coflow.LoadAndCompile(new[]
        {
            "namespace game::rules; use common::BASE as Imported; Rule { constants { evaluate: fn(value: int) -> int { Imported + LOCAL + common::BASE } } }",
        }, module);

        Assert.Equal(10, data.Table<Rule>().Get("constants").Value.Evaluate(0));
    }

    [Fact]
    public void LoadDataResolvesGeneratedConstantsThroughNamespaceAndUse()
    {
        var module = new TestModule(
            Array.Empty<ICoflowEnumMetadata>(),
            new[]
            {
                new CoflowConstant("common::DEFAULT_NAME", typeof(string), "Generated name"),
                new CoflowConstant("common::DEFAULT_VALUE", typeof(long), 42L),
            },
            new ItemMetadata(),
            new SettingsMetadata());
        using var data = Coflow.LoadData(new[]
        {
            "use common::DEFAULT_NAME as Name; Item { item { name: Name } } settings: Settings { value: common::DEFAULT_VALUE }",
        }, module);

        Assert.Equal("Generated name", data.Table<Item>().Get("item").Value.Name);
        Assert.Equal(42, data.Singleton<Settings>().Value);
    }

    [Fact]
    public void DeferredGeneratedConstantsResolveOncePerGenerationAndAreVisibleToFunctions()
    {
        var resolutions = 0;
        var module = new TestModule(
            Array.Empty<ICoflowEnumMetadata>(),
            new[]
            {
                new CoflowConstant("SELECTED", typeof(Item), context =>
                {
                    resolutions++;
                    return context.Resolve<Item>("Item", "sword");
                }),
            },
            new ItemMetadata(),
            new RuleMetadata());
        using var data = Coflow.LoadAndCompile(new[]
        {
            "Item { sword { name: \"Sword\" } } Rule { selected { evaluate: fn(value: int) -> int { SELECTED.$id.len() } } }",
        }, module);

        Assert.Equal(5, data.Table<Rule>().Get("selected").Value.Evaluate(0));
        Assert.Equal(5, data.Table<Rule>().Get("selected").Value.Evaluate(0));
        Assert.Equal(1, resolutions);

        Assert.True(data.Reload(new[]
        {
            "Item { sword { name: \"Blade\" } } Rule { selected { evaluate: fn(value: int) -> int { SELECTED.$id.len() } } }",
        }).IsOk);
        Assert.Equal(5, data.Table<Rule>().Get("selected").Value.Evaluate(0));
        Assert.Equal(2, resolutions);
    }

    [Theory]
    [InlineData("$unknown")]
    [InlineData("selected.$type")]
    public void CompilerRejectsUnknownOrInvalidMetadataNames(string expression)
    {
        var error = Assert.Throws<CoflowLoadException>(() => Coflow.LoadAndCompile(new[]
        {
            $"Rule {{ invalid {{ evaluate: fn(value: int) -> int {{ {expression}.len() }} }} }}",
        }, new TestModule(new RuleMetadata(), new RewardMetadata(), new CoinRewardMetadata())));

        Assert.Contains(error.Diagnostics,
            diagnostic => diagnostic.Code == "COFLOW-FUNCTION-METADATA");
    }

    [Theory]
    [InlineData("Rule { invalid { evaluate: fn(if: int) -> int { 0 } } }")]
    [InlineData("Rule { invalid { evaluate: fn(value: int) -> int { var return = value; 0 } } }")]
    [InlineData("Rule { invalid { evaluate: fn(value: int) -> int { for match in [1] { } 0 } } }")]
    [InlineData("Rule { invalid { evaluate: fn(value: int) -> int { [1].map(fn(while: int) -> int { 0 }).sum() } } }")]
    [InlineData("Rule { invalid { evaluate: fn(value: int) -> int { match Some(value) { Some(for) => 0, None => 0, } } } }")]
    public void FunctionBindingsRejectReservedIdentifiers(string source)
    {
        var error = Assert.Throws<CoflowLoadException>(() =>
            Coflow.LoadAndCompile(new[] { source }, new TestModule(new RuleMetadata())));

        Assert.Contains(error.Diagnostics, diagnostic => diagnostic.Code == "COFLOW-FUNCTION-NAME");
    }

    [Theory]
    [InlineData("Rule { invalid { evaluate: fn(offset: int) -> int { offset } } }")]
    [InlineData("Rule { invalid { evaluate: fn(value: int) -> int { var selected = value; selected } } }")]
    [InlineData("Rule { invalid { evaluate: fn(value: int) -> int { for enabled in [1] { } 0 } } }")]
    [InlineData("Rule { invalid { evaluate: fn(value: int) -> int { match Some(value) { Some(choose) => choose, None => 0, } } } }")]
    [InlineData("Rule { invalid { evaluate: fn(value: int) -> int { for item in [1] { item = 2; } 0 } } }")]
    [InlineData("Rule { invalid { evaluate: fn(value: int) -> int { match Some(value) { Some(item) => { item = 2; item }, None => 0, } } } }")]
    [InlineData("Rule { invalid { evaluate: fn(value: int) -> int { for item, item in [1] { } 0 } } }")]
    public void FunctionBindingsCannotShadowFieldsAndOnlyVarIsMutable(string source)
    {
        var error = Assert.Throws<CoflowLoadException>(() =>
            Coflow.LoadAndCompile(new[] { source }, new TestModule(new RuleMetadata())));

        Assert.Contains(error.Diagnostics, diagnostic =>
            diagnostic.Code is "COFLOW-FUNCTION-NAME" or "COFLOW-FUNCTION-ASSIGN");
    }

    private sealed class TestModule : ICoflowGeneratedContract
    {
        public TestModule(params ICoflowTypeMetadata[] types) : this(
            Array.Empty<ICoflowEnumMetadata>(), Array.Empty<CoflowConstant>(), types) { }
        public TestModule(IReadOnlyList<ICoflowEnumMetadata> enums, params ICoflowTypeMetadata[] types) : this(
            enums, Array.Empty<CoflowConstant>(), types) { }
        public TestModule(
            IReadOnlyList<ICoflowEnumMetadata> enums,
            IReadOnlyList<CoflowConstant> constants,
            params ICoflowTypeMetadata[] types)
        {
            Enums = enums;
            Constants = constants;
            Types = types;
        }
        public IReadOnlyList<ICoflowTypeMetadata> Types { get; }
        public IReadOnlyList<ICoflowEnumMetadata> Enums { get; }
        public IReadOnlyList<CoflowConstant> Constants { get; }
    }

    [Flags]
    private enum TestAccess : long { Read = 1, Write = 2 }

    private sealed class TestAccessMetadata : ICoflowEnumMetadata
    {
        private readonly string _declaredType;

        public TestAccessMetadata(string declaredType = "TestAccess") => _declaredType = declaredType;

        public string DeclaredType => _declaredType;
        public Type RuntimeType => typeof(TestAccess);
        public bool IsFlags => true;
        public IReadOnlyList<CoflowAnnotation> Annotations => Array.Empty<CoflowAnnotation>();
        public IReadOnlyDictionary<string, object> Variants { get; } =
            new Dictionary<string, object> { ["Read"] = TestAccess.Read, ["Write"] = TestAccess.Write };
        public IReadOnlyList<CoflowAnnotation> VariantAnnotations(string variantName) =>
            Array.Empty<CoflowAnnotation>();
        public object FromInt64(long value) => (TestAccess)value;
    }

    private sealed record Item(string Id, string Name);
    private sealed record EnumItem(TestAccess Id, string Name);
    private sealed record Settings(string Id, long Value);

    private sealed record Target(string Id, long Value);
    private sealed record RewardHolder(string Id, Reward Selected);
    private sealed record OptionalRewardHolder(string Id, Option<Reward> Selected);

    private sealed class Link
    {
        internal Link(string id, long value, Target next, CoflowFunctionSlot evaluate)
        {
            Id = id;
            Value = value;
            Next = next;
            _evaluate = evaluate;
        }

        internal readonly CoflowFunctionSlot _evaluate;
        public string Id { get; }
        public long Value { get; }
        public Target Next { get; }
        public long Evaluate(long input) => _evaluate.Invoke<long>(input);
    }

    private sealed class Rule
    {
        internal readonly CoflowFunctionSlot _evaluate;
        internal readonly CoflowFunctionSlot _enabled;
        internal readonly CoflowFunctionSlot _choose;
        internal readonly CoflowFunctionSlot _validate;
        internal readonly CoflowFunctionSlot _describe;

        internal Rule(
            string id,
            long offset,
            Reward selected,
            CoflowFunctionSlot evaluate,
            CoflowFunctionSlot enabled,
            CoflowFunctionSlot choose,
            CoflowFunctionSlot validate,
            CoflowFunctionSlot describe,
            IReadOnlyList<Func<long, long>> handlers)
        {
            Id = id;
            Offset = offset;
            Selected = selected;
            _evaluate = evaluate;
            _enabled = enabled;
            _choose = choose;
            _validate = validate;
            _describe = describe;
            Handlers = handlers;
        }

        public string Id { get; }
        public long Offset { get; }
        public Reward Selected { get; }
        public Stats Callbacks { get; } = new(0, 0);
        public long Evaluate(long value) => _evaluate.Invoke<long>(value);
        public bool Enabled(long value) => _enabled.Invoke<bool>(value);
        public Option<long> Choose(long value) => _choose.Invoke<Option<long>>(value);
        public Result<long, string> Validate(long value) =>
            _validate.Invoke<Result<long, string>>(value);
        public string Describe(long value) => _describe.Invoke<string>(value);
        public IReadOnlyList<Func<long, long>> Handlers { get; }
    }

    private sealed class HostServices
    {
        internal readonly CoflowHostSlot _host;
        internal readonly CoflowFunctionSlot _log;
        internal readonly CoflowFunctionSlot _adjust;
        internal string _environment = string.Empty;

        internal HostServices(CoflowHostSlot host, CoflowFunctionSlot log, CoflowFunctionSlot adjust)
        {
            _host = host;
            _log = log;
            _adjust = adjust;
        }

        public string Environment
        {
            get
            {
                _host.EnsureBound();
                return _environment;
            }
        }

        public void Log(string value)
        {
            _host.EnsureBound();
            _log.InvokeVoid(value);
        }

        public Result<Unit, HostBindError> Bind(string environment, Action<string> log) =>
            Bind(environment, log, static (value, operation) => operation(value));

        public Result<Unit, HostBindError> Bind(
            string environment,
            Action<string> log,
            Func<long, Func<long, long>, long> adjust) =>
            _host.Bind(
                () => _environment = environment,
                new CoflowHostFunctionBinding(_log, log),
                new CoflowHostFunctionBinding(_adjust, adjust));
    }

    private sealed record ValueRecord(
        string Id,
        Option<long> Optional,
        Result<string, long> Outcome);
    private sealed record Stats(long Hp, long Attack);
    private abstract record Reward(string Id);
    private sealed record CoinReward(string Id, long Amount) : Reward(Id);

    private abstract class Metadata<T> : ICoflowTypeMetadata
    {
        public Type RuntimeType => typeof(T);
        public virtual Type KeyType => typeof(string);
        public virtual bool IsSingleton => false;
        public virtual bool IsHost => false;
        public virtual bool IsAbstract => false;
        public virtual bool IsSealed => true;
        public virtual bool IsRecord => true;
        public virtual IReadOnlyList<CoflowAnnotation> Annotations => Array.Empty<CoflowAnnotation>();
        public abstract string DeclaredType { get; }
        public virtual IReadOnlyList<string> AssignableTypes => new[] { DeclaredType };
        public virtual IReadOnlyList<string> FieldNames => Array.Empty<string>();
        public virtual IReadOnlyList<CoflowAnnotation> FieldAnnotations(string fieldName) =>
            Array.Empty<CoflowAnnotation>();
        public virtual Type GetFieldType(string fieldName) =>
            throw new ArgumentException($"unknown field `{fieldName}`", nameof(fieldName));
        public virtual object GetField(object record, string fieldName) =>
            throw new ArgumentException($"unknown field `{fieldName}`", nameof(fieldName));
        public virtual bool HasFieldDefault(string fieldName) => false;
        public virtual object CreateObject(
            CfdLoadContext context,
            IReadOnlyDictionary<string, object?> fields) =>
            throw new InvalidOperationException("Test type cannot be constructed as an object.");
        public virtual string? ObjectFieldType(string fieldName) => null;
        public virtual string? ReferenceFieldType(string fieldName) => null;
        public virtual object ParseKey(string key) => key;
        public virtual object CreateHost(CfdLoadContext context) =>
            throw new InvalidOperationException("Test type is not @Host.");
        public virtual void TransferHostState(object source, object target) =>
            throw new InvalidOperationException("Test type is not @Host.");
        public object GetKey(object record) => record switch
        {
            Item item => item.Id,
            EnumItem item => item.Id,
            Settings settings => settings.Id,
            Link link => link.Id,
            Target target => target.Id,
            RewardHolder holder => holder.Id,
            OptionalRewardHolder holder => holder.Id,
            Rule rule => rule.Id,
            ValueRecord value => value.Id,
            Reward reward => reward.Id,
            _ => throw new ArgumentException("Unexpected record type.", nameof(record)),
        };
        public abstract object Read(CfdRecordNode record, CfdLoadContext context);
    }

    private sealed class ItemMetadata : Metadata<Item>
    {
        public override string DeclaredType => "Item";

        public override object Read(CfdRecordNode record, CfdLoadContext context) => new Item(
            record.Key,
            CfdValueReader.String(CfdValueReader.Field(record.Fields, "name"), context));
    }

    private sealed class EnumItemMetadata : Metadata<EnumItem>
    {
        public override string DeclaredType => "EnumItem";
        public override Type KeyType => typeof(TestAccess);
        public override object ParseKey(string key) => key switch
        {
            "Read" => TestAccess.Read,
            "Write" => TestAccess.Write,
            _ => throw new ArgumentException(nameof(key)),
        };
        public override object Read(CfdRecordNode record, CfdLoadContext context) => new EnumItem(
            (TestAccess)ParseKey(record.Key),
            CfdValueReader.String(CfdValueReader.Field(record.Fields, "name"), context));
    }

    private sealed class SettingsMetadata : Metadata<Settings>
    {
        public override string DeclaredType => "Settings";
        public override bool IsSingleton => true;

        public override object Read(CfdRecordNode record, CfdLoadContext context) => new Settings(
            record.Key,
            CfdValueReader.Int64(CfdValueReader.Field(record.Fields, "value")));
    }

    private sealed class LinkMetadata : Metadata<Link>
    {
        public override string DeclaredType => "Link";
        public override IReadOnlyList<string> FieldNames => new[] { "value", "next", "evaluate" };
        public override Type GetFieldType(string fieldName) => fieldName switch
        {
            "value" => typeof(long),
            "next" => typeof(Target),
            "evaluate" => typeof(CoflowFunctionSlot),
            _ => throw new ArgumentException(nameof(fieldName)),
        };
        public override string? ReferenceFieldType(string fieldName) => fieldName == "next" ? "Target" : null;
        public override object GetField(object record, string fieldName) => fieldName switch
        {
            "value" => ((Link)record).Value,
            "next" => ((Link)record).Next!,
            "evaluate" => ((Link)record)._evaluate,
            _ => throw new ArgumentException(nameof(fieldName)),
        };
        public override object Read(CfdRecordNode record, CfdLoadContext context)
        {
            using var scope = context.EnterRecord(record.DeclaredType, record.Key);
            CfdValueReader.ValidateFields(record.Fields, "value", "next", "evaluate");
            return new Link(
                record.Key,
                CfdValueReader.Int64(CfdValueReader.Field(record.Fields, "value")),
                CfdValueReader.Reference<Target>(
                    CfdValueReader.Field(record.Fields, "next"), context, "Target"),
                context.Function(CfdValueReader.FindField(record.Fields, "evaluate"),
                    "evaluate", typeof(long), typeof(long)));
        }
    }

    private sealed class TargetMetadata : Metadata<Target>
    {
        public override string DeclaredType => "Target";
        public override IReadOnlyList<string> FieldNames => new[] { "value" };
        public override Type GetFieldType(string fieldName) => fieldName == "value"
            ? typeof(long) : throw new ArgumentException(nameof(fieldName));
        public override object GetField(object record, string fieldName) => fieldName == "value"
            ? ((Target)record).Value : throw new ArgumentException(nameof(fieldName));
        public override object Read(CfdRecordNode record, CfdLoadContext context) => new Target(
            record.Key, CfdValueReader.Int64(CfdValueReader.Field(record.Fields, "value")));
    }

    private sealed class StatsMetadata : Metadata<Stats>
    {
        private readonly bool _includeFunctionField;

        public StatsMetadata(bool includeFunctionField = false) =>
            _includeFunctionField = includeFunctionField;

        public override string DeclaredType => "Stats";
        public override bool IsRecord => false;
        public override IReadOnlyList<string> FieldNames => _includeFunctionField
            ? new[] { "hp", "attack", "callback" }
            : new[] { "hp", "attack" };
        public override Type GetFieldType(string fieldName) => fieldName switch
        {
            "hp" or "attack" => typeof(long),
            "callback" when _includeFunctionField => typeof(CoflowFunctionSlot),
            _ => throw new ArgumentException(nameof(fieldName)),
        };
        public override object GetField(object record, string fieldName) => fieldName switch
        {
            "hp" => ((Stats)record).Hp,
            "attack" => ((Stats)record).Attack,
            "callback" when _includeFunctionField => throw new InvalidOperationException(),
            _ => throw new ArgumentException(nameof(fieldName)),
        };
        public override bool HasFieldDefault(string fieldName) => fieldName == "attack";
        public override object CreateObject(
            CfdLoadContext context,
            IReadOnlyDictionary<string, object?> fields) => new Stats(
                (long)fields["hp"]!,
                fields.TryGetValue("attack", out var attack) ? (long)attack! : 10);
        public override object Read(CfdRecordNode record, CfdLoadContext context) =>
            throw new InvalidOperationException();
    }

    private sealed class RuleMetadata : Metadata<Rule>
    {
        private readonly string _declaredType;
        private readonly bool _requireEvaluate;

        public RuleMetadata(string declaredType = "Rule", bool requireEvaluate = false)
        {
            _declaredType = declaredType;
            _requireEvaluate = requireEvaluate;
        }

        public int GetFieldCalls { get; private set; }
        public Rule? LastRule { get; private set; }

        public override string DeclaredType => _declaredType;
        public override IReadOnlyList<string> FieldNames => new[]
            { "offset", "selected", "callbacks", "evaluate", "enabled", "choose", "validate", "describe", "handlers" };
        public override Type GetFieldType(string fieldName) => fieldName switch
        {
            "offset" => typeof(long),
            "selected" => typeof(Reward),
            "callbacks" => typeof(Stats),
            "handlers" => typeof(IReadOnlyList<Func<long, long>>),
            "evaluate" or "enabled" or "choose" or "validate" or "describe" => typeof(CoflowFunctionSlot),
            _ => throw new ArgumentException($"unknown field `{fieldName}`", nameof(fieldName)),
        };

        public override object GetField(object record, string fieldName)
        {
            GetFieldCalls++;
            var rule = (Rule)record;
            return fieldName switch
            {
                "offset" => rule.Offset,
                "selected" => rule.Selected,
                "callbacks" => rule.Callbacks,
                "evaluate" => rule._evaluate,
                "enabled" => rule._enabled,
                "choose" => rule._choose,
                "validate" => rule._validate,
                "describe" => rule._describe,
                "handlers" => rule.Handlers,
                _ => throw new ArgumentException($"unknown field `{fieldName}`", nameof(fieldName)),
            };
        }

        public override object Read(CfdRecordNode record, CfdLoadContext context)
        {
            using var scope = context.EnterRecord(record.DeclaredType, record.Key);
            CfdValueReader.ValidateFields(record.Fields, "offset", "evaluate", "enabled", "choose", "validate", "describe", "handlers");
            return LastRule = new Rule(
                record.Key,
                CfdValueReader.FindField(record.Fields, "offset") is { } offset
                    ? CfdValueReader.Int64(offset)
                    : 0,
                new CoinReward("coin", 4),
                _requireEvaluate
                    ? context.RequiredFunction(CfdValueReader.FindField(record.Fields, "evaluate"),
                        "evaluate", typeof(long), typeof(long))
                    : context.Function(CfdValueReader.FindField(record.Fields, "evaluate"),
                        "evaluate", typeof(long), typeof(long)),
                context.Function(CfdValueReader.FindField(record.Fields, "enabled"),
                    "enabled", typeof(bool), typeof(long)),
                context.Function(CfdValueReader.FindField(record.Fields, "choose"),
                    "choose", typeof(Option<long>), typeof(long)),
                context.Function(CfdValueReader.FindField(record.Fields, "validate"),
                    "validate", typeof(Result<long, string>), typeof(long)),
                context.Function(CfdValueReader.FindField(record.Fields, "describe"),
                    "describe", typeof(string), typeof(long)),
                CfdValueReader.FindField(record.Fields, "handlers") is { } handlers
                    ? CfdValueReader.Array(handlers, context,
                        static (item, context) => context.FunctionValue<Func<long, long>>(item))
                    : Array.Empty<Func<long, long>>());
        }
    }


    private sealed class RewardMetadata : Metadata<Reward>
    {
        public override string DeclaredType => "Reward";
        public override bool IsAbstract => true;
        public override bool IsSealed => false;
        public override object Read(CfdRecordNode record, CfdLoadContext context) =>
            throw new InvalidOperationException();
    }

    private sealed class RewardHolderMetadata : Metadata<RewardHolder>
    {
        public override string DeclaredType => "RewardHolder";
        public override IReadOnlyList<string> FieldNames => new[] { "selected" };
        public override Type GetFieldType(string fieldName) => fieldName == "selected"
            ? typeof(Reward) : throw new ArgumentException(nameof(fieldName));
        public override string? ReferenceFieldType(string fieldName) => fieldName == "selected"
            ? "Reward" : null;
        public override object GetField(object record, string fieldName) => fieldName == "selected"
            ? ((RewardHolder)record).Selected : throw new ArgumentException(nameof(fieldName));
        public override object Read(CfdRecordNode record, CfdLoadContext context) => new RewardHolder(
            record.Key,
            CfdValueReader.Reference<Reward>(
                CfdValueReader.Field(record.Fields, "selected"), context, "Reward"));
    }

    private sealed class OptionalRewardHolderMetadata : Metadata<OptionalRewardHolder>
    {
        public override string DeclaredType => "OptionalRewardHolder";
        public override IReadOnlyList<string> FieldNames => new[] { "selected" };
        public override Type GetFieldType(string fieldName) => fieldName == "selected"
            ? typeof(Option<Reward>) : throw new ArgumentException(nameof(fieldName));
        public override object GetField(object record, string fieldName) => fieldName == "selected"
            ? ((OptionalRewardHolder)record).Selected : throw new ArgumentException(nameof(fieldName));
        public override object Read(CfdRecordNode record, CfdLoadContext context) => new OptionalRewardHolder(
            record.Key,
            CfdValueReader.Option(
                CfdValueReader.Field(record.Fields, "selected"), context,
                (node, loadContext) => CfdValueReader.Reference<Reward>(node, loadContext, "Reward")));
    }

    private sealed class CoinRewardMetadata : Metadata<CoinReward>
    {
        public override string DeclaredType => "CoinReward";
        public override IReadOnlyList<string> AssignableTypes => new[] { "CoinReward", "Reward" };
        public override IReadOnlyList<string> FieldNames => new[] { "amount" };
        public override Type GetFieldType(string fieldName) => fieldName == "amount"
            ? typeof(long) : throw new ArgumentException(nameof(fieldName));
        public override object GetField(object record, string fieldName) => fieldName == "amount"
            ? ((CoinReward)record).Amount : throw new ArgumentException(nameof(fieldName));
        public override object Read(CfdRecordNode record, CfdLoadContext context)
        {
            CfdValueReader.ValidateFields(record.Fields, "amount");
            return new CoinReward(record.Key,
                CfdValueReader.Int64(CfdValueReader.Field(record.Fields, "amount")));
        }
    }

    private sealed class HostServicesMetadata : Metadata<HostServices>
    {
        private readonly Action? _beforeTransfer;

        internal HostServicesMetadata(Action? beforeTransfer = null) =>
            _beforeTransfer = beforeTransfer;

        public override string DeclaredType => "HostServices";
        public override bool IsSingleton => true;
        public override bool IsHost => true;
        public override IReadOnlyList<string> FieldNames => new[] { "environment", "log", "adjust" };
        public override Type GetFieldType(string fieldName) => fieldName switch
        {
            "environment" => typeof(string),
            "log" => typeof(CoflowFunctionSlot),
            "adjust" => typeof(CoflowFunctionSlot),
            _ => throw new ArgumentException(nameof(fieldName)),
        };
        public override object GetField(object record, string fieldName) => fieldName switch
        {
            "environment" => ((HostServices)record).Environment,
            "log" => ((HostServices)record)._log,
            "adjust" => ((HostServices)record)._adjust,
            _ => throw new ArgumentException(nameof(fieldName)),
        };

        public override object CreateHost(CfdLoadContext context)
        {
            using var scope = context.EnterRecord(DeclaredType, string.Empty);
            return new HostServices(
                context.Host(),
                context.Function(null, "log", typeof(Unit), typeof(string)),
                context.Function(null, "adjust", typeof(long), typeof(long), typeof(Func<long, long>)));
        }

        public override void TransferHostState(object source, object target)
        {
            _beforeTransfer?.Invoke();
            var previous = (HostServices)source;
            var candidate = (HostServices)target;
            previous._host.TransferStateTo(candidate._host,
                () => candidate._environment = previous._environment,
                new CoflowHostFunctionTransfer(previous._log, candidate._log),
                new CoflowHostFunctionTransfer(previous._adjust, candidate._adjust));
        }

        public override object Read(CfdRecordNode record, CfdLoadContext context) =>
            throw new InvalidOperationException("Host records are not read from CFD.");
    }

    [System.Runtime.CompilerServices.MethodImpl(
        System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    private static (CoflowModule Data, IReadOnlyList<WeakReference> Records)
        ReloadManyAndCaptureOldRecords()
    {
        var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { current { offset: 0, evaluate: fn(value: int) -> int { value + offset } } }",
        }, new TestModule(new RuleMetadata()));
        var records = new List<WeakReference>();
        for (var generation = 1; generation <= 16; generation++)
        {
            records.Add(new WeakReference(data.Table<Rule>().Get("current").Value));
            Assert.True(data.Reload(
                $"Rule {{ current {{ offset: {generation}, evaluate: fn(value: int) -> int {{ value + offset }} }} }}").IsOk);
        }
        return (data, records);
    }

    [System.Runtime.CompilerServices.MethodImpl(
        System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    private static WeakReference DisposeAndCaptureRecord()
    {
        var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { disposed { evaluate: fn(value: int) -> int { value } } }",
        }, new TestModule(new RuleMetadata()));
        var record = data.Table<Rule>().Get("disposed").Value;
        data.Dispose();
        return new WeakReference(record);
    }

    [System.Runtime.CompilerServices.MethodImpl(
        System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    private static (CoflowModule Data, WeakReference Closure) ReloadAndCaptureOldClosure()
    {
        var data = Coflow.LoadAndCompile(new[]
        {
            "Rule { current { handlers: [fn(value: int) -> int { value + 1 }] } }",
        }, new TestModule(new RuleMetadata()));
        var closure = data.Table<Rule>().Get("current").Value.Handlers[0];
        Assert.True(data.Reload("Rule { current { handlers: [] } }").IsOk);
        return (data, new WeakReference(closure));
    }

    private static void CollectGarbage()
    {
        for (var attempt = 0; attempt < 3; attempt++)
        {
            GC.Collect();
            GC.WaitForPendingFinalizers();
            GC.Collect();
        }
    }

    private sealed class ValueRecordMetadata : Metadata<ValueRecord>
    {
        public override string DeclaredType => "ValueRecord";
        public override IReadOnlyList<string> FieldNames => new[] { "optional", "outcome" };
        public override Type GetFieldType(string fieldName) => fieldName switch
        {
            "optional" => typeof(Option<long>),
            "outcome" => typeof(Result<string, long>),
            _ => throw new ArgumentException(nameof(fieldName)),
        };
        public override object GetField(object record, string fieldName) => fieldName switch
        {
            "optional" => ((ValueRecord)record).Optional,
            "outcome" => ((ValueRecord)record).Outcome,
            _ => throw new ArgumentException(nameof(fieldName)),
        };

        public override object Read(CfdRecordNode record, CfdLoadContext context) => new ValueRecord(
            record.Key,
            CfdValueReader.Option(
                CfdValueReader.Field(record.Fields, "optional"),
                context,
                static (node, _) => CfdValueReader.Int64(node)),
            CfdValueReader.Result(
                CfdValueReader.Field(record.Fields, "outcome"),
                context,
                static (node, context) => CfdValueReader.String(node, context),
                static (node, _) => CfdValueReader.Int64(node)));
    }
}
