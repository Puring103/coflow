using Integration.Config.integration.domain;
using Integration.Config.integration.runtime;
using CoflowRuntime;

var exampleRoot = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../../../"));
var charactersSource = File.ReadAllText(Path.Combine(exampleRoot, "data", "characters.cfd"));
var scenarioSource = File.ReadAllText(Path.Combine(exampleRoot, "data", "scenario.cfd"));
var root = Load(charactersSource, scenarioSource);
var traces = new List<string>();
var hostCalls = 0;
var host = Require(root.Singleton<HostServices>(), "HostServices");
host.Configure(
    "integration",
    traces.Add,
    (value, operation) =>
    {
        hostCalls++;
        return operation(value + 1) + 100;
    },
    value => $"[integration] {value}",
    value => value.HasValue
        ? Result<long, string>.Ok(value.Value + 1)
        : Result<long, string>.Err("missing"));

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
Assert(scenario.Execute(5) == 87, "host-to-CFD execution returned the wrong result");
Assert(traces.SequenceEqual(new[] { "[integration] higher-order" }),
    "CFD-to-host calls did not preserve their order or arguments");

// VM passes a captured VM closure to C#; C# invokes it before returning to the VM.
Assert(scenario.CallHost(4) == 120, "VM closure passed to the host returned the wrong result");
Assert(hostCalls == 1, "host higher-order function was not called exactly once");

// VM returns a captured closure to C#. C# invokes it and then passes the same delegate back to VM.
Func<long, long> scaler = scenario.MakeScaler(3);
Assert(scaler(4) == 31, "VM closure returned to the host lost its captured values");
Assert(scenario.Apply(4, scaler) == 56,
    "VM closure returned to C# could not be passed back into the VM");

// Compose a host delegate and a VM closure in VM, return the new closure, then send it back once more.
Func<long, long> composed = scenario.Compose(value => value + 2, scaler);
Assert(composed(5) == 40, "mixed host/VM composition returned the wrong result");
Assert(scenario.Apply(5, composed) == 65,
    "composed higher-order function could not complete a second host/VM round trip");

// Dedicated VM workloads are also used by the runtime benchmark project.
Assert(scenario.IntegerLoop(1_000) == 499_500, "integer loop returned the wrong result");
Assert(scenario.FloatLoop(1_000) == 500.0, "floating-point loop returned the wrong result");
Assert(scenario.NumericConversions(4, 5.75) == 5 && scenario.NumericConversions(6, 5.75) == 6,
    "numeric conversions or floating-point comparison returned the wrong result");
Assert(scenario.EnumRoundTrip(CharacterId.Arcanist) && !scenario.EnumRoundTrip(CharacterId.Guardian),
    "enum VM boundary or equality returned the wrong result");
Assert(scenario.DirectCallChain(10) == 14, "direct CFD call chain returned the wrong result");
Assert(scenario.TailRecursion(1_000) == 0, "tail recursion returned the wrong result");
Assert(scenario.TailAccumulator(1_000, 0) == 500_500,
    "tail-call argument copying corrupted overlapping register windows");
Assert(scenario.FieldReadLoop(10) == 190, "generated field loop returned the wrong result");
Assert(scenario.CollectionPipeline(6) == 30, "collection pipeline returned the wrong result");
Assert(scenario.HostCall(7) == 7 && traces[^1] == "benchmark",
    "direct CFD-to-host call returned the wrong result");
Assert(scenario.HostComposite(Option<long>.Some(4)).Value == 5 &&
       scenario.HostComposite(Option<long>.None).Error == "missing",
    "Host composite argument/result layouts were not preserved");
Assert(!scenario.PropagateOption(Option<long>.None).HasValue &&
       scenario.PropagateOption(Option<long>.Some(4)).Value == 5,
    "Option propagation did not preserve the structural tag/payload layout");
Assert(scenario.PropagateResult(Result<long, string>.Err("failed")).Error == "failed" &&
       scenario.PropagateResult(Result<long, string>.Ok(4)).Value == 5,
    "Result propagation did not preserve the structural error/value layout");
Assert(scenario.PropagateNested(Result<Result<long, string>, string>.Err("outer")).Error == "outer" &&
       scenario.PropagateNested(Result<Result<long, string>, string>.Ok(
           Result<long, string>.Err("inner"))).Error == "inner" &&
       scenario.PropagateNested(Result<Result<long, string>, string>.Ok(
           Result<long, string>.Ok(4))).Value == 5,
    "nested propagation did not handle differing structural payload widths");
Assert(scenario.MakeOptionalAdder(Option<long>.Some(3))(4) == 7 &&
       scenario.MakeOptionalAdder(Option<long>.None)(4) == 4,
    "closure capture did not preserve an Option payload layout");
Assert(scenario.CollectionQueries(4) == 114 && scenario.CollectionQueries(8) == 100,
    "find/any/all lowering returned the wrong result");
Assert(scenario.EmptyCollectionQueries() == 107,
    "higher-order lowering returned the wrong empty-collection identities");

for (var index = 0; index < 32; index++)
{
    _ = scenario.IntegerLoop(10);
    _ = scenario.DirectCallChain(10);
    _ = scaler(4);
}
var allocationStart = GC.GetAllocatedBytesForCurrentThread();
for (var index = 0; index < 1_000; index++)
{
    _ = scenario.IntegerLoop(10);
    _ = scenario.DirectCallChain(10);
    _ = scaler(4);
}
var hotPathAllocations = GC.GetAllocatedBytesForCurrentThread() - allocationStart;
Assert(hotPathAllocations == 0,
    $"warmed VM calls allocated {hotPathAllocations} managed bytes");

var constructedStats = scenario.MakeStats(7);
Assert(constructedStats.Health == 7 && constructedStats.Attack == 8 &&
       constructedStats.Resistances["typed"] == 9,
    "typed object construction returned the wrong object");
var defaultStats = scenario.MakeDefaultStats(7);
Assert(defaultStats.Health == 7 && defaultStats.Attack == 8 && defaultStats.Resistances.Count == 0,
    "typed object construction did not apply a generated field default");
var formatted = scenario.FormatValues(4, 1.5, true, Option<long>.Some(6));
Assert(formatted ==
       "value=4, ratio=1.5, enabled=true, optional=Some(6), stats=Some(integration::domain::Stats { health: 3, attack: 5, resistances: { \"arcane\": 11 } })",
    $"typed interpolation returned the wrong text: {formatted}");

Assert(scenario.SyntaxControlFlow(5) == 42,
    "for/range/break/continue or compound assignment syntax returned the wrong result");
Assert(scenario.SyntaxOperators(8, 3, "a"),
    "unary, arithmetic, bitwise, comparison, or logical operator syntax returned the wrong result");
Assert(scenario.SyntaxMatch(0, Option<long>.Some(4), Result<long, string>.Err("bad"), CharacterId.Arcanist) == 19 &&
       scenario.SyntaxMatch(-1, Option<long>.None, Result<long, string>.Ok(5), CharacterId.Guardian) == 26,
    "literal, Option, Result, bool, or enum match syntax returned the wrong result");
Assert(scenario.TypeMetadata() ==
       "integration::runtime::Scenario|typeMetadata|typeMetadata|fullRoundTrip|integration::runtime::Scenario::fullRoundTrip|arcanist|integration::domain::Character::arcanist",
    "type predicates, type patterns, or Coflow metadata returned the wrong value");
Assert(scenario.BuiltinSyntax("abc"), "built-in function syntax returned the wrong result");
var syntaxFormatted = scenario.FormatSyntax("line\n\"quoted\"");
Assert(syntaxFormatted ==
       "literal={ok} text=line\n\"quoted\" owner=integration::runtime::Scenario::fullRoundTrip hero=integration::domain::Character::arcanist config=Some(integration::domain::Stats { health: 3, attack: 5, resistances: { \"arcane\": 11 } })",
    $"escaped or metadata interpolation returned the wrong text: {syntaxFormatted}");
Assert(scenario.PrimeSum(20) == 77, "prime-sum algorithm returned the wrong result");
Assert(scenario.MatrixKernel(3) == 135, "matrix-kernel algorithm returned the wrong result");
Assert(scenario.Fibonacci(20) == 6_765, "non-tail Fibonacci returned the wrong result");
host.Configure(
    "reconfigured",
    traces.Add,
    static (value, operation) => operation(value) + 7,
    static value => $"[reconfigured] {value}",
    static value => value.HasValue
        ? Result<long, string>.Ok(value.Value + 2)
        : Result<long, string>.Err("reconfigured-missing"));
Assert(scenario.CallHost(4) == 23,
    "reconfigured Host delegates were not observed by the existing module");
Assert(scenario.HostComposite(Option<long>.Some(4)).Value == 6,
    "reconfigured composite Host delegate was not observed");
host.Configure(
    "integration",
    traces.Add,
    (value, operation) => operation(value + 1) + 100,
    value => $"[integration] {value}",
    value => value.HasValue
        ? Result<long, string>.Ok(value.Value + 1)
        : Result<long, string>.Err("missing"));

// Replacement publishes a new immutable module/set and leaves the old snapshot valid.
var updatedCharacters = charactersSource.Replace("attack: 19", "attack: 20", StringComparison.Ordinal);
var updated = Load(updatedCharacters, scenarioSource);
Require(updated.Singleton<HostServices>(), "HostServices").Configure(
    "integration", traces.Add,
    (value, operation) => operation(value + 1) + 100,
    value => $"[integration] {value}",
    value => value.HasValue
        ? Result<long, string>.Ok(value.Value + 1)
        : Result<long, string>.Err("missing"));
var oldSet = Coflow.Modules(root);
var newSet = oldSet.Replace(root, updated);
var reloadedScenario = Require(newSet.Table(Scenario.Table).Get("fullRoundTrip"), "fullRoundTrip");
Assert(reloadedScenario.Execute(5) == 90,
    "replacement snapshot did not contain the updated data");
Assert(reloadedScenario.MakeScaler(3)(4) == 32,
    "replacement closure did not capture updated data");
Assert(Require(oldSet.Table(Scenario.Table).Get("fullRoundTrip"), "fullRoundTrip").Execute(5) == 87,
    "replacement mutated the old module set");

Console.WriteLine("csharp-runtime-integration-ok");

static T Require<T>(Option<T> value, string key) => value.HasValue
    ? value.Value
    : throw new InvalidOperationException($"Missing generated record '{key}'.");

static void Assert(bool condition, string message)
{
    if (!condition) throw new InvalidOperationException(message);
}

static CoflowModule Load(params string[] sources)
{
    try
    {
        return Integration.Config.CoflowData.LoadAndCompile(sources);
    }
    catch (CoflowLoadException error)
    {
        foreach (var diagnostic in error.Diagnostics)
            Console.Error.WriteLine(diagnostic);
        throw;
    }
}
