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
        internal Option<Node> Next { get; set; }
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
            Nodes = data.Table<Node>();
            Others = data.Table<Other>();
        }
        internal IReadOnlyDictionary<string, Node> Nodes { get; }
        internal IReadOnlyDictionary<string, Other> Others { get; }
    }

    private static readonly CfdField[] Fields =
    {
        new CfdField<Option<Node>>("next", CfdValues.Optional(CfdValues.Reference<Node>("Node"))),
        new CfdField<string>("label", CfdValues.FormattedString, CfdDefaults.Template("f\"{self.id}\"")),
    };

    private static void Fill(Node node, CfdReader reader)
    {
        node.Next = reader.Get<Option<Node>>("next");
        node.Label = reader.Get<string>("label");
    }

    private static readonly CfdType[] Types =
    {
        new CfdType<Node>("Node", CfdTypeKind.Table, Fields, key => new Node(key), Fill, construct: null),
        new CfdType<Child>("Child", CfdTypeKind.Table, Fields, key => new Child(key), (node, reader) => Fill(node, reader), construct: null, parent: "Node"),
        new CfdType<Other>("Other", CfdTypeKind.Table, System.Array.Empty<CfdField>(), key => new Other(key), (_, _) => { }, construct: null),
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
    public void UnrelatedTypesCanShareKeysWithoutAmbiguousTypedLookups()
    {
        var result = Load("same: Child { next: &Node::same, label: f\"{&Child::same.id}\" } same: Other {}");
        Assert.True(result.Success);
        var child = result.Data.Nodes["same"];
        Assert.Same(child, child.Next.Value);
        Assert.Equal("same", child.Label);
        Assert.Equal("same", result.Data.Others["same"].Key);
    }

    [Theory]
    [InlineData("Host", true)]
    [InlineData("Result", true)]
    [InlineData("Ok", true)]
    [InlineData("Err", true)]
    [InlineData("fstring", false)]
    public void TypeIdentifiersFollowTheSharedLanguageReservations(string name, bool success)
    {
        var type = new CfdType<Other>(name, CfdTypeKind.Table, System.Array.Empty<CfdField>(), key => new Other(key), (_, _) => { }, construct: null);
        var result = CfdLoader.Load(new[] { new CfdSource("names.cfd", "a: " + name + " {}") },
            new CfdType[] { type }, data => new System.Tuple<Table<Other>>(data.Table<Other>()));
        Assert.Equal(success, result.Success);
        if (success) Assert.Equal("a", result.Data.Item1["a"].Key);
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
