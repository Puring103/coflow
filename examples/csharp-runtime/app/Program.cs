using Integration.Config.integration.domain;
using Integration.Config.integration.runtime;
using CoflowRuntime;

var exampleRoot = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../../../"));
var cfdSources = Directory.GetFiles(Path.Combine(exampleRoot, "data"), "*.cfd")
    .OrderBy(path => path, StringComparer.Ordinal)
    .Select(File.ReadAllText)
    .ToArray();

using var data = Load(cfdSources);
var traces = new List<string>();
var hostCalls = 0;
var host = data.Singleton<HostServices>();
AssertOk(host.Bind(
    "integration",
    traces.Add,
    (value, operation) =>
    {
        hostCalls++;
        return operation(value + 1) + 100;
    },
    value => $"[integration] {value}"));

var characters = data.Table<Character>();
var arcanist = Require(characters.Get(CharacterId.Arcanist), "arcanist");
var guardian = Require(characters.Get(CharacterId.Guardian), "guardian");
var scenario = Require(data.Table<Scenario>().Get("fullRoundTrip"), "fullRoundTrip");

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

static CoflowData Load(string[] sources)
{
    try
    {
        return Coflow.LoadAndCompile(sources);
    }
    catch (CoflowLoadException error)
    {
        foreach (var diagnostic in error.Diagnostics)
            Console.Error.WriteLine(diagnostic);
        throw;
    }
}
