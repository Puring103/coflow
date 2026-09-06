using Coflow.Runtime;
using Coflow.Runtime.CompilerServices;
using System;
using System.Collections.Generic;
using System.Linq;
using Xunit;

namespace Coflow.Runtime.Tests;

public sealed class CoflowModuleTests
{
    private static readonly CoflowStringTableToken<Node> Nodes = new();
    private static readonly CoflowStringTableToken<Rule> Rules = new();

    static CoflowModuleTests()
    {
        CoflowTypeCodec.Register<Node>(new(1), 4, 0, 0,
            static value => value.CoflowId,
            static _ => true,
            static (value, id) => { value.CoflowId = id; return value; },
            static (context, value) => new Node
            {
                Id = value.Id,
                Value = value.Value,
                Next = context.Import(value.Next),
            },
            static (ref CoflowValueWriter writer, Node value) =>
            {
                writer.Write(value.Value);
                writer.Write(value.Next);
                writer.WriteValueId(value.CoflowId);
            });
        CoflowTypeCodec.Register<Settings>(new(2), 2, 0, 0,
            static value => value.CoflowId,
            static _ => true,
            static (value, id) => { value.CoflowId = id; return value; },
            static (_, value) => new Settings { Value = value.Value },
            static (ref CoflowValueWriter writer, Settings value) =>
            {
                writer.Write(value.Value);
                writer.WriteValueId(value.CoflowId);
            });
        CoflowTypeCodec.Register<Rule>(new(3), 1, 0, 0,
            static value => value.CoflowId,
            static _ => true,
            static (value, id) => { value.CoflowId = id; return value; },
            static (_, value) => new Rule { Id = value.Id },
            static (ref CoflowValueWriter writer, Rule value) => writer.WriteValueId(value.CoflowId));
        CoflowValueLayout.RegisterEnum<TestMode>();
        CoflowValueLayout.RegisterOption<Node>();
        CoflowValueLayout.RegisterArray<long>();
        CoflowValueLayout.RegisterArray<string>();
        CoflowValueLayout.RegisterDictionary<string, long>();
        CoflowValueLayout.RegisterFunction<CoflowFunction<long, long>>();
        CoflowValueLayout.RegisterFunction<CoflowFunction<long, CoflowFunction<long, long>>>();
        CoflowValueLayout.RegisterFunction<CoflowFunction<long>>();
        CoflowValueLayout.RegisterFunction<CoflowFunction<Node, CoflowFunction<long>>>();
        CoflowValueLayout.RegisterFunction<CoflowFunction<Node, Node, Node>>();
    }

    [Fact]
    public void LoadConnectsForwardAndCircularReferences()
    {
        var module = Compile("Node { first { value: 1, next: Some(&second) } second { value: 2, next: Some(&first) } }", Contract(new NodeMetadata()));
        var first = module.Table(Nodes).Get("first").Value;
        var second = module.Table(Nodes).Get("second").Value;
        Assert.Same(second, first.Next.Value);
        Assert.Same(first, second.Next.Value);
    }

    [Fact]
    public void MissingReferencesFailAndCrossModuleReferencesResolve()
    {
        var contract = Contract(new NodeMetadata());
        var missing = Coflow.Create(contract);
        missing.LoadModule(new CoflowSource("missing.cfd",
            "Node { first { value: 1, next: Some(&missing) } }"));
        var failed = missing.Compile();
        Assert.False(failed.Success);
        Assert.Contains(failed.Diagnostics, item => item.Code.StartsWith("CFD-REF", StringComparison.Ordinal));

        var cross = Coflow.Create(contract);
        cross.LoadModule(new CoflowSource("target.cfd", "Node { target { value: 2, next: None } }"));
        cross.LoadModule(new CoflowSource("source.cfd", "Node { source { value: 1, next: Some(&target) } }"));
        Assert.True(cross.Compile().Success);
        Assert.Same(cross.Table(Nodes).Get("target").Value,
            cross.Table(Nodes).Get("source").Value.Next.Value);
    }

    [Fact]
    public void MissingTableAndSingletonAreSharedEmptyAndNone()
    {
        var module = Compile(string.Empty, Contract(new NodeMetadata(), new SettingsMetadata()));
        Assert.Same(Nodes.Empty, module.Table(Nodes));
        Assert.Same(module.Table(Nodes), module.Table(Nodes));
        Assert.False(module.Singleton<Settings>().HasValue);
    }

    [Fact]
    public void SingletonIsOptionalButCannotBeDuplicated()
    {
        var contract = Contract(new SettingsMetadata());
        var module = Compile("settings: Settings { value: 42 }", contract);
        Assert.Equal(42, module.Singleton<Settings>().Value.Value);
        var error = Assert.Throws<CoflowLoadException>(() => Compile(
            "one: Settings { value: 1 } two: Settings { value: 2 }", contract));
        Assert.Contains(error.Diagnostics, item => item.Code == "CFD-SINGLETON-COUNT");
    }

    [Fact]
    public void ReplacementPublishesANewSnapshotWithoutMutatingOldValues()
    {
        var contract = Contract(new NodeMetadata());
        var coflow = Coflow.Create(contract);
        var module = coflow.LoadModule(new CoflowSource("nodes.cfd",
            "Node { item { value: 1, next: None } }"));
        Assert.True(coflow.Compile().Success);
        Assert.Equal(1, module.ParseCount);
        Assert.True(coflow.Compile().Success);
        Assert.Equal(1, module.ParseCount);
        var oldValue = coflow.Table(Nodes).Get("item").Value;
        coflow.ReplaceModule(module, new CoflowSource("nodes.cfd",
            "Node { item { value: 2, next: None } }"));
        Assert.True(coflow.Compile().Success);
        Assert.Equal(2, module.ParseCount);
        Assert.Equal(1, oldValue.Value);
        Assert.Equal(2, coflow.Table(Nodes).Get("item").Value.Value);
        coflow.RemoveModule(module);
        Assert.True(coflow.Compile().Success);
        Assert.Same(Nodes.Empty, coflow.Table(Nodes));
    }

    [Fact]
    public void VmExecutesCallsTailRecursionAndClosures()
    {
        var source = "Rule { main { " +
            "calculate: fn(value: int) -> int { if value > 0 { helper(value) } else { 0 } }, " +
            "helper: fn(value: int) -> int { value * 2 + 1 }, " +
            "tail: fn(value: int) -> int { if value <= 0 { 0 } else { tail(value - 1) } }, " +
            "make: fn(offset: int) -> fn(int) -> int { fn(value: int) -> int { value + offset } } } }";
        var module = Compile(source, Contract(new RuleMetadata()));
        var rule = module.Table(Rules).Get("main").Value;
        Assert.Equal(11, rule.Calculate(module, 5));
        Assert.Equal(0, rule.Tail(module, 1_000));
        Assert.Equal(9, rule.Make(module, 4).Invoke(module, 5));
    }

    [Fact]
    public void VmReusesTheRegisterWindowAcrossMutualTailCalls()
    {
        var source = "Rule { main { " +
            "calculate: fn(value: int) -> int { " +
            "if value <= 0 { 1 } else { helper(value - 1) } }, " +
            "helper: fn(value: int) -> int { " +
            "if value <= 0 { 0 } else { calculate(value - 1) } } } }";
        var coflow = Compile(source, Contract(new RuleMetadata()));
        var rule = coflow.Table(Rules).Get("main").Value;

        Assert.Equal(1, rule.Calculate(coflow, 1_000));
        Assert.Equal(0, rule.Calculate(coflow, 1_001));
    }

    [Fact]
    public void OverflowProducesSourceMappedFault()
    {
        var module = Compile(
            "Rule { main { calculate: fn(value: int) -> int { value + 1 } } }", Contract(new RuleMetadata()));
        var fault = Assert.Throws<CoflowFaultException>(() =>
            module.Table(Rules).Get("main").Value.Calculate(module, long.MaxValue));
        Assert.Equal("Rule", fault.Function.DeclaredType);
        Assert.NotNull(fault.SourceSpan);
    }

    [Fact]
    public void VmFaultContainsCrossFunctionCallStack()
    {
        var source = "Rule { main { " +
            "calculate: fn(value: int) -> int { helper(value) + 1 }, " +
            "helper: fn(value: int) -> int { value / 0 } } }";
        var coflow = Compile(source, Contract(new RuleMetadata()));
        var rule = coflow.Table(Rules).Get("main").Value;

        var fault = Assert.Throws<CoflowFaultException>(() => rule.Calculate(coflow, 3));
        Assert.Contains(fault.CallStack, item => item.FieldName == "calculate");
        Assert.Contains(fault.CallStack, item => item.FieldName == "helper");
        Assert.NotNull(fault.SourceSpan);
    }

    [Fact]
    public void CompilerFoldsConstantsAndEliminatesDeadCode()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { return if true { 1 + 2 } else { 99 }; 100 } } }";
        var coflow = Compile(source, Contract(new RuleMetadata()));
        var rule = coflow.Table(Rules).Get("main").Value;

        Assert.Equal(3, rule.Calculate(coflow, 0));
        var program = rule.CalculateEntry.CompiledProgram!;
        Assert.Equal(new[] { CoflowRegisterOpCode.ConstantInteger, CoflowRegisterOpCode.Return },
            program.RegisterProgram.Instructions.Select(instruction => instruction.Code));
    }

    [Fact]
    public void GeneratedFieldBindingUsesTypedReadersWithoutExpressionCompilation()
    {
        var before = CoflowExpressionCompiler.InterpretedCompilationCount;
        using (CoflowExpressionCompiler.OverrideDynamicCodeSupportForCurrentThread(false))
        {
            var binding = CoflowFieldBinding.Create<Settings, long>(
                "value", static settings => settings.Value);
            Assert.Equal(42, binding.ReadInteger!(new Settings { Value = 42 }));
            Assert.Equal(typeof(long), binding.RuntimeType);
        }
        Assert.Equal(before, CoflowExpressionCompiler.InterpretedCompilationCount);
    }

    [Fact]
    public void InstructionBudgetProducesSourceMappedFaultAndResetsPerTopLevelCall()
    {
        const string source = "Rule { main { calculate: fn(value: int) -> int { value + 1 } } }";
        var coflow = Coflow.Create(Contract(new RuleMetadata()), new CoflowOptions(maxInstructions: 1));
        coflow.LoadModule(new CoflowSource("budget.cfd", source));
        Assert.True(coflow.Compile().Success);
        var rule = coflow.Table(Rules).Get("main").Value;

        var first = Assert.Throws<CoflowFaultException>(() => rule.Calculate(coflow, 1));
        var limit = Assert.IsType<CoflowExecutionLimitException>(first.InnerException);
        Assert.Equal(nameof(CoflowOptions.MaxInstructions), limit.Limit);
        Assert.NotNull(first.SourceSpan);

        var second = Assert.Throws<CoflowFaultException>(() => rule.Calculate(coflow, 1));
        Assert.IsType<CoflowExecutionLimitException>(second.InnerException);
    }

    [Fact]
    public void ReplacingACalleeReusesCallerTemplateAndRelinksTheNewSnapshot()
    {
        var coflow = Coflow.Create(Contract(new RuleMetadata()));
        var target = coflow.LoadModule(new CoflowSource("target.cfd",
            "Rule { target { calculate: fn(value: int) -> int { value + 1 } } }"));
        var caller = coflow.LoadModule(new CoflowSource("caller.cfd",
            "Rule { caller { calculate: fn(value: int) -> int { &target.calculate(value) } } }"));

        Assert.True(coflow.Compile().Success);
        Assert.Equal(1, target.TemplateCompileCount);
        Assert.Equal(1, caller.TemplateCompileCount);
        var oldSnapshot = coflow.Snapshot;
        var oldCaller = coflow.Table(Rules).Get("caller").Value;
        Assert.Equal(2, oldCaller.Calculate(coflow, 1));

        coflow.ReplaceModule(target, new CoflowSource("target.cfd",
            "Rule { target { calculate: fn(value: int) -> int { value + 10 } } }"));
        Assert.True(coflow.Compile().Success);

        Assert.Equal(2, target.TemplateCompileCount);
        Assert.Equal(1, caller.TemplateCompileCount);
        Assert.Equal(11, coflow.Table(Rules).Get("caller").Value.Calculate(coflow, 1));
        using (CoflowInvocationContext.Enter(coflow, oldSnapshot))
            Assert.Equal(2, oldCaller.CalculateEntry.Invoke<long, long>(1));

        var publishedBeforeFailure = coflow.Snapshot;
        coflow.RemoveModule(target);
        var failed = coflow.Compile();
        Assert.False(failed.Success);
        Assert.Same(publishedBeforeFailure, coflow.Snapshot);
        Assert.Equal(11, coflow.Table(Rules).Get("caller").Value.Calculate(coflow, 1));
        Assert.NotSame(oldSnapshot, coflow.Snapshot);
    }

    [Fact]
    public void CachedUnqualifiedReferenceIsRecheckedWhenTheGlobalCatalogChanges()
    {
        var coflow = Coflow.Create(Contract(new RuleMetadata(), new RivalMetadata()));
        coflow.LoadModule(new CoflowSource("target.cfd",
            "Rule { target { calculate: fn(value: int) -> int { value + 1 } } }"));
        var caller = coflow.LoadModule(new CoflowSource("caller.cfd",
            "Rule { caller { calculate: fn(value: int) -> int { &target.calculate(value) } } }"));
        Assert.True(coflow.Compile().Success);
        var published = coflow.Snapshot;
        Assert.Equal(1, caller.TemplateCompileCount);

        coflow.LoadModule(new CoflowSource("rival.cfd",
            "Rival { target { calculate: fn(value: int) -> int { value + 100 } } }"));
        var failed = coflow.Compile();

        Assert.False(failed.Success);
        Assert.Contains(failed.Diagnostics, diagnostic =>
            diagnostic.Code == "COFLOW-FUNCTION-REFERENCE" &&
            diagnostic.Message.Contains("ambiguous", StringComparison.Ordinal));
        Assert.Same(published, coflow.Snapshot);
        Assert.Equal(1, caller.TemplateCompileCount);
    }

    [Fact]
    public void NonHostVmFieldAccessCannotRetainClrReaders()
    {
        var binding = CoflowFieldBinding.Create<Settings, long>(
            "value", static settings => settings.Value);
        var access = CoflowFieldAccess.Bind(new SettingsMetadata(), binding);

        Assert.False(access.IsHost);
        Assert.Null(access.ReadInteger);
        Assert.Null(access.ReadFloat);
        Assert.Null(access.ReadReference);
        Assert.Null(access.ReadValue);
    }

    [Fact]
    public void GeneratedEnumFieldBindingDoesNotBoxOnRead()
    {
        var binding = CoflowFieldBinding.CreateEnum<EnumSettings, TestMode>(
            "mode", static settings => settings.Mode, static value => (long)value);
        var settings = new EnumSettings { Mode = TestMode.Secondary };
        for (var index = 0; index < 1_000; index++) _ = binding.ReadInteger!(settings);

        var before = GC.GetAllocatedBytesForCurrentThread();
        long sum = 0;
        for (var index = 0; index < 10_000; index++) sum += binding.ReadInteger!(settings);
        var allocated = GC.GetAllocatedBytesForCurrentThread() - before;

        Assert.Equal(20_000, sum);
        Assert.Equal(0, allocated);
    }

    [Fact]
    public void SameTypeNumericConversionDoesNotEmitReinterpret()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { int(value) } } }";
        var coflow = Compile(source, Contract(new RuleMetadata()));
        var rule = coflow.Table(Rules).Get("main").Value;

        Assert.Equal(7, rule.Calculate(coflow, 7));
        Assert.DoesNotContain(
            rule.CalculateEntry.CompiledProgram!.RegisterProgram.Instructions,
            instruction => instruction.Code == CoflowRegisterOpCode.MoveValue);
    }

    [Fact]
    public void CompilerEmitsExplicitRegisterOperands()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { value + 1 } } }";
        var coflow = Compile(source, Contract(new RuleMetadata()));
        var rule = coflow.Table(Rules).Get("main").Value;

        Assert.Equal(8, rule.Calculate(coflow, 7));
        var instructions = rule.CalculateEntry.CompiledProgram!
            .RegisterProgram.Instructions;
        var add = Assert.Single(instructions, instruction =>
            instruction.Code == CoflowRegisterOpCode.AddInt);
        Assert.DoesNotContain(instructions, instruction =>
            instruction.Code == CoflowRegisterOpCode.MoveInteger);
        Assert.Equal(16, System.Runtime.InteropServices.Marshal.SizeOf<CoflowRegisterInstruction>());
        Assert.NotEqual(add.A, add.B);
        Assert.NotEqual(add.B, add.C);
    }

    [Fact]
    public void VmAndCollectionOperationsRunWithoutDynamicCodeSupport()
    {
        using (CoflowExpressionCompiler.OverrideDynamicCodeSupportForCurrentThread(false))
        {
            var source = "Rule { main { " +
                "calculate: fn(value: int) -> int { " +
                "[value, value + 1].map(fn(item: int) -> int { item * 2 })" +
                ".fold(0, fn(total: int, item: int) -> int { total + item }) }, " +
                "make: fn(offset: int) -> fn(int) -> int { " +
                "fn(value: int) -> int { value + offset } } } }";
            var coflow = Compile(source, Contract(new RuleMetadata()));
            var rule = coflow.Table(Rules).Get("main").Value;

            Assert.Equal(14, rule.Calculate(coflow, 3));
            Assert.Equal(9, rule.Make(coflow, 4).Invoke(coflow, 5));
            var calculate = coflow.Snapshot.Functions.Values.Single(function =>
                function.Identity.FieldName == "calculate").CompiledProgram!;
            Assert.Contains(calculate.RegisterProgram.Instructions,
                instruction => instruction.Code == CoflowRegisterOpCode.BeginArrayBuilder);
            Assert.Contains(calculate.RegisterProgram.Instructions,
                instruction => instruction.Code == CoflowRegisterOpCode.AppendArrayBuilder);
            Assert.DoesNotContain(calculate.RegisterProgram.Instructions,
                instruction => instruction.Code == CoflowRegisterOpCode.Native);
        }
    }

    [Fact]
    public void CollectionCountAndDictionaryProjectionsStayInsideTheArena()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { " +
            "var dictionary = { \"a\": value, \"b\": value + 1 }; " +
            "dictionary.len() + dictionary.keys().len() + dictionary.values().len() } } }";
        var coflow = Compile(source, Contract(new RuleMetadata()));
        var rule = coflow.Table(Rules).Get("main").Value;

        Assert.Equal(6, rule.Calculate(coflow, 3));
        var instructions = rule.CalculateEntry.CompiledProgram!.RegisterProgram.Instructions;
        Assert.Contains(instructions, instruction => instruction.Code == CoflowRegisterOpCode.DictionaryKeys);
        Assert.Contains(instructions, instruction => instruction.Code == CoflowRegisterOpCode.DictionaryValues);
        Assert.Equal(3, instructions.Count(instruction =>
            instruction.Code == CoflowRegisterOpCode.CollectionCount));
        Assert.DoesNotContain(instructions, instruction => instruction.Code == CoflowRegisterOpCode.Native);
    }

    [Fact]
    public void CollectionBuiltinsOperateOnArenaLanesWithoutNativeCalls()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { " +
            "var numbers = [1, 2, 3]; var dictionary = { \"a\": 1, \"b\": 2 }; " +
            "if numbers.contains(2) && numbers.isUnique() && numbers.isSorted() && " +
            "numbers.isStrictlySorted() && numbers.intersects([3, 4]) && " +
            "numbers.isDisjoint([4, 5]) && numbers.isSubsetOf([0, 1, 2, 3]) && " +
            "numbers.isSupersetOf([1, 2]) && dictionary.containsKey(\"a\") && " +
            "dictionary.contains(\"b\") && dictionary.containsValue(2) { " +
            "numbers.min() + numbers.max() + numbers.sum() + dictionary.values().sum() " +
            "} else { value } } } }";
        var coflow = Compile(source, Contract(new RuleMetadata()));
        var rule = coflow.Table(Rules).Get("main").Value;

        Assert.Equal(13, rule.Calculate(coflow, -1));
        var instructions = rule.CalculateEntry.CompiledProgram!.RegisterProgram.Instructions;
        Assert.Contains(instructions, instruction =>
            instruction.Code == CoflowRegisterOpCode.CollectionBuiltin);
        Assert.DoesNotContain(instructions, instruction =>
            instruction.Code == CoflowRegisterOpCode.Native);
    }

    [Fact]
    public void ReturnedVmClosuresUseTwoIntegerIds()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { value }, " +
            "make: fn(offset: int) -> fn(int) -> int { " +
            "fn(value: int) -> int { value + offset } } } }";
        var coflow = Compile(source, Contract(new RuleMetadata()));
        var rule = coflow.Table(Rules).Get("main").Value;

        var closure = rule.Make(coflow, 4);
        Assert.Equal(2, CoflowValueShape.Of(typeof(CoflowFunction<long, long>)).IntegerCount);
        Assert.Equal(0, CoflowValueShape.Of(typeof(CoflowFunction<long, long>)).ReferenceCount);
        Assert.Equal(9, closure.Invoke(coflow, 5));
        Assert.Throws<CoflowFunctionNotBoundException>(() =>
            default(CoflowFunction<long, long>).Invoke(coflow, 1));

        var other = Compile(source, Contract(new RuleMetadata()));
        Assert.Throws<CoflowStaleValueException>(() => closure.Invoke(other, 5));
        Assert.True(coflow.Compile().Success);
        Assert.Throws<CoflowStaleValueException>(() => closure.Invoke(coflow, 5));
    }

    [Fact]
    public void ReturnedClosureKeepsCapturedInvocationCollectionsAlive()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { value }, " +
            "make: fn(offset: int) -> fn(int) -> int { " +
            "var captured = [offset, offset + 1]; " +
            "fn(value: int) -> int { captured.sum() + value } } } }";
        var coflow = Compile(source, Contract(new RuleMetadata()));
        var closure = coflow.Table(Rules).Get("main").Value.Make(coflow, 4);

        var vmClosure = coflow.Snapshot.Function(closure.FunctionId, closure.EnvironmentId).Closure!;
        Assert.Single(vmClosure.Collections);
        Assert.Equal(14, closure.Invoke(coflow, 5));
        Assert.Equal(15, closure.Invoke(coflow, 6));
    }

    [Fact]
    public void ReturnedClosureDoesNotRetainUnreachableInvocationCollections()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { value }, " +
            "make: fn(offset: int) -> fn(int) -> int { " +
            "var unused = [offset, offset + 1]; " +
            "fn(value: int) -> int { offset + value } } } }";
        var coflow = Compile(source, Contract(new RuleMetadata()));
        var closure = coflow.Table(Rules).Get("main").Value.Make(coflow, 4);

        var vmClosure = coflow.Snapshot.Function(closure.FunctionId, closure.EnvironmentId).Closure!;
        Assert.Empty(vmClosure.Collections);
        Assert.Equal(9, closure.Invoke(coflow, 5));
    }

    [Fact]
    public void EscapeCollectorOnlyIncludesReachableGeneratedValues()
    {
        var child = new Node { CoflowId = new CoflowValueId(7, 11), Value = 2 };
        var root = new Node
        {
            CoflowId = new CoflowValueId(7, 10),
            Value = 1,
            Next = Option<Node>.Some(child),
        };
        var unrelated = new Node { CoflowId = new CoflowValueId(7, 12), Value = 3 };
        var collector = new CoflowValueIdCollector();

        CoflowEscapeValue<Node>.Collect(root, collector);

        Assert.Contains(root.CoflowId, collector.Ids);
        Assert.Contains(child.CoflowId, collector.Ids);
        Assert.DoesNotContain(unrelated.CoflowId, collector.Ids);
    }

    [Fact]
    public void ReturnedClosurePromotesCapturedExternalGeneratedValue()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { value }, " +
            "makeNode: fn(node: Node) -> fn() -> int { fn() -> int { node.value } } } }";
        var coflow = Compile(source, Contract(new RuleMetadata(), new NodeMetadata()));
        var external = new Node { Value = 37, Next = Option<Node>.None };

        var closure = coflow.Table(Rules).Get("main").Value.MakeNode(coflow, external);

        Assert.Equal(37, closure.Invoke(coflow));
    }

    [Fact]
    public void ReturningOneExternalValueDoesNotPromoteOtherArguments()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { value }, " +
            "select: fn(keep: Node, discard: Node) -> Node { keep } } }";
        var coflow = Compile(source, Contract(new RuleMetadata(), new NodeMetadata()));
        var keep = new Node { Value = 1, Next = Option<Node>.None };
        var discard = new Node { Value = 2, Next = Option<Node>.None };

        var returned = coflow.Table(Rules).Get("main").Value.Select(coflow, keep, discard);

        Assert.Equal(1, returned.Value);
        Assert.Equal(1, coflow.Snapshot.EscapedValueCount);
    }

    [Fact]
    public void EscapedValueLimitIsAppliedAcrossTopLevelCalls()
    {
        var coflow = Coflow.Create(Contract(new RuleMetadata(), new NodeMetadata()),
            new CoflowOptions(maxEscapedValues: 1));
        coflow.LoadModule(new CoflowSource("escape.cfd",
            "Rule { main { calculate: fn(value: int) -> int { value }, " +
            "select: fn(keep: Node, discard: Node) -> Node { keep } } }"));
        Assert.True(coflow.Compile().Success);
        var rule = coflow.Table(Rules).Get("main").Value;

        _ = rule.Select(coflow,
            new Node { Value = 1, Next = Option<Node>.None },
            new Node { Value = 0, Next = Option<Node>.None });
        var fault = Assert.Throws<CoflowFaultException>(() => rule.Select(coflow,
            new Node { Value = 2, Next = Option<Node>.None },
            new Node { Value = 0, Next = Option<Node>.None }));
        var error = Assert.IsType<CoflowExecutionLimitException>(fault.InnerException);
        Assert.Equal(nameof(CoflowOptions.MaxEscapedValues), error.Limit);
    }

    [Fact]
    public void EscapedLaneLimitChecksTheWholeAdditionBeforePublishing()
    {
        var coflow = Coflow.Create(Contract(new RuleMetadata(), new NodeMetadata()),
            new CoflowOptions(maxEscapedLanes: 3));
        coflow.LoadModule(new CoflowSource("escape-lanes.cfd",
            "Rule { main { calculate: fn(value: int) -> int { value }, " +
            "select: fn(keep: Node, discard: Node) -> Node { keep } } }"));
        Assert.True(coflow.Compile().Success);

        var fault = Assert.Throws<CoflowFaultException>(() =>
            coflow.Table(Rules).Get("main").Value.Select(coflow,
                new Node { Value = 1, Next = Option<Node>.None },
                new Node { Value = 0, Next = Option<Node>.None }));
        var error = Assert.IsType<CoflowExecutionLimitException>(fault.InnerException);
        Assert.Equal(nameof(CoflowOptions.MaxEscapedLanes), error.Limit);
        Assert.Equal(0, coflow.Snapshot.EscapedValueCount);
    }

    [Fact]
    public void CompilerHandlesDeepExpressionsClosuresAndNonTailRecursion()
    {
        const int depth = 128;
        var expression = "value";
        for (var index = 0; index < depth; index++) expression = $"({expression} + 1)";
        var captures = string.Join(" ", Enumerable.Range(0, depth).Select(index =>
            $"var capture{index} = {(index == 0 ? "offset" : $"capture{index - 1}")} + 1;"));
        var source = $"Rule {{ main {{ " +
            $"calculate: fn(value: int) -> int {{ {expression} }}, " +
            $"deep: fn(value: int) -> int {{ if value <= 0 {{ 0 }} else {{ 1 + deep(value - 1) }} }}, " +
            $"make: fn(offset: int) -> fn(int) -> int {{ {captures} fn(value: int) -> int {{ value + capture{depth - 1} }} }} " +
            "} }";
        var coflow = Compile(source, Contract(new RuleMetadata()));
        var rule = coflow.Table(Rules).Get("main").Value;

        Assert.Equal(depth + 1, rule.Calculate(coflow, 1));
        Assert.Equal(depth, rule.Deep(coflow, depth));
        Assert.Equal(depth + 2, rule.Make(coflow, 1).Invoke(coflow, 1));
    }

    [Fact]
    public void VmExecutesLoopControlRangesIndexesAndCompoundAssignments()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { " +
            "var total = 0; for item, index in 0..value { " +
            "if item == 1 { continue; }; if item == 4 { break; }; " +
            "total += item + index; } " +
            "total -= 1; total *= 2; total /= 2; total } } }";
        var coflow = Compile(source, Contract(new RuleMetadata()));
        var rule = coflow.Table(Rules).Get("main").Value;

        Assert.Equal(9, rule.Calculate(coflow, 10));
    }

    [Fact]
    public void VmExecutesTerminatingLoopsAndInclusiveRangeAtIntegerLimit()
    {
        var source = "Rule { main { " +
            "calculate: fn(value: int) -> int { " +
            "for item in value..=9223372036854775807 { return item; } 0 }, " +
            "helper: fn(value: int) -> int { while value > 0 { return 7; } 3 } } }";
        var coflow = Compile(source, Contract(new RuleMetadata()));
        var rule = coflow.Table(Rules).Get("main").Value;

        Assert.Equal(long.MaxValue, rule.Calculate(coflow, long.MaxValue));
        Assert.Equal(7, rule.Helper(coflow, 1));
        Assert.Equal(3, rule.Helper(coflow, 0));
        Assert.DoesNotContain(rule.CalculateEntry.CompiledProgram!
            .RegisterProgram.Instructions,
            instruction => instruction.Code == CoflowRegisterOpCode.Native);
    }

    [Fact]
    public void VmExecutesPrecompiledRegexWithoutLeakingThePatternOntoTheStack()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { " +
            "if \"abc\".matches(\"^a.*c$\") { value } else { 0 } } } }";
        var coflow = Compile(source, Contract(new RuleMetadata()));
        var rule = coflow.Table(Rules).Get("main").Value;

        Assert.Equal(7, rule.Calculate(coflow, 7));
    }

    [Fact]
    public void CompilerAcceptsExhaustiveBooleanMatchPatterns()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { " +
            "match value > 0 { true => value, false => 0 } } } }";
        var coflow = Compile(source, Contract(new RuleMetadata()));
        var rule = coflow.Table(Rules).Get("main").Value;

        Assert.Equal(7, rule.Calculate(coflow, 7));
        Assert.Equal(0, rule.Calculate(coflow, -1));
    }

    [Fact]
    public void VmFormatsEscapedBracesArgumentsAndContextMetadata()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { " +
            "\"{{ok}} {$id} {value}\".len() } } }";
        var coflow = Compile(source, Contract(new RuleMetadata()));
        var rule = coflow.Table(Rules).Get("main").Value;

        Assert.Equal(11, rule.Calculate(coflow, 7));
    }

    [Fact]
    public void CompilerReportsIndependentFunctionErrorsWithSourceLocations()
    {
        var source = "Rule { main { " +
            "calculate: fn(value: int) -> int { \"wrong\" }, " +
            "helper: fn(value: int) -> int { unknown(value) } } }";

        var error = Assert.Throws<CoflowLoadException>(() =>
            Compile(source, Contract(new RuleMetadata())));

        Assert.Equal(2, error.Diagnostics.Count);
        Assert.Contains(error.Diagnostics, item => item.Code == "COFLOW-FUNCTION-TYPE");
        Assert.Contains(error.Diagnostics, item => item.Code == "COFLOW-FUNCTION-NAME");
        Assert.All(error.Diagnostics, item =>
        {
            Assert.False(string.IsNullOrWhiteSpace(item.Path));
            Assert.NotNull(item.Span);
        });
    }

    [Theory]
    [InlineData("break; 0", "COFLOW-FUNCTION-CONTROL")]
    [InlineData("continue; 0", "COFLOW-FUNCTION-CONTROL")]
    [InlineData("for item in value { } 0", "COFLOW-FUNCTION-TYPE")]
    [InlineData("value += 1; value", "COFLOW-FUNCTION-ASSIGN")]
    [InlineData("~\"text\"", "COFLOW-FUNCTION-TYPE")]
    [InlineData("value + 1.0", "COFLOW-FUNCTION-TYPE")]
    [InlineData("var range = 0..value; 0", "COFLOW-FUNCTION-TYPE")]
    [InlineData("if value is int { 1 } else { 0 }", "COFLOW-FUNCTION-TYPE")]
    [InlineData("$missing; 0", "COFLOW-FUNCTION-METADATA")]
    [InlineData("\"{}\"; 0", "COFLOW-FUNCTION-INTERPOLATION")]
    [InlineData("match value { 1 => 1 }", "COFLOW-FUNCTION-MATCH")]
    [InlineData("value.len()", "COFLOW-FUNCTION-BUILTIN")]
    [InlineData("Missing::Rule {}; 0", "COFLOW-FUNCTION-NAME")]
    [InlineData("Missing::Group::Value", "COFLOW-FUNCTION-NAME")]
    public void CompilerRejectsInvalidControlOperatorMatchMetadataAndBuiltinSyntax(
        string body,
        string code)
    {
        var source = $"Rule {{ main {{ calculate: fn(value: int) -> int {{ {body} }} }} }}";

        var error = Assert.Throws<CoflowLoadException>(() =>
            Compile(source, Contract(new RuleMetadata())));

        Assert.Contains(error.Diagnostics, item => item.Code == code);
        Assert.All(error.Diagnostics, item => Assert.NotNull(item.Span));
    }

    private static Coflow Compile(string source, ICoflowSchema schema)
    {
        var coflow = Coflow.Create(schema);
        if (source.Length != 0) coflow.LoadModule(new CoflowSource("test.cfd", source));
        var result = coflow.Compile();
        if (!result.Success) throw new CoflowLoadException(result.Diagnostics);
        return coflow;
    }

    private static ICoflowSchema Contract(params ICoflowTypeMetadata[] metadata) => new TestContract(metadata);

    private sealed class TestContract(params ICoflowTypeMetadata[] types) : ICoflowSchema
    {
        public IReadOnlyList<ICoflowTypeMetadata> Types { get; } = types;
        public IReadOnlyList<ICoflowEnumMetadata> Enums { get; } = Array.Empty<ICoflowEnumMetadata>();
        public IReadOnlyList<CoflowConstant> Constants { get; } = Array.Empty<CoflowConstant>();
    }

    private interface IIdentified
    {
        CoflowValueId CoflowId { get; set; }
    }

    private sealed class Node : IIdentified
    {
        public CoflowValueId CoflowId { get; set; }
        public string Id { get; internal set; } = string.Empty;
        public long Value { get; internal set; }
        public Option<Node> Next { get; internal set; }
    }

    private sealed class Settings : IIdentified
    {
        public CoflowValueId CoflowId { get; set; }
        public long Value { get; internal set; }
    }

    private enum TestMode { Primary = 1, Secondary = 2 }
    private sealed class EnumSettings { public TestMode Mode { get; internal set; } }

    private sealed class Rule : IIdentified
    {
        public CoflowValueId CoflowId { get; set; }
        public string Id { get; internal set; } = string.Empty;
        internal CoflowFunctionEntry CalculateEntry = default!;
        internal CoflowFunctionEntry HelperEntry = default!;
        internal CoflowFunctionEntry TailEntry = default!;
        internal CoflowFunctionEntry MakeEntry = default!;
        internal CoflowFunctionEntry DeepEntry = default!;
        public long Calculate(Coflow coflow, long value) =>
            CoflowInvoker.Invoke<Rule, long, long>(coflow, this, CoflowId, new(3), new(0), value);
        public long Helper(Coflow coflow, long value) =>
            CoflowInvoker.Invoke<Rule, long, long>(coflow, this, CoflowId, new(3), new(1), value);
        public long Tail(Coflow coflow, long value) =>
            CoflowInvoker.Invoke<Rule, long, long>(coflow, this, CoflowId, new(3), new(2), value);
        public CoflowFunction<long, long> Make(Coflow coflow, long value) =>
            CoflowInvoker.Invoke<Rule, long, CoflowFunction<long, long>>(coflow, this, CoflowId, new(3), new(3), value);
        public long Deep(Coflow coflow, long value) =>
            CoflowInvoker.Invoke<Rule, long, long>(coflow, this, CoflowId, new(3), new(4), value);
        public CoflowFunction<long> MakeNode(Coflow coflow, Node value) =>
            CoflowInvoker.Invoke<Rule, Node, CoflowFunction<long>>(coflow, this, CoflowId, new(3), new(5), value);
        public Node Select(Coflow coflow, Node keep, Node discard) =>
            CoflowInvoker.Invoke<Rule, Node, Node, Node>(
                coflow, this, CoflowId, new(3), new(6), keep, discard);
    }

    private sealed class Rival : IIdentified
    {
        public CoflowValueId CoflowId { get; set; }
        public string Id { get; internal set; } = string.Empty;
        internal CoflowFunctionEntry CalculateEntry = default!;
    }

    private abstract class Metadata<T> : ICoflowRecordMetadata where T : class, new()
    {
        public abstract CoflowTypeId TypeId { get; }
        public Type RuntimeType => typeof(T);
        public Type KeyType => typeof(string);
        public virtual bool IsSingleton => false;
        public bool IsAbstract => false;
        public bool IsSealed => true;
        public abstract string DeclaredType { get; }
        public IReadOnlyList<string> AssignableTypes => new[] { DeclaredType };
        public IReadOnlyList<CoflowTypeId> AssignableTypeIds => new[] { TypeId };
        public IReadOnlyList<CoflowAnnotation> Annotations => Array.Empty<CoflowAnnotation>();
        public abstract IReadOnlyList<string> FieldNames { get; }
        public IReadOnlyList<CoflowFieldMetadata> Fields => FieldNames.Select(name =>
            new CoflowFieldMetadata(GetFieldBinding(name), Array.Empty<CoflowAnnotation>(),
                HasFieldDefault(name), ObjectFieldType(name), ReferenceFieldType(name))).ToArray();
        public IReadOnlyList<CoflowAnnotation> FieldAnnotations(string fieldName) => Array.Empty<CoflowAnnotation>();
        public abstract CoflowFieldBinding GetFieldBinding(string fieldName);
        public bool HasFieldDefault(string fieldName) => false;
        public object CreateObject(CfdLoadContext context, IReadOnlyDictionary<string, object?> fields) => throw new InvalidOperationException();
        public Delegate CreateVmObjectFactory(CfdLoadContext context) => throw new InvalidOperationException();
        public Delegate CreateVmDefaultFactory(string fieldName, CfdLoadContext context) => throw new ArgumentException(nameof(fieldName));
        public string? ObjectFieldType(string fieldName) => null;
        public virtual string? ReferenceFieldType(string fieldName) => null;
        public object ParseKey(string key) => key;
        public abstract Delegate GetKeyReader();
        public object CreateRecord(string key, CfdLoadContext context) => new T();
        public CoflowValueId GetValueId(object value) => ((IIdentified)value).CoflowId;
        public object WithValueId(object value, CoflowValueId id)
        {
            ((IIdentified)value).CoflowId = id;
            return value;
        }
        public abstract void PopulateRecord(object target, CfdRecordNode record, CfdLoadContext context);
    }

    private sealed class NodeMetadata : Metadata<Node>
    {
        private static readonly CoflowFieldBinding Value =
            CoflowFieldBinding.Create<Node, long>("value", static node => node.Value);
        private static readonly CoflowFieldBinding Next =
            CoflowFieldBinding.Create<Node, Option<Node>>("next", static node => node.Next,
                integerOffset: 1);

        public override string DeclaredType => "Node";
        public override CoflowTypeId TypeId => new(1);
        public override IReadOnlyList<string> FieldNames => new[] { "value", "next" };
        public override CoflowFieldBinding GetFieldBinding(string name) => name switch
            { "value" => Value, "next" => Next, _ => throw new ArgumentException(nameof(name)) };
        public override string? ReferenceFieldType(string name) => name == "next" ? "Node" : null;
        public override Delegate GetKeyReader() => new Func<Node, string>(static value => value.Id);
        public override void PopulateRecord(object target, CfdRecordNode record, CfdLoadContext context)
        {
            using var scope = context.EnterRecord(record.DeclaredType, record.Key);
            var node = (Node)target;
            node.Id = record.Key;
            node.Value = CfdValueReader.Int64(CfdValueReader.Field(record.Fields, "value"));
            node.Next = CfdValueReader.Option(CfdValueReader.Field(record.Fields, "next"), context,
                static (item, load) => CfdValueReader.Reference<Node>(item, load, "Node"));
        }
    }

    private sealed class SettingsMetadata : Metadata<Settings>
    {
        private static readonly CoflowFieldBinding Value =
            CoflowFieldBinding.Create<Settings, long>("value", static settings => settings.Value);

        public override string DeclaredType => "Settings";
        public override CoflowTypeId TypeId => new(2);
        public override bool IsSingleton => true;
        public override IReadOnlyList<string> FieldNames => new[] { "value" };
        public override CoflowFieldBinding GetFieldBinding(string name) => name == "value"
            ? Value : throw new ArgumentException(nameof(name));
        public override Delegate GetKeyReader() => new Func<Settings, string>(static _ => string.Empty);
        public override void PopulateRecord(object target, CfdRecordNode record, CfdLoadContext context) =>
            ((Settings)target).Value = CfdValueReader.Int64(CfdValueReader.Field(record.Fields, "value"));
    }

    private sealed class RuleMetadata : Metadata<Rule>
    {
        private static readonly IReadOnlyDictionary<string, CoflowFieldBinding> Bindings =
            new Dictionary<string, CoflowFieldBinding>(StringComparer.Ordinal)
            {
                ["calculate"] = CoflowFieldBinding.Function<Rule, CoflowFunction<long, long>>(
                    "calculate", new(3), new(0), static rule => rule.CoflowId, false),
                ["helper"] = CoflowFieldBinding.Function<Rule, CoflowFunction<long, long>>(
                    "helper", new(3), new(1), static rule => rule.CoflowId, false),
                ["tail"] = CoflowFieldBinding.Function<Rule, CoflowFunction<long, long>>(
                    "tail", new(3), new(2), static rule => rule.CoflowId, false),
                ["make"] = CoflowFieldBinding.Function<Rule, CoflowFunction<long, CoflowFunction<long, long>>>(
                    "make", new(3), new(3), static rule => rule.CoflowId, false),
                ["deep"] = CoflowFieldBinding.Function<Rule, CoflowFunction<long, long>>(
                    "deep", new(3), new(4), static rule => rule.CoflowId, false),
                ["makeNode"] = CoflowFieldBinding.Function<Rule, CoflowFunction<Node, CoflowFunction<long>>>(
                    "makeNode", new(3), new(5), static rule => rule.CoflowId, false),
                ["select"] = CoflowFieldBinding.Function<Rule, CoflowFunction<Node, Node, Node>>(
                    "select", new(3), new(6), static rule => rule.CoflowId, false),
            };

        public override string DeclaredType => "Rule";
        public override CoflowTypeId TypeId => new(3);
        public override IReadOnlyList<string> FieldNames => Bindings.Keys.ToArray();
        public override CoflowFieldBinding GetFieldBinding(string name) => Bindings.TryGetValue(name, out var binding)
            ? binding : throw new ArgumentException(nameof(name));
        public override Delegate GetKeyReader() => new Func<Rule, string>(static value => value.Id);
        public override void PopulateRecord(object target, CfdRecordNode record, CfdLoadContext context)
        {
            using var scope = context.EnterRecord(record.DeclaredType, record.Key);
            var rule = (Rule)target;
            rule.Id = record.Key;
            rule.CalculateEntry = Entry(context, record, "calculate", required: true);
            rule.HelperEntry = Entry(context, record, "helper");
            rule.TailEntry = Entry(context, record, "tail");
            rule.MakeEntry = context.Function(CfdValueReader.FindField(record.Fields, "make"),
                "make", typeof(CoflowFunction<long, long>), typeof(long));
            rule.DeepEntry = Entry(context, record, "deep");
            _ = context.Function(CfdValueReader.FindField(record.Fields, "makeNode"),
                "makeNode", typeof(CoflowFunction<long>), typeof(Node));
            _ = context.Function(CfdValueReader.FindField(record.Fields, "select"),
                "select", typeof(Node), typeof(Node), typeof(Node));
        }
        private static CoflowFunctionEntry Entry(
            CfdLoadContext context, CfdRecordNode record, string name, bool required = false)
        {
            var node = CfdValueReader.FindField(record.Fields, name);
            var entry = required
                ? context.RequiredFunction(node, name, typeof(long), typeof(long))
                : context.Function(node, name, typeof(long), typeof(long));
            return entry;
        }
    }

    private sealed class RivalMetadata : Metadata<Rival>
    {
        private static readonly CoflowFieldBinding Calculate =
            CoflowFieldBinding.Function<Rival, CoflowFunction<long, long>>(
                "calculate", new(4), new(0), static rival => rival.CoflowId, false);

        public override string DeclaredType => "Rival";
        public override CoflowTypeId TypeId => new(4);
        public override IReadOnlyList<string> FieldNames => new[] { "calculate" };
        public override CoflowFieldBinding GetFieldBinding(string name) => name == "calculate"
            ? Calculate
            : throw new ArgumentException(nameof(name));
        public override Delegate GetKeyReader() => new Func<Rival, string>(static value => value.Id);
        public override void PopulateRecord(object target, CfdRecordNode record, CfdLoadContext context)
        {
            using var scope = context.EnterRecord(record.DeclaredType, record.Key);
            var rival = (Rival)target;
            rival.Id = record.Key;
            rival.CalculateEntry = context.RequiredFunction(
                CfdValueReader.FindField(record.Fields, "calculate"),
                "calculate", typeof(long), typeof(long));
        }
    }
}
