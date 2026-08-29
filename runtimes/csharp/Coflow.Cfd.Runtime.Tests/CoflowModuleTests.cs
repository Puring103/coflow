using CoflowRuntime;
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
        Assert.Equal(new[] { CoflowOpCode.Constant, CoflowOpCode.Return },
            program.Instructions.Select(instruction => instruction.Code));
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
        public abstract Type GetFieldType(string fieldName);
        public abstract object GetField(object record, string fieldName);
        public abstract Delegate GetFieldReader(string fieldName);
        public bool HasFieldDefault(string fieldName) => false;
        public object CreateObject(CfdLoadContext context, IReadOnlyDictionary<string, object?> fields) => throw new InvalidOperationException();
        public Delegate CreateVmObjectFactory(CfdLoadContext context) => throw new InvalidOperationException();
        public Delegate CreateVmDefaultFactory(string fieldName, CfdLoadContext context) => throw new ArgumentException(nameof(fieldName));
        public string? ObjectFieldType(string fieldName) => null;
        public virtual string? ReferenceFieldType(string fieldName) => null;
        public object ParseKey(string key) => key;
        public abstract object GetKey(object record);
        public Delegate GetKeyReader() => new Func<T, string>(record => (string)GetKey(record));
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
        public override string DeclaredType => "Node";
        public override IReadOnlyList<string> FieldNames => new[] { "value", "next" };
        public override Type GetFieldType(string name) => name switch
            { "value" => typeof(long), "next" => typeof(Option<Node>), _ => throw new ArgumentException(nameof(name)) };
        public override object GetField(object value, string name) => name switch
            { "value" => ((Node)value).Value, "next" => ((Node)value).Next, _ => throw new ArgumentException(nameof(name)) };
        public override Delegate GetFieldReader(string name) => name switch
            { "value" => new Func<Node, long>(static value => value.Value), "next" => new Func<Node, Option<Node>>(static value => value.Next), _ => throw new ArgumentException(nameof(name)) };
        public override string? ReferenceFieldType(string name) => name == "next" ? "Node" : null;
        public override object GetKey(object value) => ((Node)value).Id;
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
        public override string DeclaredType => "Settings";
        public override bool IsSingleton => true;
        public override IReadOnlyList<string> FieldNames => new[] { "value" };
        public override Type GetFieldType(string name) => typeof(long);
        public override object GetField(object value, string name) => ((Settings)value).Value;
        public override Delegate GetFieldReader(string name) => name == "value"
            ? new Func<Settings, long>(static value => value.Value)
            : throw new ArgumentException(nameof(name));
        public override object GetKey(object value) => string.Empty;
        public override void PopulateRecord(object target, CfdRecordNode record, CfdLoadContext context) =>
            ((Settings)target).Value = CfdValueReader.Int64(CfdValueReader.Field(record.Fields, "value"));
    }

    private sealed class RuleMetadata : Metadata<Rule>
    {
        public override string DeclaredType => "Rule";
        public override IReadOnlyList<string> FieldNames => new[] { "calculate", "helper", "tail", "make", "deep" };
        public override Type GetFieldType(string name) => typeof(CoflowFunctionEntry);
        public override object GetField(object value, string name)
        {
            var rule = (Rule)value;
            return name switch { "calculate" => rule.CalculateEntry.RuntimeEntry, "helper" => rule.HelperEntry.RuntimeEntry,
                "tail" => rule.TailEntry.RuntimeEntry, "make" => rule.MakeEntry.RuntimeEntry,
                "deep" => rule.DeepEntry.RuntimeEntry, _ => throw new ArgumentException(nameof(name)) };
        }
        public override Delegate GetFieldReader(string name) => name switch
        {
            "calculate" => new Func<Rule, CoflowFunctionEntry>(static value => value.CalculateEntry.RuntimeEntry),
            "helper" => new Func<Rule, CoflowFunctionEntry>(static value => value.HelperEntry.RuntimeEntry),
            "tail" => new Func<Rule, CoflowFunctionEntry>(static value => value.TailEntry.RuntimeEntry),
            "make" => new Func<Rule, CoflowFunctionEntry>(static value => value.MakeEntry.RuntimeEntry),
            "deep" => new Func<Rule, CoflowFunctionEntry>(static value => value.DeepEntry.RuntimeEntry),
            _ => throw new ArgumentException(nameof(name)),
        };
        public override object GetKey(object value) => ((Rule)value).Id;
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
