using CoflowRuntime;
using System.Reflection;
using System.Runtime.Versioning;

var runtimeAssembly = typeof(Option<>).Assembly;
var framework = runtimeAssembly.GetCustomAttribute<TargetFrameworkAttribute>()?.FrameworkName;
if (framework != ".NETStandard,Version=v2.1")
    throw new InvalidOperationException($"Expected the netstandard2.1 runtime assembly, found `{framework}`.");

var option = Option<long>.Some(42);
var result = Result<Option<long>, string>.Ok(option);
if (!result.IsOk || !result.Value.HasValue || result.Value.Value != 42)
    throw new InvalidOperationException("The netstandard2.1 Option/Result boundary failed.");

Console.WriteLine("csharp-runtime-netstandard21-smoke-ok");
