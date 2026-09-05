using CoflowRuntime.Generated;
using System.ComponentModel;
using System.Linq;
using System.Reflection;
using Xunit;

namespace CoflowRuntime.Tests;

public sealed class ApiSurfaceTests
{
    [Fact]
    public void GeneratedAbiTypesAreHiddenFromApplicationIntelliSense()
    {
        var generatedTypes = typeof(CfdValueReader).Assembly.ExportedTypes
            .Where(type => type.Namespace == "CoflowRuntime.Generated")
            .ToArray();

        Assert.NotEmpty(generatedTypes);
        Assert.All(generatedTypes, type =>
        {
            var attribute = type.GetCustomAttribute<EditorBrowsableAttribute>();
            Assert.True(
                attribute?.State == EditorBrowsableState.Never,
                $"Generated ABI type `{type.FullName}` must be hidden from application IntelliSense.");
        });
    }
}
