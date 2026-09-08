using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

internal sealed class CoflowExecutionBudget
{
    private global::Coflow.Runtime.CoflowOptions _limits;
    private long _instructions;
    private int _invocationValues;
    private int _collectionElements;
    private long _collectionWork;
    private int _closureLanes;
    private int _hostCalls;
    private long _boundaryLanes;
    private int _frameDepth;
    private int _integerRegisters;
    private int _floatRegisters;
    private int _referenceRegisters;

    internal CoflowExecutionBudget(global::Coflow.Runtime.CoflowOptions limits) => _limits = limits;

    internal void Reset(global::Coflow.Runtime.CoflowOptions limits)
    {
        _limits = limits;
        _instructions = 0;
        _invocationValues = 0;
        _collectionElements = 0;
        _collectionWork = 0;
        _closureLanes = 0;
        _hostCalls = 0;
        _boundaryLanes = 0;
        _frameDepth = 0;
        _integerRegisters = 0;
        _floatRegisters = 0;
        _referenceRegisters = 0;
    }

    internal static CoflowExecutionBudget CreateUnbounded() => new(new global::Coflow.Runtime.CoflowOptions(
        long.MaxValue, int.MaxValue, int.MaxValue, int.MaxValue, int.MaxValue,
        int.MaxValue, int.MaxValue, long.MaxValue, int.MaxValue, int.MaxValue, int.MaxValue,
        int.MaxValue, int.MaxValue));

    internal void Instruction() => Add(ref _instructions, 1, _limits.MaxInstructions, nameof(_limits.MaxInstructions));
    internal void InvocationValue() => Add(ref _invocationValues, 1, _limits.MaxInvocationValues, nameof(_limits.MaxInvocationValues));
    internal void CollectionElements(int count) => Add(ref _collectionElements, count, _limits.MaxCollectionElements, nameof(_limits.MaxCollectionElements));
    internal void CollectionWork(long count) => Add(ref _collectionWork, count, _limits.MaxCollectionWork, nameof(_limits.MaxCollectionWork));
    internal void ClosureLanes(int count) => Add(ref _closureLanes, count, _limits.MaxClosureLanes, nameof(_limits.MaxClosureLanes));
    internal void HostCall(long boundaryLanes)
    {
        Add(ref _hostCalls, 1, _limits.MaxHostCalls, nameof(_limits.MaxHostCalls));
        Add(ref _boundaryLanes, boundaryLanes, _limits.MaxBoundaryLanes, nameof(_limits.MaxBoundaryLanes));
    }

    internal void AcquireRegisters(int integers, int floats, int references)
    {
        RequireAddition(_integerRegisters, integers, _limits.MaxIntegerRegisters, nameof(_limits.MaxIntegerRegisters));
        RequireAddition(_floatRegisters, floats, _limits.MaxFloatRegisters, nameof(_limits.MaxFloatRegisters));
        RequireAddition(_referenceRegisters, references, _limits.MaxReferenceRegisters, nameof(_limits.MaxReferenceRegisters));
        _integerRegisters += integers;
        _floatRegisters += floats;
        _referenceRegisters += references;
    }

    internal void ReleaseRegisters(int integers, int floats, int references)
    {
        _integerRegisters -= integers;
        _floatRegisters -= floats;
        _referenceRegisters -= references;
        if (_integerRegisters < 0 || _floatRegisters < 0 || _referenceRegisters < 0)
            throw new InvalidOperationException("Coflow register budget is unbalanced.");
    }

    internal void EnterFrame()
    {
        RequireAddition(_frameDepth, 1, _limits.MaxFrameDepth, nameof(_limits.MaxFrameDepth));
        _frameDepth++;
    }

    internal void ExitFrame()
    {
        if (_frameDepth <= 0) throw new InvalidOperationException("Coflow execution frame budget is unbalanced.");
        _frameDepth--;
    }

    private static void Add(ref int current, int count, int limit, string name)
    {
        if (count < 0 || current > limit - count) throw new global::Coflow.Runtime.CoflowExecutionLimitException(name);
        current += count;
    }

    private static void Add(ref long current, long count, long limit, string name)
    {
        if (count < 0 || current > limit - count) throw new global::Coflow.Runtime.CoflowExecutionLimitException(name);
        current += count;
    }

    private static void RequireAddition(int current, int addition, int limit, string name)
    {
        if (addition < 0 || current > limit - addition)
            throw new global::Coflow.Runtime.CoflowExecutionLimitException(name);
    }
}
}
