using System.Collections.Generic;
using Coflow.Loader;
using Xunit;

namespace Coflow.Loader.Tests;

public sealed class IdentityTests
{
    private class Node { internal string Id=""; internal Option<Node> Next; internal string Label=""; }
    private sealed class Child : Node { }
    private sealed class Other { internal string Id=""; }
    private static T Fill<T>(T value,CfdReader reader) where T:Node { value.Next=reader.Get<Option<Node>>("next");value.Label=reader.Get<string>("label");return value; }

    private sealed class Graph
    {
        internal Graph(CfdData data)
        {
            Nodes = data.Base.Table<Node>();
            Others = data.Base.Table<Other>();
        }
        internal IReadOnlyDictionary<string, Node> Nodes { get; }
        internal IReadOnlyDictionary<string, Other> Others { get; }
    }

    private static readonly CfdField[] Fields =
    {
        new CfdField<Option<Node>>("next", CfdValues.Optional(CfdValues.Reference<Node>("Node")), CfdDefaults.None()),
        new CfdField<string>("label", CfdValues.String, CfdDefaults.Template("f\"{self.id}\"")),
    };

    private static readonly CfdType[] Types =
    {
        new CfdType<Node>("Node", CfdTypeKind.Table, Fields, id => new Node { Id=id }, (value,reader)=>Fill(value,reader)),
        new CfdType<Child>("Child", CfdTypeKind.Table, Fields, id => new Child { Id=id }, (value,reader)=>Fill(value,reader), parent: "Node"),
        new CfdType<Other>("Other", CfdTypeKind.Table, System.Array.Empty<CfdField>(), id => new Other { Id=id }, (value,reader)=>value),
    };

    private static LoadResult<Graph> Load(string source) => CfdLoader.Load(
        new[] { new CfdSource("identity.cfd", source) }, Types, data => new Graph(data));

    [Fact]
    public void BaseTypedReferencesKeepDerivedIdentityAndCycles()
    {
        var result = Load("base: Node { next: &Node::child } child: Child { next: &Node::base }");
        Assert.True(result.Success);
        var parent = result.Data.Nodes["base"];
        var child = Assert.IsType<Child>(result.Data.Nodes["child"]);
        Assert.Same(child, parent.Next.Value);
        Assert.Same(parent, child.Next.Value);
    }

    [Fact]
    public void UnrelatedTypesCanShareIdsWithoutAmbiguousTypedLookups()
    {
        var result = Load("same: Child { next: &Node::same, label: f\"{&Child::same.id}\" } same: Other {}");
        Assert.True(result.Success);
        var child = result.Data.Nodes["same"];
        Assert.Same(child, child.Next.Value);
        Assert.Equal("same", child.Label);
        Assert.Equal("same", result.Data.Others["same"].Id);
    }

    [Theory]
    [InlineData("Host", true)]
    [InlineData("Result", true)]
    [InlineData("Ok", true)]
    [InlineData("Err", true)]
    [InlineData("fstring", true)]
    public void TypeIdentifiersFollowTheSharedLanguageReservations(string name, bool success)
    {
        var type = new CfdType<Other>(name, CfdTypeKind.Table, System.Array.Empty<CfdField>(), id => new Other { Id=id }, (value,reader)=>value);
        var result = CfdLoader.Load(new[] { new CfdSource("names.cfd", "a: " + name + " {}") },
            new CfdType[] { type }, data => new System.Tuple<Table<Other>>(data.Base.Table<Other>()));
        Assert.Equal(success, result.Success);
        if (success) Assert.Equal("a", result.Data.Item1["a"].Id);
        else Assert.Equal("names.cfd", result.Diagnostics[0].Path);
    }

    [Theory]
    [InlineData("same: Node {} same: Child {}", "CFD-DUPLICATE")]
    [InlineData("Other { child: Child {} }", "CFD-TYPE")]
    [InlineData("Missing { child: Child {} }", "CFD-TYPE")]
    [InlineData("base: Node {} child: Child { next: &Child::base }", "CFD-REFERENCE")]
    public void DuplicateInheritedIdentitiesAndWrongDerivedReferencesFail(string source, string code)
    {
        var result = Load(source);
        Assert.False(result.Success);
        Assert.Equal(code, result.Diagnostics[0].Code);
        Assert.Equal("identity.cfd", result.Diagnostics[0].Path);
    }
}
