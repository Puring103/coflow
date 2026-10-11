using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System;
using System.Collections.Generic;

namespace Coflow.Runtime.CompilerServices
{

internal sealed class CoflowClosureTemplate
{
    private int _targetIndex = -1;
    internal CoflowProgram Program { get; }

    internal CoflowCaptureLayout[] Captures { get; }

    internal int CaptureCount => Captures.Length;
    internal int TargetIndex => _targetIndex >= 0 ? _targetIndex
        : throw new InvalidOperationException("The closure has not been assigned a snapshot target index.");

    internal void AssignTargetIndex(int targetIndex)
    {
        if (_targetIndex >= 0) throw new InvalidOperationException("The closure target is already linked.");
        _targetIndex = targetIndex;
    }

    internal int IntegerCount { get; private set; }

    internal int FloatCount { get; private set; }

    internal int ReferenceCount { get; private set; }

    internal CoflowClosureTemplate(CoflowProgram program, IReadOnlyList<Type> captureTypes)
    {
        Program = program;
        Captures = new CoflowCaptureLayout[captureTypes.Count];
        for (int i = 0; i < captureTypes.Count; i++)
        {
            CoflowValueShape coflowValueShape = CoflowValueShape.Of(captureTypes[i]);
            Captures[i] = new CoflowCaptureLayout(coflowValueShape, IntegerCount, FloatCount, ReferenceCount);
            IntegerCount += coflowValueShape.IntegerCount;
            FloatCount += coflowValueShape.FloatCount;
            ReferenceCount += coflowValueShape.ReferenceCount;
        }
    }
}

internal sealed class CoflowClosureProgramTemplate
{
    private readonly IReadOnlyList<Type> _captureTypes;

    internal CoflowClosureProgramTemplate(
        CoflowProgramTemplate program,
        IReadOnlyList<Type> captureTypes)
    {
        Program = program;
        _captureTypes = captureTypes.ToArray();
    }

    internal CoflowProgramTemplate Program { get; }

    internal CoflowClosureTemplate Link(CoflowProgramLinker linker) =>
        linker.RegisterClosure(new CoflowClosureTemplate(Program.Link(linker), _captureTypes));
}
}
