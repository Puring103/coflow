using Coflow.Runtime;


var exampleRoot = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../../../"));
var charactersSource = File.ReadAllText(Path.Combine(exampleRoot, "data", "characters.cfd"));
var scenarioSource = File.ReadAllText(Path.Combine(exampleRoot, "data", "scenario.cfd"));
var traces = new List<string>();
var hostCalls = 0;
var root = Schema.Create();
var host = new HostServices(
    "integration",
    traces.Add,
    (value, operation) =>
    {
        hostCalls++;
        return operation.Invoke(root, value + 1) + 100;
    },
    operation => operation,
    value => $"[integration] {value}",
    value => value.HasValue
        ? Result<long, string>.Ok(value.Value + 1)
        : Result<long, string>.Err("missing"),
    value => new Stats(value.Health + 1, value.Attack + 2,
        new Dictionary<string, long>(value.Resistances)));
var charactersModule = root.LoadModule(new CoflowSource("characters.cfd", charactersSource));
var scenarioModule = root.LoadModule(new CoflowSource("scenario.cfd", scenarioSource));
root.Bind(host);
Compile(root);

var characters = root.Table(Character.Table);
var settings = Require(root.Singleton<RuntimeSettings>(), "RuntimeSettings");
var arcanist = Require(characters.Get(CharacterId.Arcanist), "arcanist");
var guardian = Require(characters.Get(CharacterId.Guardian), "guardian");
var scenario = Require(root.Table(Scenario.Table).Get("fullRoundTrip"), "fullRoundTrip");

Assert(arcanist.Stats.Attack == 19, "nested object was not loaded");
Assert(settings.Environment == "integration" && settings.Retries == 3,
    "ordinary singleton record or its schema default was not loaded");
Assert(arcanist.Stats.Resistances["ice"] == 9, "nested dictionary was not loaded");
Assert(arcanist.Tags.SequenceEqual(new[] { "caster", "advanced" }), "array was not loaded");
Assert(arcanist.Class == CharacterClass.Arcanist,
    "enum field was not generated or loaded correctly");
Assert(arcanist.Traits == (CharacterTrait.Ranged | CharacterTrait.Magical),
    "flag enum combination was not preserved");
Assert(arcanist.Enabled && guardian.Enabled,
    "schema field default was not materialized");
Assert(arcanist.PrimaryAbility is DamageAbility damageAbility && damageAbility.Damage == 35,
    "polymorphic inline object lost its concrete type");
Assert(arcanist.Abilities.Count == 2 && arcanist.Abilities[1] is HealAbility,
    "polymorphic object collection was not loaded");
Assert(arcanist.Status.IsOk && arcanist.Status.Value == 0 &&
       guardian.Status.IsErr && guardian.Status.Error == "resting",
    "Result default or explicit error branch was not loaded");
Assert(arcanist.Fallback.HasValue && ReferenceEquals(arcanist.Fallback.Value, guardian),
    "optional record reference was not resolved");
Assert(scenario.Config.OptionalBonus.HasValue && scenario.Config.OptionalBonus.Value.Attack == 5,
    "nested Option was not loaded");
Assert(scenario.Config.Validation.IsOk && scenario.Config.Validation.Value.HasValue &&
       scenario.Config.Validation.Value.Value.Health == 1,
    "nested Result<Option<T>> was not loaded");
Assert(scenario.Config.Checkpoints["finish"].Resistances["ice"] == 5,
    "nested map object was not loaded");

// Host -> VM, followed by VM -> Host trace/decorate calls and VM higher-order collection calls.
var executeResult = scenario.Execute(root, 5);
Assert(executeResult == 87, $"host-to-CFD execution returned {executeResult} instead of 87");
Assert(traces.SequenceEqual(new[] { "[integration] higher-order" }),
    "CFD-to-host calls did not preserve their order or arguments");

// VM passes a captured VM closure to C#; C# invokes it before returning to the VM.
Assert(scenario.CallHost(root, 4) == 120, "VM closure passed to the host returned the wrong result");
Assert(hostCalls == 1, "host higher-order function was not called exactly once");

// VM returns a captured closure to C#. C# invokes it and then passes the same delegate back to VM.
CoflowFunction<long, long> scaler = scenario.MakeScaler(root, 3);
Assert(scaler.Invoke(root, 4) == 31, "VM closure returned to the host lost its captured values");
Assert(scenario.Apply(root, 4, scaler) == 56,
    "VM closure returned to C# could not be passed back into the VM");
Assert(scenario.HostFunction(root, 6).Invoke(root, 4) == 10,
    "a function handle returned by the Host lost its VM closure environment");
Assert(scenario.OptionalFunction(root, Option<CoflowFunction<long, long>>.Some(scaler))
           .Value.Invoke(root, 4) == 31,
    "an Option-wrapped function handle did not preserve both ID lanes");
Assert(scenario.ResultFunction(root,
           Result<CoflowFunction<long, long>, string>.Ok(scaler)).Value.Invoke(root, 4) == 31,
    "a Result-wrapped function handle did not preserve both ID lanes");

// default 函数句柄允许作为值传递，但只有真正调用时才报告缺失。
try
{
    _ = scenario.Compose(root, default, scaler).Invoke(root, 1);
    throw new InvalidOperationException("A missing function handle unexpectedly executed.");
}
catch (CoflowFaultException error) when (error.InnerException is CoflowFunctionNotBoundException)
{
}
// Dedicated VM workloads are also used by the runtime benchmark project.
Assert(scenario.IntegerLoop(root, 1_000) == 499_500, "integer loop returned the wrong result");
Assert(scenario.FloatLoop(root, 1_000) == 500.0, "floating-point loop returned the wrong result");
Assert(scenario.NumericConversions(root, 4, 5.75) == 5 && scenario.NumericConversions(root, 6, 5.75) == 6,
    "numeric conversions or floating-point comparison returned the wrong result");
Assert(scenario.EnumRoundTrip(root, CharacterId.Arcanist) && !scenario.EnumRoundTrip(root, CharacterId.Guardian),
    "enum VM boundary or equality returned the wrong result");
Assert(scenario.DirectCallChain(root, 10) == 14, "direct CFD call chain returned the wrong result");
Assert(scenario.TailRecursion(root, 1_000) == 0, "tail recursion returned the wrong result");
Assert(scenario.TailAccumulator(root, 1_000, 0) == 500_500,
    "tail-call argument copying corrupted overlapping register windows");
Assert(scenario.FieldReadLoop(root, 10) == 190, "generated field loop returned the wrong result");
Assert(scenario.CollectionPipeline(root, 6) == 30, "collection pipeline returned the wrong result");
Assert(scenario.HostCall(root, 7) == 7 && traces[^1] == "benchmark",
    "direct CFD-to-host call returned the wrong result");
Assert(scenario.HostComposite(root, Option<long>.Some(4)).Value == 5 &&
       scenario.HostComposite(root, Option<long>.None).Error == "missing",
    "Host composite argument/result layouts were not preserved");
Assert(!scenario.PropagateOption(root, Option<long>.None).HasValue &&
       scenario.PropagateOption(root, Option<long>.Some(4)).Value == 5,
    "Option propagation did not preserve the structural tag/payload layout");
Assert(scenario.PropagateResult(root, Result<long, string>.Err("failed")).Error == "failed" &&
       scenario.PropagateResult(root, Result<long, string>.Ok(4)).Value == 5,
    "Result propagation did not preserve the structural error/value layout");
Assert(scenario.PropagateNested(root, Result<Result<long, string>, string>.Err("outer")).Error == "outer" &&
       scenario.PropagateNested(root, Result<Result<long, string>, string>.Ok(
           Result<long, string>.Err("inner"))).Error == "inner" &&
       scenario.PropagateNested(root, Result<Result<long, string>, string>.Ok(
           Result<long, string>.Ok(4))).Value == 5,
    "nested propagation did not handle differing structural payload widths");
Assert(scenario.MakeOptionalAdder(root, Option<long>.Some(3)).Invoke(root, 4) == 7 &&
       scenario.MakeOptionalAdder(root, Option<long>.None).Invoke(root, 4) == 4,
    "closure capture did not preserve an Option payload layout");
Assert(scenario.CollectionQueries(root, 4) == 114 && scenario.CollectionQueries(root, 8) == 100,
    "find/any/all lowering returned the wrong result");
Assert(scenario.EmptyCollectionQueries(root) == 107,
    "higher-order lowering returned the wrong empty-collection identities");

for (var index = 0; index < 32; index++)
{
    _ = scenario.IntegerLoop(root, 10);
    _ = scenario.DirectCallChain(root, 10);
    _ = scenario.FieldReadLoop(root, 10);
    _ = scaler.Invoke(root, 4);
}
var integerAllocationStart = GC.GetAllocatedBytesForCurrentThread();
for (var index = 0; index < 1_000; index++)
    _ = scenario.IntegerLoop(root, 10);
var integerLoopAllocations = GC.GetAllocatedBytesForCurrentThread() - integerAllocationStart;
var directAllocationStart = GC.GetAllocatedBytesForCurrentThread();
for (var index = 0; index < 1_000; index++)
    _ = scenario.DirectCallChain(root, 10);
var directCallAllocations = GC.GetAllocatedBytesForCurrentThread() - directAllocationStart;
var fieldReadAllocationStart = GC.GetAllocatedBytesForCurrentThread();
for (var index = 0; index < 1_000; index++)
    _ = scenario.FieldReadLoop(root, 10);
var fieldReadAllocations = GC.GetAllocatedBytesForCurrentThread() - fieldReadAllocationStart;
var closureAllocationStart = GC.GetAllocatedBytesForCurrentThread();
for (var index = 0; index < 1_000; index++)
    _ = scaler.Invoke(root, 4);
var closureAllocations = GC.GetAllocatedBytesForCurrentThread() - closureAllocationStart;
Assert(integerLoopAllocations == 0 && directCallAllocations == 0 && fieldReadAllocations == 0 && closureAllocations == 0,
    $"warmed VM allocations: integer={integerLoopAllocations}, direct={directCallAllocations}, field={fieldReadAllocations}, closure={closureAllocations}");

var constructedStats = scenario.MakeStats(root, 7);
Assert(constructedStats.Health == 7 && constructedStats.Attack == 8 &&
       constructedStats.Resistances["typed"] == 9,
    "typed object construction returned the wrong object");
var defaultStats = scenario.MakeDefaultStats(root, 7);
Assert(defaultStats.Health == 7 && defaultStats.Attack == 8 && defaultStats.Resistances.Count == 0,
    "typed object construction did not apply a generated field default");
var externalResistances = new Dictionary<string, long> { ["bonus"] = 4 };
var externalStats = new Stats(10, 20, externalResistances);
try
{
    _ = scenario.ReadExternalStats(root, default);
    throw new InvalidOperationException("An uninitialized default struct unexpectedly entered the VM.");
}
catch (CoflowFaultException error) when (error.InnerException is CoflowBoundaryException)
{
}
var externalStatsResult = scenario.ReadExternalStats(root, externalStats);
Assert(externalStatsResult == 34,
    $"an externally constructed struct was not imported with its default function: {externalStatsResult}");
Assert(scenario.HostStats(root, externalStats) == 37,
    "a Host-returned struct did not use the common import and default-function path");
var externalTags = new List<string> { "external" };
var externalAttributes = new Dictionary<string, long> { ["rank"] = 3 };
var externalAbilities = new List<Ability> { new DamageAbility("external-hit", "Hit", 8) };
var externalCharacter = new Character(
    CharacterId.Guardian,
    "External",
    CharacterClass.Guardian,
    CharacterTrait.Durable,
    true,
    externalStats,
    externalTags,
    externalAttributes,
    new DamageAbility("external-primary", "Primary", 12),
    externalAbilities,
    Result<long, string>.Ok(5),
    Option<Character>.None);
var copiedCharacter = scenario.CopyExternalCharacter(root, externalCharacter);
externalTags.Add("mutated");
externalAttributes["rank"] = 99;
externalAbilities.Clear();
Assert(copiedCharacter.Tags.SequenceEqual(new[] { "external" }) &&
       copiedCharacter.Attributes["rank"] == 3 && copiedCharacter.Abilities.Count == 1,
    "external class collections were not copied and frozen");
Assert(copiedCharacter.Power(root, 2) == 12,
    "a VM-returned class did not retain a callable current-snapshot identity");
Assert(scenario.ReadExternalCharacterCollections(root, copiedCharacter) == 3,
    "an escaped external class lost its collection Arena before the next VM call");
var copiedOptionalStats = scenario.CopyOptionalStats(root, Option<Stats>.Some(externalStats));
Assert(copiedOptionalStats.Value.Score(root, 4) == 34,
    "a generated value nested in an Option was not promoted to the escape store");
var formatted = scenario.FormatValues(root, 4, 1.5, true, Option<long>.Some(6));
Assert(formatted ==
       "value=4, ratio=1.5, enabled=true, optional=Some(6), stats=Some(Stats { health: 3, attack: 5, resistances: { \"arcane\": 11 } })",
    $"typed interpolation returned the wrong text: {formatted}");

Assert(scenario.SyntaxControlFlow(root, 5) == 42,
    "for/range/break/continue or compound assignment syntax returned the wrong result");
Assert(scenario.SyntaxOperators(root, 8, 3, "a"),
    "unary, arithmetic, bitwise, comparison, or logical operator syntax returned the wrong result");
Assert(scenario.SyntaxMatch(root, 0, Option<long>.Some(4), Result<long, string>.Err("bad"), CharacterId.Arcanist) == 19 &&
       scenario.SyntaxMatch(root, -1, Option<long>.None, Result<long, string>.Ok(5), CharacterId.Guardian) == 26,
    "literal, Option, Result, bool, or enum match syntax returned the wrong result");
Assert(scenario.TypeMetadata(root) ==
       "Scenario|typeMetadata|typeMetadata|fullRoundTrip|Scenario::fullRoundTrip|arcanist|Character::arcanist",
    "type predicates, type patterns, or Coflow metadata returned the wrong value");
Assert(scenario.BuiltinSyntax(root, "abc"), "built-in function syntax returned the wrong result");
var syntaxFormatted = scenario.FormatSyntax(root, "line\n\"quoted\"");
Assert(syntaxFormatted ==
       "literal={ok} text=line\n\"quoted\" owner=Scenario::fullRoundTrip hero=Character::arcanist config=Some(Stats { health: 3, attack: 5, resistances: { \"arcane\": 11 } })",
    $"escaped or metadata interpolation returned the wrong text: {syntaxFormatted}");
Assert(scenario.PrimeSum(root, 20) == 77, "prime-sum algorithm returned the wrong result");
Assert(scenario.MatrixKernel(root, 3) == 135, "matrix-kernel algorithm returned the wrong result");
Assert(scenario.Fibonacci(root, 20) == 6_765, "non-tail Fibonacci returned the wrong result");
root.Bind(new HostServices(
    "reconfigured",
    traces.Add,
    (value, operation) => operation.Invoke(root, value) + 7,
    operation => operation,
    static value => $"[reconfigured] {value}",
    static value => value.HasValue
        ? Result<long, string>.Ok(value.Value + 2)
        : Result<long, string>.Err("reconfigured-missing"),
    static value => value));
Compile(root);
scenario = Require(root.Table(Scenario.Table).Get("fullRoundTrip"), "fullRoundTrip");
AssertThrows<CoflowStaleValueException>(() => copiedCharacter.Power(root, 2));
Assert(scenario.CallHost(root, 4) == 23,
    "recompiled Host delegates were not observed by the published snapshot");
Assert(scenario.HostComposite(root, Option<long>.Some(4)).Value == 6,
    "reconfigured composite Host delegate was not observed");
root.Bind(new HostServices(
    "integration",
    traces.Add,
    (value, operation) => operation.Invoke(root, value + 1) + 100,
    operation => operation,
    value => $"[integration] {value}",
    value => value.HasValue
        ? Result<long, string>.Ok(value.Value + 1)
        : Result<long, string>.Err("missing"),
    static value => value));
Compile(root);
scenario = Require(root.Table(Scenario.Table).Get("fullRoundTrip"), "fullRoundTrip");

// Module replacement recompiles the global snapshot and invalidates execution through old values.
var updatedCharacters = charactersSource.Replace("attack: 19", "attack: 20", StringComparison.Ordinal);
var staleScenario = scenario;
root.ReplaceModule(charactersModule, new CoflowSource("characters.cfd", updatedCharacters));
Compile(root);
var reloadedScenario = Require(root.Table(Scenario.Table).Get("fullRoundTrip"), "fullRoundTrip");
Assert(reloadedScenario.Execute(root, 5) == 90,
    "replacement snapshot did not contain the updated data");
Assert(reloadedScenario.MakeScaler(root, 3).Invoke(root, 4) == 32,
    "replacement closure did not capture updated data");
AssertThrows<CoflowStaleValueException>(() => staleScenario.Execute(root, 5));

// Host 返回值无论从生成 API 还是 VM 调用，都经过相同的边界验证。
var invalidHostRuntime = Schema.Create();
invalidHostRuntime.LoadModule(
    new CoflowSource("characters.cfd", charactersSource),
    new CoflowSource("scenario.cfd", scenarioSource));
invalidHostRuntime.Bind(new HostServices(
    "invalid-host",
    static _ => { },
    static (value, _) => value,
    static operation => operation,
    static _ => null!,
    static _ => Result<long, string>.Ok(0),
    static value => value));
Compile(invalidHostRuntime);
var invalidHost = Require(invalidHostRuntime.Singleton<HostServices>(), "HostServices");
var invalidHostScenario = Require(
    invalidHostRuntime.Table(Scenario.Table).Get("fullRoundTrip"), "fullRoundTrip");
AssertThrows<CoflowFaultException>(() => invalidHost.Decorate(invalidHostRuntime, "value"));
AssertThrows<CoflowFaultException>(() => invalidHostScenario.Execute(invalidHostRuntime, 1));

Console.WriteLine("csharp-runtime-integration-ok");

static T Require<T>(Option<T> value, string key) => value.HasValue
    ? value.Value
    : throw new InvalidOperationException($"Missing generated record '{key}'.");

static void Assert(bool condition, string message)
{
    if (!condition) throw new InvalidOperationException(message);
}

static void AssertThrows<TException>(Action action) where TException : Exception
{
    try { action(); }
    catch (TException) { return; }
    throw new InvalidOperationException($"Expected {typeof(TException).Name}.");
}

static void Compile(global::Coflow.Runtime.Coflow coflow)
{
    var result = coflow.Compile();
    if (result.Success) return;
    foreach (var diagnostic in result.Diagnostics) Console.Error.WriteLine(diagnostic);
    throw new CoflowLoadException(result.Diagnostics);
}
