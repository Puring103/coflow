using Coflow.Runtime.CompilerServices;
using System;
using System.Collections.Generic;
using Xunit;

namespace Coflow.Runtime.Tests;

public sealed class CoflowLayoutRegistryTests
{
    [Fact]
    public void CandidateClosedLayoutsDoNotEscapeTheirCompilationScope()
    {
        var type = typeof(IReadOnlyList<IReadOnlyList<long>>);
        var candidate = new CoflowLayoutRegistry();

        using (CoflowLayoutCompilation.Enter(candidate))
        {
            var layout = CoflowValueShape.Of(type);
            Assert.Equal(CoflowValueShapeKind.Collection, layout.Kind);
            Assert.True(candidate.TryGet(type, out var registered));
            Assert.Same(layout, registered);
        }

        Assert.Throws<InvalidOperationException>(() => CoflowValueShape.Of(type));
    }

    [Fact]
    public void EachCandidateRebuildsClosedLayoutsIndependently()
    {
        var type = typeof(IReadOnlyDictionary<string, IReadOnlyList<IReadOnlyList<long>>>);
        var first = new CoflowLayoutRegistry();
        var second = new CoflowLayoutRegistry();
        CoflowValueShape firstLayout;

        using (CoflowLayoutCompilation.Enter(first))
            firstLayout = CoflowValueShape.Of(type);
        using (CoflowLayoutCompilation.Enter(second))
        {
            var secondLayout = CoflowValueShape.Of(type);
            Assert.NotSame(firstLayout, secondLayout);
            Assert.True(second.TryGet(type, out var registered));
            Assert.Same(secondLayout, registered);
        }

        Assert.Throws<InvalidOperationException>(() => CoflowValueShape.Of(type));
    }
}
