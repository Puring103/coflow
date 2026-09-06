using Coflow.Runtime;
using Coflow.Runtime.CompilerServices;
using System;
using Xunit;

namespace Coflow.Runtime.Tests;

public sealed class CoflowExecutionBudgetTests
{
    [Fact]
    public void OptionsRejectEveryNonPositiveLimit()
    {
        Assert.Throws<ArgumentOutOfRangeException>(() => new CoflowOptions(maxInstructions: 0));
        Assert.Throws<ArgumentOutOfRangeException>(() => new CoflowOptions(maxFrameDepth: 0));
        Assert.Throws<ArgumentOutOfRangeException>(() => new CoflowOptions(maxIntegerRegisters: 0));
        Assert.Throws<ArgumentOutOfRangeException>(() => new CoflowOptions(maxFloatRegisters: 0));
        Assert.Throws<ArgumentOutOfRangeException>(() => new CoflowOptions(maxReferenceRegisters: 0));
        Assert.Throws<ArgumentOutOfRangeException>(() => new CoflowOptions(maxInvocationValues: 0));
        Assert.Throws<ArgumentOutOfRangeException>(() => new CoflowOptions(maxCollectionElements: 0));
        Assert.Throws<ArgumentOutOfRangeException>(() => new CoflowOptions(maxCollectionWork: 0));
        Assert.Throws<ArgumentOutOfRangeException>(() => new CoflowOptions(maxClosureLanes: 0));
        Assert.Throws<ArgumentOutOfRangeException>(() => new CoflowOptions(maxHostCalls: 0));
        Assert.Throws<ArgumentOutOfRangeException>(() => new CoflowOptions(maxBoundaryLanes: 0));
        Assert.Throws<ArgumentOutOfRangeException>(() => new CoflowOptions(maxEscapedValues: 0));
        Assert.Throws<ArgumentOutOfRangeException>(() => new CoflowOptions(maxEscapedLanes: 0));
    }

    [Fact]
    public void BudgetAccountsForEveryInvocationResource()
    {
        var budget = new CoflowExecutionBudget(new CoflowOptions(
            maxInstructions: 1, maxFrameDepth: 1,
            maxIntegerRegisters: 1, maxFloatRegisters: 1, maxReferenceRegisters: 1,
            maxInvocationValues: 1, maxCollectionElements: 1, maxCollectionWork: 1,
            maxClosureLanes: 1, maxHostCalls: 1, maxBoundaryLanes: 1));

        Exceeds(nameof(CoflowOptions.MaxInstructions), budget.Instruction, budget.Instruction);
        Exceeds(nameof(CoflowOptions.MaxInvocationValues), budget.InvocationValue, budget.InvocationValue);
        Exceeds(nameof(CoflowOptions.MaxCollectionElements),
            () => budget.CollectionElements(1), () => budget.CollectionElements(1));
        Exceeds(nameof(CoflowOptions.MaxCollectionWork),
            () => budget.CollectionWork(1), () => budget.CollectionWork(1));
        Exceeds(nameof(CoflowOptions.MaxClosureLanes),
            () => budget.ClosureLanes(1), () => budget.ClosureLanes(1));
        Exceeds(nameof(CoflowOptions.MaxHostCalls), () => budget.HostCall(0), () => budget.HostCall(0));

        var boundary = new CoflowExecutionBudget(new CoflowOptions(maxBoundaryLanes: 1));
        Exceeds(nameof(CoflowOptions.MaxBoundaryLanes), () => boundary.HostCall(1), () => boundary.HostCall(1));

        budget.EnterFrame();
        Limit(nameof(CoflowOptions.MaxFrameDepth), budget.EnterFrame);
        budget.ExitFrame();
        budget.AcquireRegisters(1, 1, 1);
        Limit(nameof(CoflowOptions.MaxIntegerRegisters), () => budget.AcquireRegisters(1, 0, 0));
        Limit(nameof(CoflowOptions.MaxFloatRegisters), () => budget.AcquireRegisters(0, 1, 0));
        Limit(nameof(CoflowOptions.MaxReferenceRegisters), () => budget.AcquireRegisters(0, 0, 1));
        budget.ReleaseRegisters(1, 1, 1);
    }

    private static void Exceeds(string name, Action first, Action second)
    {
        first();
        Limit(name, second);
    }

    private static void Limit(string name, Action action) =>
        Assert.Equal(name, Assert.Throws<CoflowExecutionLimitException>(action).Limit);
}
