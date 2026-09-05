using CoflowRuntime;
using CoflowRuntime.Generated;
using System;
using System.Collections.Generic;
using System.Linq;
using Xunit;

namespace CoflowRuntime.Tests;

public sealed class CoflowModuleTests
{
    private static readonly CoflowStringTableToken<Node> Nodes = new();
    private static readonly CoflowStringTableToken<Rule> Rules = new();

    [Fact]
    public void LoadConnectsForwardAndCircularReferences()
    {
        var module = LoadData("Node { first { value: 1, next: Some(&second) } second { value: 2, next: Some(&first) } }");
        var first = module.Table(Nodes).Get("first").Value;
        var second = module.Table(Nodes).Get("second").Value;
        Assert.Same(second, first.Next.Value);
        Assert.Same(first, second.Next.Value);
    }

    [Fact]
    public void MissingAndCrossModuleReferencesFail()
    {
        var contract = Contract(new NodeMetadata());
        var missing = Assert.Throws<CfdLoadException>(() => Coflow.LoadData(
            new[] { "Node { first { value: 1, next: Some(&missing) } }" }, contract));
        Assert.Contains(missing.Diagnostics, item => item.Code.StartsWith("CFD-REF", StringComparison.Ordinal));

        _ = Coflow.LoadData(new[] { "Node { target { value: 2, next: None } }" }, contract);
        var cross = Assert.Throws<CfdLoadException>(() => Coflow.LoadData(
            new[] { "Node { source { value: 1, next: Some(&target) } }" }, contract));
        Assert.Contains(cross.Diagnostics, item => item.Code.StartsWith("CFD-REF", StringComparison.Ordinal));
    }

    [Fact]
    public void MissingTableAndSingletonAreSharedEmptyAndNone()
    {
        var module = Coflow.LoadData(Array.Empty<string>(), Contract(new NodeMetadata(), new SettingsMetadata()));
        Assert.Same(Nodes.Empty, module.Table(Nodes));
        Assert.Same(module.Table(Nodes), module.Table(Nodes));
        Assert.False(module.Singleton<Settings>().HasValue);
    }

    [Fact]
    public void SingletonIsOptionalButCannotBeDuplicated()
    {
        var contract = Contract(new SettingsMetadata());
        var module = Coflow.LoadData(new[] { "settings: Settings { value: 42 }" }, contract);
        Assert.Equal(42, module.Singleton<Settings>().Value.Value);
        var error = Assert.Throws<CoflowLoadException>(() => Coflow.LoadData(new[]
            { "one: Settings { value: 1 } two: Settings { value: 2 }" }, contract));
        Assert.Contains(error.Diagnostics, item => item.Code == "CFD-SINGLETON-COUNT");
    }

    [Fact]
    public void ModuleSetReplacementDoesNotMutateOldView()
    {
        var contract = Contract(new NodeMetadata());
        var first = Coflow.LoadData(new[] { "Node { item { value: 1, next: None } }" }, contract);
        var second = Coflow.LoadData(new[] { "Node { item { value: 2, next: None } }" }, contract);
        var oldView = new CoflowModuleSet(first);
        var newView = oldView.Replace(first, second);
        Assert.Equal(1, oldView.Table(Nodes).Get("item").Value.Value);
        Assert.Equal(2, newView.Table(Nodes).Get("item").Value.Value);
        Assert.Throws<CoflowLoadException>(() => oldView.Add(second));
        Assert.Single(oldView.Modules);
        var removed = oldView.Remove(first);
        Assert.Empty(removed.Modules);
        Assert.Same(Nodes.Empty, removed.Table(Nodes));
    }

    [Fact]
    public void VmExecutesCallsTailRecursionAndClosures()
    {
        var source = "Rule { main { " +
            "calculate: fn(value: int) -> int { if value > 0 { helper(value) } else { 0 } }, " +
            "helper: fn(value: int) -> int { value * 2 + 1 }, " +
            "tail: fn(value: int) -> int { if value <= 0 { 0 } else { tail(value - 1) } }, " +
            "make: fn(offset: int) -> fn(int) -> int { fn(value: int) -> int { value + offset } } } }";
        var module = Coflow.LoadAndCompile(new[] { source }, Contract(new RuleMetadata()));
        var rule = module.Table(Rules).Get("main").Value;
        Assert.Equal(11, rule.Calculate(5));
        Assert.Equal(0, rule.Tail(1_000));
        Assert.Equal(9, rule.Make(4)(5));
    }

    [Fact]
    public void VmReusesTheRegisterWindowAcrossMutualTailCalls()
    {
        var source = "Rule { main { " +
            "calculate: fn(value: int) -> int { " +
            "if value <= 0 { 1 } else { helper(value - 1) } }, " +
            "helper: fn(value: int) -> int { " +
            "if value <= 0 { 0 } else { calculate(value - 1) } } } }";
        var rule = Coflow.LoadAndCompile(new[] { source }, Contract(new RuleMetadata()))
            .Table(Rules).Get("main").Value;

        Assert.Equal(1, rule.Calculate(1_000));
        Assert.Equal(0, rule.Calculate(1_001));
    }

    [Fact]
    public void OverflowProducesSourceMappedFault()
    {
        var module = Coflow.LoadAndCompile(new[]
            { "Rule { main { calculate: fn(value: int) -> int { value + 1 } } }" }, Contract(new RuleMetadata()));
        var fault = Assert.Throws<CoflowFaultException>(() => module.Table(Rules).Get("main").Value.Calculate(long.MaxValue));
        Assert.Equal("Rule", fault.Function.DeclaredType);
        Assert.NotNull(fault.SourceSpan);
    }

    [Fact]
    public void VmFaultContainsCrossFunctionCallStack()
    {
        var source = "Rule { main { " +
            "calculate: fn(value: int) -> int { helper(value) + 1 }, " +
            "helper: fn(value: int) -> int { value / 0 } } }";
        var rule = Coflow.LoadAndCompile(new[] { source }, Contract(new RuleMetadata()))
            .Table(Rules).Get("main").Value;

        var fault = Assert.Throws<CoflowFaultException>(() => rule.Calculate(3));
        Assert.Contains(fault.CallStack, item => item.FieldName == "calculate");
        Assert.Contains(fault.CallStack, item => item.FieldName == "helper");
        Assert.NotNull(fault.SourceSpan);
    }

    [Fact]
    public void CompilerFoldsConstantsAndEliminatesDeadCode()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { return if true { 1 + 2 } else { 99 }; 100 } } }";
        var rule = Coflow.LoadAndCompile(new[] { source }, Contract(new RuleMetadata()))
            .Table(Rules).Get("main").Value;

        Assert.Equal(3, rule.Calculate(0));
        var program = rule.CalculateEntry.RuntimeEntry.CompiledProgram!;
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
        var rule = Coflow.LoadAndCompile(new[] { source }, Contract(new RuleMetadata()))
            .Table(Rules).Get("main").Value;

        Assert.Equal(7, rule.Calculate(7));
        Assert.DoesNotContain(
            rule.CalculateEntry.RuntimeEntry.CompiledProgram!.RegisterProgram.Instructions,
            instruction => instruction.Code == CoflowRegisterOpCode.MoveValue);
    }

    [Fact]
    public void CompilerEmitsExplicitRegisterOperands()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { value + 1 } } }";
        var rule = Coflow.LoadAndCompile(new[] { source }, Contract(new RuleMetadata()))
            .Table(Rules).Get("main").Value;

        Assert.Equal(8, rule.Calculate(7));
        var instructions = rule.CalculateEntry.RuntimeEntry.CompiledProgram!
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
    public void VmRunsThroughTheAotCompatibleExpressionInterpreter()
    {
        var before = CoflowExpressionCompiler.InterpretedCompilationCount;
        using (CoflowExpressionCompiler.OverrideDynamicCodeSupportForCurrentThread(false))
        {
            var source = "Rule { main { " +
                "calculate: fn(value: int) -> int { " +
                "[value, value + 1].map(fn(item: int) -> int { item * 2 })" +
                ".fold(0, fn(total: int, item: int) -> int { total + item }) }, " +
                "make: fn(offset: int) -> fn(int) -> int { " +
                "fn(value: int) -> int { value + offset } } } }";
            var rule = Coflow.LoadAndCompile(new[] { source }, Contract(new RuleMetadata()))
                .Table(Rules).Get("main").Value;

            Assert.Equal(14, rule.Calculate(3));
            Assert.Equal(9, rule.Make(4)(5));
        }
        Assert.True(CoflowExpressionCompiler.InterpretedCompilationCount > before);
    }

    [Fact]
    public void GeneratedDelegateAdaptersCoverReturnedVmClosures()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { value }, " +
            "make: fn(offset: int) -> fn(int) -> int { " +
            "fn(value: int) -> int { value + offset } } } }";
        var rule = Coflow.LoadAndCompile(new[] { source }, Contract(new RuleMetadata()))
            .Table(Rules).Get("main").Value;

        Assert.True(CoflowFunctionDelegates.HasGeneratedAdapter<Func<long, long>>());
        Assert.Equal(9, rule.Make(4)(5));
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
        var rule = Coflow.LoadAndCompile(new[] { source }, Contract(new RuleMetadata()))
            .Table(Rules).Get("main").Value;

        Assert.Equal(depth + 1, rule.Calculate(1));
        Assert.Equal(depth, rule.Deep(depth));
        Assert.Equal(depth + 2, rule.Make(1)(1));
    }

    [Fact]
    public void VmExecutesLoopControlRangesIndexesAndCompoundAssignments()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { " +
            "var total = 0; for item, index in 0..value { " +
            "if item == 1 { continue; }; if item == 4 { break; }; " +
            "total += item + index; } " +
            "total -= 1; total *= 2; total /= 2; total } } }";
        var rule = Coflow.LoadAndCompile(new[] { source }, Contract(new RuleMetadata()))
            .Table(Rules).Get("main").Value;

        Assert.Equal(9, rule.Calculate(10));
    }

    [Fact]
    public void VmExecutesTerminatingLoopsAndInclusiveRangeAtIntegerLimit()
    {
        var source = "Rule { main { " +
            "calculate: fn(value: int) -> int { " +
            "for item in value..=9223372036854775807 { return item; } 0 }, " +
            "helper: fn(value: int) -> int { while value > 0 { return 7; } 3 } } }";
        var rule = Coflow.LoadAndCompile(new[] { source }, Contract(new RuleMetadata()))
            .Table(Rules).Get("main").Value;

        Assert.Equal(long.MaxValue, rule.Calculate(long.MaxValue));
        Assert.Equal(7, rule.HelperEntry.Function(1));
        Assert.Equal(3, rule.HelperEntry.Function(0));
        Assert.DoesNotContain(rule.CalculateEntry.RuntimeEntry.CompiledProgram!
            .RegisterProgram.Instructions,
            instruction => instruction.Code == CoflowRegisterOpCode.Native);
    }

    [Fact]
    public void VmExecutesPrecompiledRegexWithoutLeakingThePatternOntoTheStack()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { " +
            "if \"abc\".matches(\"^a.*c$\") { value } else { 0 } } } }";
        var rule = Coflow.LoadAndCompile(new[] { source }, Contract(new RuleMetadata()))
            .Table(Rules).Get("main").Value;

        Assert.Equal(7, rule.Calculate(7));
    }

    [Fact]
    public void CompilerAcceptsExhaustiveBooleanMatchPatterns()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { " +
            "match value > 0 { true => value, false => 0 } } } }";
        var rule = Coflow.LoadAndCompile(new[] { source }, Contract(new RuleMetadata()))
            .Table(Rules).Get("main").Value;

        Assert.Equal(7, rule.Calculate(7));
        Assert.Equal(0, rule.Calculate(-1));
    }

    [Fact]
    public void VmFormatsEscapedBracesArgumentsAndContextMetadata()
    {
        var source = "Rule { main { calculate: fn(value: int) -> int { " +
            "\"{{ok}} {$id} {value}\".len() } } }";
        var rule = Coflow.LoadAndCompile(new[] { source }, Contract(new RuleMetadata()))
            .Table(Rules).Get("main").Value;

        Assert.Equal(11, rule.Calculate(7));
    }

    [Fact]
    public void CompilerReportsIndependentFunctionErrorsWithSourceLocations()
    {
        var source = "Rule { main { " +
            "calculate: fn(value: int) -> int { \"wrong\" }, " +
            "helper: fn(value: int) -> int { unknown(value) } } }";

        var error = Assert.Throws<CoflowLoadException>(() =>
            Coflow.LoadAndCompile(new[] { source }, Contract(new RuleMetadata())));

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
            Coflow.LoadAndCompile(new[] { source }, Contract(new RuleMetadata())));

        Assert.Contains(error.Diagnostics, item => item.Code == code);
        Assert.All(error.Diagnostics, item => Assert.NotNull(item.Span));
    }

    private static CoflowModule LoadData(string source) =>
        Coflow.LoadData(new[] { source }, Contract(new NodeMetadata()));
    private static ICoflowGeneratedContract Contract(params ICoflowTypeMetadata[] metadata) => new TestContract(metadata);

    private sealed class TestContract(params ICoflowTypeMetadata[] types) : ICoflowGeneratedContract
    {
        public IReadOnlyList<ICoflowTypeMetadata> Types { get; } = types;
        public IReadOnlyList<ICoflowEnumMetadata> Enums { get; } = Array.Empty<ICoflowEnumMetadata>();
        public IReadOnlyList<CoflowConstant> Constants { get; } = Array.Empty<CoflowConstant>();
    }

    private sealed class Node
    {
        public string Id { get; internal set; } = string.Empty;
        public long Value { get; internal set; }
        public Option<Node> Next { get; internal set; }
    }

    private sealed class Settings { public long Value { get; internal set; } }

    private enum TestMode { Primary = 1, Secondary = 2 }
    private sealed class EnumSettings { public TestMode Mode { get; internal set; } }

    private sealed class Rule
    {
        public string Id { get; internal set; } = string.Empty;
        internal CoflowFunctionEntry<Func<long, long>> CalculateEntry = default!;
        internal CoflowFunctionEntry<Func<long, long>> HelperEntry = default!;
        internal CoflowFunctionEntry<Func<long, long>> TailEntry = default!;
        internal CoflowFunctionEntry<Func<long, Func<long, long>>> MakeEntry = default!;
        internal CoflowFunctionEntry<Func<long, long>> DeepEntry = default!;
        public long Calculate(long value) => CalculateEntry.Function(value);
        public long Tail(long value) => TailEntry.Function(value);
        public Func<long, long> Make(long value) => MakeEntry.Function(value);
        public long Deep(long value) => DeepEntry.Function(value);
    }

    private abstract class Metadata<T> : ICoflowRecordMetadata where T : class, new()
    {
        public Type RuntimeType => typeof(T);
        public Type KeyType => typeof(string);
        public virtual bool IsSingleton => false;
        public bool IsAbstract => false;
        public bool IsSealed => true;
        public abstract string DeclaredType { get; }
        public IReadOnlyList<string> AssignableTypes => new[] { DeclaredType };
        public IReadOnlyList<CoflowAnnotation> Annotations => Array.Empty<CoflowAnnotation>();
        public abstract IReadOnlyList<string> FieldNames { get; }
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
        public abstract void PopulateRecord(object target, CfdRecordNode record, CfdLoadContext context);
        public object Read(CfdRecordNode record, CfdLoadContext context)
        {
            var target = new T();
            PopulateRecord(target, record, context);
            return target;
        }
    }

    private sealed class NodeMetadata : Metadata<Node>
    {
        private static readonly CoflowFieldBinding Value =
            CoflowFieldBinding.Create<Node, long>("value", static node => node.Value);
        private static readonly CoflowFieldBinding Next =
            CoflowFieldBinding.Create<Node, Option<Node>>("next", static node => node.Next);

        public override string DeclaredType => "Node";
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
        private static readonly IReadOnlyDictionary<string, CoflowFieldBinding> Fields =
            new Dictionary<string, CoflowFieldBinding>(StringComparer.Ordinal)
            {
                ["calculate"] = CoflowFieldBinding.Create<Rule, CoflowFunctionEntry>(
                    "calculate", static rule => rule.CalculateEntry.RuntimeEntry),
                ["helper"] = CoflowFieldBinding.Create<Rule, CoflowFunctionEntry>(
                    "helper", static rule => rule.HelperEntry.RuntimeEntry),
                ["tail"] = CoflowFieldBinding.Create<Rule, CoflowFunctionEntry>(
                    "tail", static rule => rule.TailEntry.RuntimeEntry),
                ["make"] = CoflowFieldBinding.Create<Rule, CoflowFunctionEntry>(
                    "make", static rule => rule.MakeEntry.RuntimeEntry),
                ["deep"] = CoflowFieldBinding.Create<Rule, CoflowFunctionEntry>(
                    "deep", static rule => rule.DeepEntry.RuntimeEntry),
            };

        public override string DeclaredType => "Rule";
        public override IReadOnlyList<string> FieldNames => Fields.Keys.ToArray();
        public override CoflowFieldBinding GetFieldBinding(string name) => Fields.TryGetValue(name, out var binding)
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
            var entry = context.Function(CfdValueReader.FindField(record.Fields, "make"), "make", typeof(Func<long, long>), typeof(long));
            rule.MakeEntry = CoflowFunctionEntry<Func<long, Func<long, long>>>.CreateAot(entry,
                static value => argument => value.Invoke<long, Func<long, long>>(argument));
            rule.DeepEntry = Entry(context, record, "deep");
        }
        private static CoflowFunctionEntry<Func<long, long>> Entry(
            CfdLoadContext context, CfdRecordNode record, string name, bool required = false)
        {
            var node = CfdValueReader.FindField(record.Fields, name);
            var entry = required
                ? context.RequiredFunction(node, name, typeof(long), typeof(long))
                : context.Function(node, name, typeof(long), typeof(long));
            return CoflowFunctionEntry<Func<long, long>>.CreateAot(entry,
                static value => argument => value.Invoke<long, long>(argument));
        }
    }
}
