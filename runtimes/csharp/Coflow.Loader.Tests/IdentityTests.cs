using System.Collections.Generic;
using Coflow.Loader;
using Xunit;

namespace Coflow.Loader.Tests;

public sealed class IdentityTests
{
    private class Node
    {
        internal Node(string key) { Key = key; }
        internal string Key { get; }
        internal Node? Next { get; set; }
        internal string Label { get; set; } = null!;
    }

    private sealed class Child : Node
    {
        internal Child(string key) : base(key) { }
    }

    private sealed class Other
    {
        internal Other(string key) { Key = key; }
        internal string Key { get; }
    }

    private sealed class Graph
    {
        internal Graph(CfdData data)
        {
            Nodes = data.Records<Node>();
            Others = data.Records<Other>();
        }
        internal IReadOnlyDictionary<string, Node> Nodes { get; }
        internal IReadOnlyDictionary<string, Other> Others { get; }
    }

    private static readonly CfdField[] Fields =
    {
        new CfdField<Node?>("next", CfdValues.OptionalReference(CfdValues.Reference<Node>("Node"))),
        new CfdField<string>("label", CfdValues.FormattedString, CfdDefaults.Template("f\"{self.id}\"")),
    };

    private static void Fill(Node node, CfdReader reader)
    {
        node.Next = reader.Get<Node?>("next");
        node.Label = reader.Get<string>("label");
    }

    private static readonly CfdType[] Types =
    {
        new CfdType<Node>("Node", Fields, key => new Node(key), Fill),
        new CfdType<Child>("Child", Fields, key => new Child(key), (node, reader) => Fill(node, reader), parent: "Node"),
        new CfdType<Other>("Other", System.Array.Empty<CfdField>(), key => new Other(key), (_, _) => { }),
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
        Assert.Same(child, parent.Next);
        Assert.Same(parent, child.Next);
    }

    [Fact]
    public void UnrelatedTypesCanShareKeysWithoutAmbiguousTypedLookups()
    {
        var result = Load("same: Child { next: &Node::same, label: f\"{&Child::same.id}\" } same: Other {}");
        Assert.True(result.Success);
        var child = result.Data.Nodes["same"];
        Assert.Same(child, child.Next);
        Assert.Equal("same", child.Label);
        Assert.Equal("same", result.Data.Others["same"].Key);
    }

    [Theory]
    [InlineData("Host", true)]
    [InlineData("fstring", false)]
    public void TypeIdentifiersFollowTheSharedLanguageReservations(string name, bool success)
    {
        var type = new CfdType<Other>(name, System.Array.Empty<CfdField>(), key => new Other(key), (_, _) => { });
        var result = CfdLoader.Load(new[] { new CfdSource("names.cfd", "a: " + name + " {}") },
            new CfdType[] { type }, data => new Graph(data));
        Assert.Equal(success, result.Success);
        if (success) Assert.Equal("a", result.Data.Others["a"].Key);
        else Assert.Equal("names.cfd", result.Diagnostics[0].Path);
    }

    [Theory]
    [InlineData("same: Node {} same: Child {}", "CFD-DUPLICATE")]
    [InlineData("base: Node {} child: Child { next: &Child::base }", "CFD-REFERENCE")]
    public void DuplicateInheritedIdentitiesAndWrongDerivedReferencesFail(string source, string code)
    {
        var result = Load(source);
        Assert.False(result.Success);
        Assert.Equal(code, result.Diagnostics[0].Code);
        Assert.Equal("identity.cfd", result.Diagnostics[0].Path);
    }
}
