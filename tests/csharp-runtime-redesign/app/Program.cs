using Coflow.Runtime;

var project = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../../../"));
var coreSource = File.ReadAllText(Path.Combine(project, "data/core.cfd"));
var gameplaySource = File.ReadAllText(Path.Combine(project, "data/gameplay.cfd"));
var coflow = Schema.Create();
var core = coflow.LoadModule(new CoflowSource("data/core.cfd", coreSource));
coflow.LoadModule(new CoflowSource("arbitrary-name.cfd",
    File.ReadAllText(Path.Combine(project, "data/dimensions/language/Item_title.cfd"))));
var gameplay = coflow.LoadModule(new CoflowSource("data/gameplay.cfd", gameplaySource));

var unbound = coflow.Compile();
if (!unbound.Success)
    throw new InvalidOperationException(string.Join(Environment.NewLine, unbound.Diagnostics));
var unboundFirst = coflow.Table(Item.Table).Get("first").Value;
try
{
    _ = unboundFirst.calculate(coflow, 3);
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
if (publishedServices.adjust(coflow, 3) != 13)
    throw new InvalidOperationException("A published Host did not invoke its native implementation directly.");
publishedServices.notify(coflow, 7);
if (notified != 7)
    throw new InvalidOperationException("A published void Host function did not invoke its native implementation directly.");
try
{
    _ = services.adjust(coflow, 3);
    throw new InvalidOperationException("An application-owned Host binding unexpectedly became callable.");
}
catch (CoflowStaleValueException)
{
}

var first = coflow.Table(Item.Table).Get("first").Value;
if (first.calculate(coflow, 3) != 19)
    throw new InvalidOperationException("Cross-Module function invocation returned the wrong value.");
if (first.stats.transform(coflow, 3) != 5)
    throw new InvalidOperationException("Struct default function was not linked.");
var second = coflow.Table(Item.Table).Get("second").Value;
var self = coflow.Table(Item.Table).Get("self").Value;
if (!ReferenceEquals(first.next.Value, second) || !ReferenceEquals(second.next.Value, first))
    throw new InvalidOperationException("Cross-Module record reference was not linked.");
if (!ReferenceEquals(self.next.Value, self))
    throw new InvalidOperationException("A self-referencing record was not linked.");

coflow.ReplaceModule(gameplay, new CoflowSource("data/gameplay.cfd", "Item { second { stats: Stats { value: 6 } } }"));
var failed = coflow.Compile();
if (failed.Success || first.calculate(coflow, 3) != 19 || publishedServices.adjust(coflow, 3) != 13 ||
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
if (parallelServices.adjust(parallel, 3) != 13 || publishedServices.adjust(coflow, 3) != 13 ||
    parallel.Table(Item.Table).Get("first").Value.calculate(parallel, 3) != 19 || first.calculate(coflow, 3) != 19)
    throw new InvalidOperationException("Reusing one Host binding across Coflow instances corrupted a published snapshot.");
try
{
    _ = publishedServices.adjust(parallel, 3);
    throw new InvalidOperationException("A Host snapshot executed against a different Coflow instance.");
}
catch (CoflowStaleValueException)
{
}

coflow.ReplaceModule(gameplay, new CoflowSource("data/gameplay.cfd", gameplaySource.Replace("value: 4", "value: 6")));
var replaced = coflow.Compile();
if (!replaced.Success)
    throw new InvalidOperationException(string.Join(Environment.NewLine, replaced.Diagnostics));
if (first.stats.value != 2)
    throw new InvalidOperationException("Old API values stopped being readable after replacement.");
try
{
    _ = first.calculate(coflow, 3);
    throw new InvalidOperationException("A stale value unexpectedly executed against a new snapshot.");
}
catch (CoflowStaleValueException)
{
}

var current = coflow.Table(Item.Table).Get("first").Value;
if (current.calculate(coflow, 3) != 21)
    throw new InvalidOperationException("Module replacement was not globally relinked.");
var currentSecond = coflow.Table(Item.Table).Get("second").Value;
if (!ReferenceEquals(current.next.Value, currentSecond) || !ReferenceEquals(currentSecond.next.Value, current))
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
    try { return budgeted.Table(Item.Table).Get("first").Value.calculate(budgeted, value); }
    finally { reentering = false; }
}, static _ => { }));
if (!budgeted.Compile().Success)
    throw new InvalidOperationException("The budget reentrancy project did not compile.");
try
{
    _ = budgeted.Singleton<Services>().Value.adjust(budgeted, 1);
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
    _ = throwing.Singleton<Services>().Value.adjust(throwing, 1);
    throw new InvalidOperationException("A direct Host exception was not propagated as a Coflow fault.");
}
catch (CoflowFaultException error) when (error.InnerException is InvalidOperationException { Message: "host-failure" })
{
}

var dimensionFlow = Schema.Create();
dimensionFlow.LoadModule(new CoflowSource("dimensions.cfd", """
    text: UiText { welcome: "Hello", count: 17 }
    welcome: __coflow_language_UiText_welcome { zh: "Ni hao" }
    weights: __coflow_language_UiText_weights { zh: [3, 4, 5] }
    theme: __coflow_language_UiText_theme { zh: ThemeValue { value: 9 } }
    """));
dimensionFlow.LoadModule(new CoflowSource("inherited.cfd", """
    child: DimensionChild { name: "Base name", hint: "Base hint" }
    child: __coflow_language_DimensionBase_name { zh: "Translated name" }
    child: __coflow_platform_DimensionBase_hint { mobile: "Tap", desktop: None }
    """));
var dimensionResult = dimensionFlow.Compile();
if (!dimensionResult.Success)
    throw new InvalidOperationException(string.Join(Environment.NewLine, dimensionResult.Diagnostics));
var text = dimensionFlow.Singleton<UiText>().Value;
var child = dimensionFlow.Table(DimensionChild.Table).Get("child").Value;
if (child.name.For("zh") != "Translated name" || child.hint.For("mobile") != "Tap" ||
    child.hint.For("desktop") != "Base hint" ||
    dimensionFlow.Table(DimensionBase.Table).Count != 1)
    throw new InvalidOperationException("Inherited fields or independent dimensions were not loaded correctly.");
if (text.welcome.Default != "Hello" || text.welcome.For("zh") != "Ni hao" ||
    text.welcome.For("en") != "Hello" || text.count != 17 || text.readCount(dimensionFlow) != 17 ||
    !text.weights.For("zh").SequenceEqual(new long[] { 3, 4, 5 }) ||
    !text.weights.For("en").SequenceEqual(new long[] { 1, 2 }) ||
    text.theme.For("zh").value != 9 || text.theme.For("en").value != 5)
    throw new InvalidOperationException("Dimension layout or fallback value is incorrect.");
if (coflow.Table(Item.Table).Get("first").Value.title.Default != "Item")
    throw new InvalidOperationException("The default-only dimension wrapper was not preserved.");
if (coflow.Table(Item.Table).Get("first").Value.title.For("zh") != "First translated" ||
    coflow.Table(Item.Table).Get("first").Value.title.For("en") != "Item")
    throw new InvalidOperationException("The dimension file did not bind to the ordinary record field.");
if (!text.sameTheme(dimensionFlow, text.theme.Default, new ThemeValue(5)) ||
    text.sameTheme(dimensionFlow, text.theme.Default, text.theme.For("zh")))
    throw new InvalidOperationException("Struct equality did not compare the field before its identity lane.");

// 无主体记录或错误 singleton 字段不能作为未使用的辅助数据被静默忽略。
foreach (var invalid in new[] {
    "missing: __coflow_language_Item_title { zh: \"orphan\" }",
    "unknown: __coflow_language_UiText_welcome { zh: \"wrong field\" }",
    "text: UiText { welcome: \"Hello\", count: 17 } welcome: __coflow_language_UiText_welcome { invalid: \"bad variant\" }",
    "text: UiText { welcome: \"Hello\", count: 17 } weights: __coflow_language_UiText_weights { zh: \"bad value\" }",
    "text: UiText { welcome: \"Hello\", count: 17 } welcome: UiText_welcomeVariants { zh: \"old format\" }",
})
{
    var invalidFlow = Schema.Create();
    invalidFlow.LoadModule(new CoflowSource("invalid.cfd", invalid));
    if (invalidFlow.Compile().Success)
        throw new InvalidOperationException("An invalid dimension target compiled: " + invalid);
}

Console.WriteLine("csharp-runtime-redesign-ok");
