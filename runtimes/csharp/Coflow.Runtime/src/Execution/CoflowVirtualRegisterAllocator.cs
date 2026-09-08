using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

internal sealed record CoflowRegisterAllocation(
    CoflowValueRegister[] Registers,
    int IntegerCount,
    int FloatCount,
    int ReferenceCount,
    CoflowVirtualValue[][][] ReferenceDeaths);

/// <summary>基于 CFG 活性为三类 lane 分配连续区间；参数区固定，临时值按干涉关系复用。</summary>
internal static class CoflowVirtualRegisterAllocator
{
    internal static CoflowRegisterAllocation Allocate(CoflowVirtualProgram program)
    {
        var valueCount = program.Values.Length;
        var successors = program.Blocks.Select(Successors).ToArray();
        var uses = new HashSet<int>[program.Blocks.Length];
        var definitions = new HashSet<int>[program.Blocks.Length];
        for (var index = 0; index < program.Blocks.Length; index++)
            AnalyzeBlock(program.Blocks[index], out uses[index], out definitions[index]);

        var liveIn = Enumerable.Range(0, program.Blocks.Length).Select(_ => new HashSet<int>()).ToArray();
        var liveOut = Enumerable.Range(0, program.Blocks.Length).Select(_ => new HashSet<int>()).ToArray();
        var changed = true;
        while (changed)
        {
            changed = false;
            for (var index = program.Blocks.Length - 1; index >= 0; index--)
            {
                var outgoing = new HashSet<int>();
                foreach (var successor in successors[index]) outgoing.UnionWith(liveIn[successor]);
                var incoming = new HashSet<int>(outgoing);
                incoming.ExceptWith(definitions[index]);
                incoming.UnionWith(uses[index]);
                if (!outgoing.SetEquals(liveOut[index]))
                {
                    liveOut[index] = outgoing;
                    changed = true;
                }
                if (!incoming.SetEquals(liveIn[index]))
                {
                    liveIn[index] = incoming;
                    changed = true;
                }
            }
        }

        var interference = Enumerable.Range(0, valueCount).Select(_ => new HashSet<int>()).ToArray();
        var referenceDeaths = program.Blocks.Select(block =>
            Enumerable.Range(0, block.Instructions.Count)
                .Select(_ => Array.Empty<CoflowVirtualValue>()).ToArray()).ToArray();
        var used = new HashSet<int>(program.Parameters.Select(value => value.Index));
        foreach (var block in program.Blocks)
        {
            var live = new HashSet<int>(liveOut[block.Index]);
            AddTerminatorInterference(block.Terminator!, live, interference, used);
            for (var index = block.Instructions.Count - 1; index >= 0; index--)
            {
                var instruction = block.Instructions[index];
                var inputIndexes = instruction.Inputs.Select(value => value.Index).ToArray();
                referenceDeaths[block.Index][index] = instruction.Inputs
                    .Where(value => CoflowValueShape.Of(value.Type).ReferenceCount != 0 && !live.Contains(value.Index))
                    .Distinct().ToArray();
                used.UnionWith(inputIndexes);
                if (instruction.Target is { } target)
                {
                    used.Add(target.Index);
                    AddEdges(target.Index, live, interference);
                    AddEdges(target.Index, inputIndexes, interference);
                    live.Remove(target.Index);
                }
                live.UnionWith(inputIndexes);
            }
        }

        var registers = new CoflowValueRegister[valueCount];
        var assigned = new bool[valueCount];
        var integerCount = 0;
        var floatCount = 0;
        var referenceCount = 0;

        // 调用入口会一次写入全部参数，因此参数 lane 保持互不重叠并为整次调用保留。
        foreach (var parameter in program.Parameters)
        {
            var shape = CoflowValueShape.Of(parameter.Type);
            registers[parameter.Index] = new CoflowValueRegister(
                shape, integerCount, floatCount, referenceCount);
            assigned[parameter.Index] = true;
            integerCount += shape.IntegerCount;
            floatCount += shape.FloatCount;
            referenceCount += shape.ReferenceCount;
        }
        var reservedIntegers = integerCount;
        var reservedFloats = floatCount;
        var reservedReferences = referenceCount;

        foreach (var value in program.Values.Where(value => used.Contains(value.Index) && !assigned[value.Index]))
        {
            var shape = CoflowValueShape.Of(value.Type);
            var integerBase = FindBase(value.Index, shape.IntegerCount, reservedIntegers,
                registers, assigned, interference, static register => register.IntegerBase,
                static register => register.Shape.IntegerCount);
            var floatBase = FindBase(value.Index, shape.FloatCount, reservedFloats,
                registers, assigned, interference, static register => register.FloatBase,
                static register => register.Shape.FloatCount);
            var referenceBase = FindBase(value.Index, shape.ReferenceCount, reservedReferences,
                registers, assigned, interference, static register => register.ReferenceBase,
                static register => register.Shape.ReferenceCount);
            registers[value.Index] = new CoflowValueRegister(shape, integerBase, floatBase, referenceBase);
            assigned[value.Index] = true;
            integerCount = Math.Max(integerCount, integerBase + shape.IntegerCount);
            floatCount = Math.Max(floatCount, floatBase + shape.FloatCount);
            referenceCount = Math.Max(referenceCount, referenceBase + shape.ReferenceCount);
        }

        return new CoflowRegisterAllocation(
            registers, integerCount, floatCount, referenceCount, referenceDeaths);
    }

    private static int FindBase(
        int value,
        int width,
        int reserved,
        CoflowValueRegister[] registers,
        bool[] assigned,
        HashSet<int>[] interference,
        Func<CoflowValueRegister, int> start,
        Func<CoflowValueRegister, int> length)
    {
        if (width == 0) return 0;
        for (var candidate = reserved; ; candidate++)
        {
            var available = true;
            foreach (var other in interference[value])
            {
                if (!assigned[other]) continue;
                var otherStart = start(registers[other]);
                var otherLength = length(registers[other]);
                if (candidate < otherStart + otherLength && otherStart < candidate + width)
                {
                    available = false;
                    break;
                }
            }
            if (available) return candidate;
        }
    }

    private static void AnalyzeBlock(
        CoflowBasicBlock block,
        out HashSet<int> uses,
        out HashSet<int> definitions)
    {
        uses = new HashSet<int>();
        definitions = new HashSet<int>();
        foreach (var instruction in block.Instructions)
        {
            foreach (var input in instruction.Inputs)
                if (!definitions.Contains(input.Index)) uses.Add(input.Index);
            if (instruction.Target is { } target) definitions.Add(target.Index);
        }
        foreach (var input in TerminatorInputs(block.Terminator!))
            if (!definitions.Contains(input.Index)) uses.Add(input.Index);
        foreach (var output in TerminatorOutputs(block.Terminator!)) definitions.Add(output.Index);
    }

    private static void AddTerminatorInterference(
        CoflowBlockTerminator terminator,
        HashSet<int> live,
        HashSet<int>[] interference,
        HashSet<int> used)
    {
        var inputs = TerminatorInputs(terminator).Select(value => value.Index).ToArray();
        var outputs = TerminatorOutputs(terminator).Select(value => value.Index).ToArray();
        used.UnionWith(inputs);
        used.UnionWith(outputs);
        foreach (var output in outputs)
        {
            AddEdges(output, live, interference);
            AddEdges(output, inputs, interference);
            AddEdges(output, outputs.Where(other => other != output), interference);
            live.Remove(output);
        }
        live.UnionWith(inputs);
    }

    private static void AddEdges(int value, IEnumerable<int> others, HashSet<int>[] interference)
    {
        foreach (var other in others)
        {
            if (value == other) continue;
            interference[value].Add(other);
            interference[other].Add(value);
        }
    }

    private static int[] Successors(CoflowBasicBlock block) => block.Terminator switch
    {
        CoflowBlockTerminator.Jump jump => new[] { jump.TargetBlock },
        CoflowBlockTerminator.Branch branch => new[] { branch.TrueBlock, branch.FalseBlock },
        CoflowBlockTerminator.Propagate propagate => new[] { propagate.ContinueBlock },
        _ => Array.Empty<int>(),
    };

    private static IEnumerable<CoflowVirtualValue> TerminatorInputs(CoflowBlockTerminator terminator) => terminator switch
    {
        CoflowBlockTerminator.Branch branch => new[] { branch.Condition },
        CoflowBlockTerminator.Return returned => new[] { returned.Value },
        CoflowBlockTerminator.DirectTailCall tail => tail.Inputs,
        CoflowBlockTerminator.IndirectTailCall tail => tail.Inputs,
        CoflowBlockTerminator.Propagate propagate => new[] { propagate.Source },
        _ => Array.Empty<CoflowVirtualValue>(),
    };

    private static IEnumerable<CoflowVirtualValue> TerminatorOutputs(CoflowBlockTerminator terminator) => terminator switch
    {
        CoflowBlockTerminator.DirectTailCall tail => new[] { tail.Result },
        CoflowBlockTerminator.IndirectTailCall tail => new[] { tail.Result },
        CoflowBlockTerminator.Propagate propagate => new[] { propagate.Payload, propagate.ReturnValue },
        _ => Array.Empty<CoflowVirtualValue>(),
    };
}
}
