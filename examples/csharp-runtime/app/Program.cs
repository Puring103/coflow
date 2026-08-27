using Integration.Config.integration.domain;
using Integration.Config.integration.runtime;
using CoflowRuntime;

var exampleRoot = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../../../"));
var charactersSource = File.ReadAllText(Path.Combine(exampleRoot, "data", "characters.cfd"));
var scenarioSource = File.ReadAllText(Path.Combine(exampleRoot, "data", "scenario.cfd"));
using var charactersModule = Coflow.LoadData(charactersSource);
using var root = Load(new[] { scenarioSource }, charactersModule);
var traces = new List<string>();
var hostCalls = 0;
var host = root.Singleton<HostServices>();
AssertOk(host.Bind(
    "integration",
    traces.Add,
    (value, operation) =>
    {
        hostCalls++;
        return operation(value + 1) + 100;
    },
    value => $"[integration] {value}"));

var characters = root.Table<Character>();
var arcanist = Require(characters.Get(CharacterId.Arcanist), "arcanist");
var guardian = Require(characters.Get(CharacterId.Guardian), "guardian");
var scenario = Require(root.Table<Scenario>().Get("fullRoundTrip"), "fullRoundTrip");

Assert(arcanist.Stats.Attack == 19, "nested object was not loaded");
Assert(arcanist.Stats.Resistances["ice"] == 9, "nested dictionary was not loaded");
Assert(arcanist.Tags.SequenceEqual(new[] { "caster", "advanced" }), "array was not loaded");
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
Assert(scenario.DirectCallChain(10) == 14, "direct CFD call chain returned the wrong result");
Assert(scenario.TailRecursion(1_000) == 0, "tail recursion returned the wrong result");
Assert(scenario.FieldReadLoop(10) == 190, "generated field loop returned the wrong result");
Assert(scenario.CollectionPipeline(6) == 30, "collection pipeline returned the wrong result");
Assert(scenario.HostCall(7) == 7 && traces[^1] == "benchmark",
    "direct CFD-to-host call returned the wrong result");

// Reload one CFD child module. The root relinks its data and recompiles parent functions.
var updatedCharacters = charactersSource.Replace("attack: 19", "attack: 20", StringComparison.Ordinal);
var reload = root.Reload(charactersModule, updatedCharacters);
Assert(reload.IsOk, "child module reload failed");
var reloadedScenario = Require(root.Table<Scenario>().Get("fullRoundTrip"), "fullRoundTrip");
Assert(reloadedScenario.Execute(5) == 90,
    "parent CFD functions were not recompiled after child module reload");
Assert(reloadedScenario.MakeScaler(3)(4) == 32,
    "reloaded parent closure did not capture child module data");

Console.WriteLine("csharp-runtime-integration-ok");

static T Require<T>(Option<T> value, string key) => value.HasValue
    ? value.Value
    : throw new InvalidOperationException($"Missing generated record '{key}'.");

static void Assert(bool condition, string message)
{
    if (!condition) throw new InvalidOperationException(message);
}

static void AssertOk<TError>(Result<Unit, TError> result)
{
    if (result.IsErr)
        throw new InvalidOperationException($"Host binding failed: {result.Error}");
}

static CoflowModule Load(string[] sources, params CoflowModule[] children)
{
    try
    {
        return Coflow.LoadAndCompile(sources, children);
    }
    catch (CoflowLoadException error)
    {
        foreach (var diagnostic in error.Diagnostics)
            Console.Error.WriteLine(diagnostic);
        throw;
    }
}
