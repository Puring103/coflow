using Coflow.Runtime;

var project = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../../../"));
var coreSource = File.ReadAllText(Path.Combine(project, "data/core.cfd"));
var gameplaySource = File.ReadAllText(Path.Combine(project, "data/gameplay.cfd"));
var coflow = Schema.Create();
var core = coflow.LoadModule(new CoflowSource("data/core.cfd", coreSource));
var gameplay = coflow.LoadModule(new CoflowSource("data/gameplay.cfd", gameplaySource));

var unbound = coflow.Compile();
if (!unbound.Success)
    throw new InvalidOperationException(string.Join(Environment.NewLine, unbound.Diagnostics));
var unboundFirst = coflow.Table(Item.Table).Get("first").Value;
try
{
    _ = unboundFirst.Calculate(coflow, 3);
    throw new InvalidOperationException("An unbound Host call unexpectedly succeeded.");
}
catch (CoflowFaultException error) when (error.InnerException is CoflowFunctionNotBoundException)
{
}

long notified = 0;
var services = new Services("test", static value => value + 10, value => notified = value);
coflow.Bind(services);
var result = coflow.Compile();
if (!result.Success)
    throw new InvalidOperationException(string.Join(Environment.NewLine, result.Diagnostics));
var publishedServices = coflow.Singleton<Services>().Value;
if (ReferenceEquals(publishedServices, services))
    throw new InvalidOperationException("The published snapshot reused the application-owned Host object.");
if (publishedServices.Adjust(coflow, 3) != 13)
    throw new InvalidOperationException("A published Host did not invoke its native implementation directly.");
publishedServices.Notify(coflow, 7);
if (notified != 7)
    throw new InvalidOperationException("A published void Host function did not invoke its native implementation directly.");
try
{
    _ = services.Adjust(coflow, 3);
    throw new InvalidOperationException("An application-owned Host binding unexpectedly became callable.");
}
catch (CoflowStaleValueException)
{
}

var first = coflow.Table(Item.Table).Get("first").Value;
if (first.Calculate(coflow, 3) != 19)
    throw new InvalidOperationException("Cross-Module function invocation returned the wrong value.");
if (first.Stats.Transform(coflow, 3) != 5)
    throw new InvalidOperationException("Struct default function was not linked.");
var second = coflow.Table(Item.Table).Get("second").Value;
var self = coflow.Table(Item.Table).Get("self").Value;
if (!ReferenceEquals(first.Next.Value, second) || !ReferenceEquals(second.Next.Value, first))
    throw new InvalidOperationException("Cross-Module record reference was not linked.");
if (!ReferenceEquals(self.Next.Value, self))
    throw new InvalidOperationException("A self-referencing record was not linked.");

coflow.ReplaceModule(gameplay, new CoflowSource("data/gameplay.cfd", "Item { second { stats: Stats { value: 6 } } }"));
var failed = coflow.Compile();
if (failed.Success || first.Calculate(coflow, 3) != 19 || publishedServices.Adjust(coflow, 3) != 13 ||
    !ReferenceEquals(coflow.Singleton<Services>().Value, publishedServices))
    throw new InvalidOperationException("A failed compile replaced the published snapshot.");

var parallel = Schema.Create();
parallel.LoadModule(new CoflowSource("data/core.cfd", coreSource));
parallel.LoadModule(new CoflowSource("data/gameplay.cfd", gameplaySource));
parallel.Bind(services);
if (!parallel.Compile().Success)
    throw new InvalidOperationException("The second Coflow instance did not compile.");
var parallelServices = parallel.Singleton<Services>().Value;
if (ReferenceEquals(parallelServices, services) || ReferenceEquals(parallelServices, publishedServices))
    throw new InvalidOperationException("Different Coflow instances shared a Host snapshot object.");
if (parallelServices.Adjust(parallel, 3) != 13 || publishedServices.Adjust(coflow, 3) != 13 ||
    parallel.Table(Item.Table).Get("first").Value.Calculate(parallel, 3) != 19 || first.Calculate(coflow, 3) != 19)
    throw new InvalidOperationException("Reusing one Host binding across Coflow instances corrupted a published snapshot.");
try
{
    _ = publishedServices.Adjust(parallel, 3);
    throw new InvalidOperationException("A Host snapshot executed against a different Coflow instance.");
}
catch (CoflowStaleValueException)
{
}

coflow.ReplaceModule(gameplay, new CoflowSource("data/gameplay.cfd", gameplaySource.Replace("value: 4", "value: 6")));
var replaced = coflow.Compile();
if (!replaced.Success)
    throw new InvalidOperationException(string.Join(Environment.NewLine, replaced.Diagnostics));
if (first.Stats.Value != 2)
    throw new InvalidOperationException("Old API values stopped being readable after replacement.");
try
{
    _ = first.Calculate(coflow, 3);
    throw new InvalidOperationException("A stale value unexpectedly executed against a new snapshot.");
}
catch (CoflowStaleValueException)
{
}

var current = coflow.Table(Item.Table).Get("first").Value;
if (current.Calculate(coflow, 3) != 21)
    throw new InvalidOperationException("Module replacement was not globally relinked.");
var currentSecond = coflow.Table(Item.Table).Get("second").Value;
if (!ReferenceEquals(current.Next.Value, currentSecond) || !ReferenceEquals(currentSecond.Next.Value, current))
    throw new InvalidOperationException("Module replacement did not rebuild the cross-Module reference cycle.");

coflow.RemoveModule(gameplay);
var removed = coflow.Compile();
if (removed.Success)
    throw new InvalidOperationException("Removing a referenced Module unexpectedly compiled.");

var budgeted = Schema.Create(new CoflowOptions(maxHostCalls: 1));
budgeted.LoadModule(new CoflowSource("data/core.cfd", coreSource));
budgeted.LoadModule(new CoflowSource("data/gameplay.cfd", gameplaySource));
var reentering = false;
budgeted.Bind(new Services("budget", value =>
{
    if (reentering) return value;
    reentering = true;
    try { return budgeted.Table(Item.Table).Get("first").Value.Calculate(budgeted, value); }
    finally { reentering = false; }
}, static _ => { }));
if (!budgeted.Compile().Success)
    throw new InvalidOperationException("The budget reentrancy project did not compile.");
try
{
    _ = budgeted.Singleton<Services>().Value.Adjust(budgeted, 1);
    throw new InvalidOperationException("Host reentrancy did not share the top-level execution budget.");
}
catch (CoflowFaultException error) when (
    error.InnerException is CoflowExecutionLimitException { Limit: nameof(CoflowOptions.MaxHostCalls) })
{
}

var throwing = Schema.Create();
throwing.LoadModule(new CoflowSource("data/core.cfd", coreSource));
throwing.LoadModule(new CoflowSource("data/gameplay.cfd", gameplaySource));
throwing.Bind(new Services("throwing", static _ => throw new InvalidOperationException("host-failure"), static _ => { }));
if (!throwing.Compile().Success)
    throw new InvalidOperationException("The throwing Host project did not compile.");
try
{
    _ = throwing.Singleton<Services>().Value.Adjust(throwing, 1);
    throw new InvalidOperationException("A direct Host exception was not propagated as a Coflow fault.");
}
catch (CoflowFaultException error) when (error.InnerException is InvalidOperationException { Message: "host-failure" })
{
}

Console.WriteLine("csharp-runtime-redesign-ok");
