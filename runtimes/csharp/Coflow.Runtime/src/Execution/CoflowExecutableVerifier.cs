using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

/// <summary>验证紧凑寄存器程序的结构边界，阻止损坏编码进入 VM。</summary>
internal static class CoflowExecutableVerifier
{
    internal static void Verify(CoflowRegisterProgram program)
    {
        if (program is null) throw new ArgumentNullException(nameof(program));
        var instructions = program.Instructions;
        if (instructions.Length == 0) throw Invalid("program has no instructions");
        if (program.InstructionSpans.Length != instructions.Length)
            throw Invalid("source map length does not match instruction count");
        if (program.IntegerRegisterCount < 0 || program.FloatRegisterCount < 0 ||
            program.ReferenceRegisterCount < 0)
            throw Invalid("register count cannot be negative");

        foreach (var parameter in program.Parameters) VerifyValue(program, parameter, "parameter");

        for (var pc = 0; pc < instructions.Length; pc++)
        {
            var instruction = instructions[pc];
            if (!CoflowRegisterInstructionSpec.IsKnown(instruction.Code))
                throw Invalid($"instruction {pc} has unknown opcode `{instruction.Code}`");
            VerifyDirectOperands(program, pc, instruction);
            VerifyDescriptor(program, pc, instruction);
        }
        var reachable = VerifyControlFlow(instructions);
        VerifyDefiniteAssignment(program, reachable);
    }

    // 字段存储类别在程序构建时确定，VM 不在每次读取时重复判定。
    private static void VerifyFieldKind(CoflowFieldAccess field, CoflowRegisterOpCode code, int pc)
    {
        var host = code is CoflowRegisterOpCode.LoadHostFieldInteger or
            CoflowRegisterOpCode.LoadHostFieldFloat or CoflowRegisterOpCode.LoadHostFieldReference or
            CoflowRegisterOpCode.LoadHostFieldValue;
        if (field.IsHost != host)
            throw Invalid($"instruction {pc} field storage does not match opcode");
    }

    private static void VerifyDirectOperands(
        CoflowRegisterProgram program,
        int pc,
        CoflowRegisterInstruction instruction)
    {
        var operands = new[] { instruction.A, instruction.B, instruction.C };
        for (var index = 0; index < operands.Length; index++)
        {
            var kind = CoflowRegisterInstructionSpec.OperandKind(instruction.Code, index);
            if (kind is not null) VerifyRegister(program, kind.Value, operands[index], $"instruction {pc} operand {(char)('A' + index)}");
        }

        if (instruction.Code is CoflowRegisterOpCode.ConstantInteger or CoflowRegisterOpCode.ConstantFloat)
            VerifyIndex(instruction.B, program.Immediates.Length, $"instruction {pc} immediate");
        if (instruction.Code == CoflowRegisterOpCode.Jump)
            VerifyJump(instruction.A, program.Instructions.Length, pc);
        else if (instruction.Code is CoflowRegisterOpCode.JumpIfFalse or CoflowRegisterOpCode.JumpIfTrue)
            VerifyJump(instruction.B, program.Instructions.Length, pc);
    }

    private static void VerifyDescriptor(
        CoflowRegisterProgram program,
        int pc,
        CoflowRegisterInstruction instruction)
    {
        var operations = program.Operations;
        var kind = CoflowRegisterInstructionSpec.Descriptor(instruction.Code);
        if (kind == CoflowRegisterDescriptorKind.None) return;
        var index = instruction.C;
        switch (kind)
        {
            case CoflowRegisterDescriptorKind.Reference:
                VerifyIndex(index, operations.References.Length, $"instruction {pc} reference descriptor");
                break;
            case CoflowRegisterDescriptorKind.Constant:
                var constant = Item(operations.Constants, index, pc);
                VerifyValue(program, constant.Target, $"instruction {pc} constant target");
                if (constant.Value.Shape.Type != constant.Target.Shape.Type)
                    throw Invalid($"instruction {pc} constant type does not match target shape");
                break;
            case CoflowRegisterDescriptorKind.Transfer:
                var transfer = Item(operations.Transfers, index, pc);
                VerifyValue(program, transfer.Source, $"instruction {pc} transfer source");
                VerifyValue(program, transfer.Target, $"instruction {pc} transfer target");
                VerifyTransferShape(instruction.Code, transfer, pc);
                break;
            case CoflowRegisterDescriptorKind.Field:
                VerifyFieldKind(Item(operations.Fields, index, pc), instruction.Code, pc);
                break;
            case CoflowRegisterDescriptorKind.FieldValue:
                var fieldValue = Item(operations.FieldValues, index, pc);
                VerifyFieldKind(fieldValue.Access, instruction.Code, pc);
                VerifyValue(program, fieldValue.Target, $"instruction {pc} field target");
                break;
            case CoflowRegisterDescriptorKind.NativeCall:
                VerifyNativeCall(program, Item(operations.NativeCalls, index, pc), pc);
                break;
            case CoflowRegisterDescriptorKind.Collection:
                VerifyCollection(program, Item(operations.Collections, index, pc), pc);
                break;
            case CoflowRegisterDescriptorKind.Index:
                var indexed = Item(operations.Indexes, index, pc);
                VerifyValue(program, indexed.Collection, $"instruction {pc} collection");
                VerifyValue(program, indexed.Index, $"instruction {pc} index");
                VerifyValue(program, indexed.Target, $"instruction {pc} result");
                break;
            case CoflowRegisterDescriptorKind.CollectionRead:
                var read = Item(operations.CollectionReads, index, pc);
                VerifyValue(program, read.Collection, $"instruction {pc} collection");
                if (read.Index is { } readIndex) VerifyValue(program, readIndex, $"instruction {pc} index");
                VerifyValue(program, read.Target, $"instruction {pc} result");
                break;
            case CoflowRegisterDescriptorKind.Projection:
                var projection = Item(operations.Projections, index, pc);
                VerifyValue(program, projection.Source, $"instruction {pc} projection source");
                VerifyValue(program, projection.Target, $"instruction {pc} projection target");
                break;
            case CoflowRegisterDescriptorKind.CollectionBuiltin:
                var builtin = Item(operations.CollectionBuiltins, index, pc);
                VerifyValue(program, builtin.Receiver, $"instruction {pc} builtin receiver");
                if (builtin.Argument is { } argument) VerifyValue(program, argument, $"instruction {pc} builtin argument");
                VerifyValue(program, builtin.Target, $"instruction {pc} builtin target");
                break;
            case CoflowRegisterDescriptorKind.ArrayBuilder:
                var builder = Item(operations.ArrayBuilders, index, pc);
                VerifyValue(program, builder.CollectionOrCapacity, $"instruction {pc} builder input");
                if (builder.Item is { } item) VerifyValue(program, item, $"instruction {pc} builder item");
                VerifyValue(program, builder.Target, $"instruction {pc} builder target");
                break;
            case CoflowRegisterDescriptorKind.Target:
                VerifyValue(program, Item(operations.Targets, index, pc).Target, $"instruction {pc} target");
                break;
            case CoflowRegisterDescriptorKind.Propagate:
                var propagate = Item(operations.Propagates, index, pc);
                VerifyValue(program, propagate.Source, $"instruction {pc} propagate source");
                VerifyValue(program, propagate.Payload, $"instruction {pc} propagate payload");
                VerifyValue(program, propagate.ReturnValue, $"instruction {pc} propagate return");
                break;
            case CoflowRegisterDescriptorKind.Closure:
                var closure = Item(operations.Closures, index, pc);
                if (closure.Captures.Length != closure.Template.CaptureCount)
                    throw Invalid($"instruction {pc} closure capture count does not match template");
                foreach (var capture in closure.Captures) VerifyValue(program, capture, $"instruction {pc} closure capture");
                VerifyValue(program, closure.Target, $"instruction {pc} closure target");
                break;
            case CoflowRegisterDescriptorKind.Type:
                var type = Item(operations.Types, index, pc);
                if (type is null) throw Invalid($"instruction {pc} has a null type descriptor");
                break;
            case CoflowRegisterDescriptorKind.Call:
                VerifyCall(program, Item(operations.Calls, index, pc), pc);
                break;
            case CoflowRegisterDescriptorKind.IndirectCall:
                var indirect = Item(operations.IndirectCalls, index, pc);
                VerifyValue(program, indirect.Callable, $"instruction {pc} callable");
                foreach (var callArgument in indirect.Arguments) VerifyValue(program, callArgument, $"instruction {pc} argument");
                VerifyValue(program, indirect.Result, $"instruction {pc} result");
                if (indirect.Result.Shape.Type != indirect.ResultType)
                    throw Invalid($"instruction {pc} indirect result type does not match its register shape");
                break;
            default:
                throw Invalid($"instruction {pc} has unsupported descriptor kind `{kind}`");
        }
    }

    private static void VerifyNativeCall(CoflowRegisterProgram program, CoflowNativeCallSite site, int pc)
    {
        if (site.Arguments.Length != site.Call.ArgumentCount)
            throw Invalid($"instruction {pc} native argument count does not match signature");
        for (var index = 0; index < site.Arguments.Length; index++)
        {
            VerifyValue(program, site.Arguments[index], $"instruction {pc} native argument {index}");
            if (site.Arguments[index].Shape.Type != site.Call.ParameterTypes[index])
                throw Invalid($"instruction {pc} native argument {index} type does not match signature");
        }
        VerifyValue(program, site.Result, $"instruction {pc} native result");
        if (site.Result.Shape.Type != site.Call.ResultType)
            throw Invalid($"instruction {pc} native result type does not match signature");
    }

    private static void VerifyCollection(CoflowRegisterProgram program, CoflowRegisterCollectionSite site, int pc)
    {
        foreach (var value in site.First) VerifyValue(program, value, $"instruction {pc} collection item");
        if (site.Second is { } second)
        {
            if (second.Length != site.First.Length)
                throw Invalid($"instruction {pc} dictionary key/value counts do not match");
            foreach (var value in second) VerifyValue(program, value, $"instruction {pc} dictionary value");
        }
        VerifyValue(program, site.Target, $"instruction {pc} collection target");
    }

    private static void VerifyCall(CoflowRegisterProgram program, CoflowRegisterCallSite site, int pc)
    {
        var count = site.VmParameterTypes.Length;
        if (site.SourceArguments.Length != count || site.Arguments.Length != count || site.CopyArguments.Length != count)
            throw Invalid($"instruction {pc} call argument arrays do not match signature");
        if (site.ProgramIndex < 0) throw Invalid($"instruction {pc} has a negative program index");
        VerifyWindowBase(site.IntegerWindowBase, site.Arguments, value => value.Shape.IntegerCount, pc, "integer");
        VerifyWindowBase(site.FloatWindowBase, site.Arguments, value => value.Shape.FloatCount, pc, "float");
        VerifyWindowBase(site.ReferenceWindowBase, site.Arguments, value => value.Shape.ReferenceCount, pc, "reference");
        for (var index = 0; index < count; index++)
        {
            VerifyValue(program, site.SourceArguments[index], $"instruction {pc} call source {index}");
            VerifyValue(program, site.Arguments[index], $"instruction {pc} call window {index}");
            if (site.SourceArguments[index].Shape.Type != site.VmParameterTypes[index] ||
                site.Arguments[index].Shape.Type != site.VmParameterTypes[index])
                throw Invalid($"instruction {pc} call argument {index} type does not match signature");
        }
        VerifyValue(program, site.Result, $"instruction {pc} call result");
        if (site.Result.Shape.Type != site.Signature.ResultType)
            throw Invalid($"instruction {pc} call result type does not match signature");
    }

    private static void VerifyWindowBase(
        int windowBase,
        IReadOnlyList<CoflowValueRegister> arguments,
        Func<CoflowValueRegister, int> width,
        int pc,
        string lane)
    {
        var hasLane = arguments.Any(value => width(value) != 0);
        if ((hasLane && windowBase < 0) || (!hasLane && windowBase != -1))
            throw Invalid($"instruction {pc} has an invalid {lane} call window base");
    }

    private static bool[] VerifyControlFlow(CoflowFrozenArray<CoflowRegisterInstruction> instructions)
    {
        var reachable = new bool[instructions.Length];
        var pending = new Queue<int>();
        pending.Enqueue(0);
        var hasTerminal = false;
        while (pending.Count != 0)
        {
            var pc = pending.Dequeue();
            if (reachable[pc]) continue;
            reachable[pc] = true;
            var instruction = instructions[pc];
            var controlFlow = CoflowRegisterInstructionSpec.ControlFlow(instruction.Code);
            if (controlFlow == CoflowRegisterControlFlow.Terminal)
            {
                hasTerminal = true;
                continue;
            }
            if (controlFlow == CoflowRegisterControlFlow.Jump)
            {
                pending.Enqueue(instruction.A);
                continue;
            }
            if (controlFlow == CoflowRegisterControlFlow.Branch)
                pending.Enqueue(instruction.B);
            if (pc + 1 >= instructions.Length)
                throw Invalid($"reachable path falls through after instruction {pc}");
            pending.Enqueue(pc + 1);
        }
        if (!hasTerminal) throw Invalid("program has no reachable return or tail call");
        return reachable;
    }

    private static void VerifyDefiniteAssignment(CoflowRegisterProgram program, bool[] reachable)
    {
        var predecessors = Enumerable.Range(0, program.Instructions.Length)
            .Select(_ => new List<int>()).ToArray();
        for (var pc = 0; pc < program.Instructions.Length; pc++)
        {
            if (!reachable[pc]) continue;
            foreach (var successor in Successors(program.Instructions, pc))
                predecessors[successor].Add(pc);
        }

        // 确定赋值是 must analysis：除入口外先取全集，再持续取所有前驱的交集。
        var entry = new AssignmentState(program, assigned: false);
        foreach (var parameter in program.Parameters) entry.Assign(parameter);
        var inputs = new AssignmentState?[program.Instructions.Length];
        inputs[0] = entry;
        for (var pc = 1; pc < inputs.Length; pc++)
            if (reachable[pc]) inputs[pc] = new AssignmentState(program, assigned: true);

        var changed = true;
        while (changed)
        {
            changed = false;
            for (var pc = 1; pc < inputs.Length; pc++)
            {
                if (!reachable[pc]) continue;
                AssignmentState? merged = null;
                foreach (var predecessor in predecessors[pc])
                {
                    var output = inputs[predecessor]!.Clone();
                    ApplyWrites(program, predecessor, output);
                    if (merged is null) merged = output;
                    else merged.Intersect(output);
                }
                if (merged is not null && !inputs[pc]!.SameAs(merged))
                {
                    inputs[pc] = merged;
                    changed = true;
                }
            }
        }

        for (var pc = 0; pc < inputs.Length; pc++)
        {
            if (!reachable[pc]) continue;
            VerifyReads(program, pc, inputs[pc]!);
        }
    }

    private static IEnumerable<int> Successors(CoflowFrozenArray<CoflowRegisterInstruction> instructions, int pc)
    {
        var instruction = instructions[pc];
        var controlFlow = CoflowRegisterInstructionSpec.ControlFlow(instruction.Code);
        if (controlFlow == CoflowRegisterControlFlow.Terminal) yield break;
        if (controlFlow == CoflowRegisterControlFlow.Jump)
        {
            yield return instruction.A;
            yield break;
        }
        if (controlFlow == CoflowRegisterControlFlow.Branch)
            yield return instruction.B;
        yield return pc + 1;
    }

    private static void VerifyReads(CoflowRegisterProgram program, int pc, AssignmentState state)
    {
        var instruction = program.Instructions[pc];
        var operands = new[] { instruction.A, instruction.B, instruction.C };
        for (var index = 0; index < operands.Length; index++)
        {
            if (CoflowRegisterInstructionSpec.OperandAccess(instruction.Code, index) == CoflowRegisterOperandAccess.Read)
                state.Require(CoflowRegisterInstructionSpec.OperandKind(instruction.Code, index)!.Value,
                    operands[index], $"instruction {pc} operand {(char)('A' + index)}");
        }
        VisitDescriptorValues(program, pc, read => state.Require(read, $"instruction {pc}"), _ => { });
    }

    private static void ApplyWrites(CoflowRegisterProgram program, int pc, AssignmentState state)
    {
        var instruction = program.Instructions[pc];
        var operands = new[] { instruction.A, instruction.B, instruction.C };
        for (var index = 0; index < operands.Length; index++)
        {
            if (CoflowRegisterInstructionSpec.OperandAccess(instruction.Code, index) == CoflowRegisterOperandAccess.Write)
                state.Assign(CoflowRegisterInstructionSpec.OperandKind(instruction.Code, index)!.Value, operands[index]);
        }
        VisitDescriptorValues(program, pc, _ => { }, state.Assign);
    }

    private static void VisitDescriptorValues(
        CoflowRegisterProgram program,
        int pc,
        Action<CoflowValueRegister> read,
        Action<CoflowValueRegister> write)
    {
        var instruction = program.Instructions[pc];
        var operations = program.Operations;
        switch (CoflowRegisterInstructionSpec.Descriptor(instruction.Code))
        {
            case CoflowRegisterDescriptorKind.Constant:
                write(operations.Constants[instruction.C].Target);
                break;
            case CoflowRegisterDescriptorKind.Transfer:
                var transfer = operations.Transfers[instruction.C];
                read(transfer.Source);
                write(transfer.Target);
                break;
            case CoflowRegisterDescriptorKind.FieldValue:
                write(operations.FieldValues[instruction.C].Target);
                break;
            case CoflowRegisterDescriptorKind.NativeCall:
                var native = operations.NativeCalls[instruction.C];
                foreach (var argument in native.Arguments) read(argument);
                write(native.Result);
                break;
            case CoflowRegisterDescriptorKind.Collection:
                var collection = operations.Collections[instruction.C];
                foreach (var value in collection.First) read(value);
                if (collection.Second is { } second)
                    foreach (var value in second) read(value);
                write(collection.Target);
                break;
            case CoflowRegisterDescriptorKind.Index:
                var index = operations.Indexes[instruction.C];
                read(index.Collection);
                read(index.Index);
                write(index.Target);
                break;
            case CoflowRegisterDescriptorKind.CollectionRead:
                var collectionRead = operations.CollectionReads[instruction.C];
                read(collectionRead.Collection);
                if (collectionRead.Index is { } collectionIndex) read(collectionIndex);
                write(collectionRead.Target);
                break;
            case CoflowRegisterDescriptorKind.Projection:
                var projection = operations.Projections[instruction.C];
                read(projection.Source);
                write(projection.Target);
                break;
            case CoflowRegisterDescriptorKind.CollectionBuiltin:
                var builtin = operations.CollectionBuiltins[instruction.C];
                read(builtin.Receiver);
                if (builtin.Argument is { } builtinArgument) read(builtinArgument);
                write(builtin.Target);
                break;
            case CoflowRegisterDescriptorKind.ArrayBuilder:
                var builder = operations.ArrayBuilders[instruction.C];
                read(builder.CollectionOrCapacity);
                if (builder.Item is { } item) read(item);
                write(builder.Target);
                break;
            case CoflowRegisterDescriptorKind.Target:
                var target = operations.Targets[instruction.C].Target;
                if (instruction.Code == CoflowRegisterOpCode.Return) read(target);
                else write(target);
                break;
            case CoflowRegisterDescriptorKind.Propagate:
                var propagate = operations.Propagates[instruction.C];
                read(propagate.Source);
                write(propagate.Payload);
                break;
            case CoflowRegisterDescriptorKind.Closure:
                var closure = operations.Closures[instruction.C];
                foreach (var capture in closure.Captures) read(capture);
                write(closure.Target);
                break;
            case CoflowRegisterDescriptorKind.Call:
                var call = operations.Calls[instruction.C];
                foreach (var argument in call.SourceArguments) read(argument);
                if (instruction.Code == CoflowRegisterOpCode.Call) write(call.Result);
                break;
            case CoflowRegisterDescriptorKind.IndirectCall:
                var indirect = operations.IndirectCalls[instruction.C];
                read(indirect.Callable);
                foreach (var argument in indirect.Arguments) read(argument);
                if (instruction.Code == CoflowRegisterOpCode.CallIndirect) write(indirect.Result);
                break;
        }
    }

    private sealed class AssignmentState
    {
        private readonly bool[] _integers;
        private readonly bool[] _floats;
        private readonly bool[] _references;

        internal AssignmentState(CoflowRegisterProgram program, bool assigned)
        {
            _integers = Enumerable.Repeat(assigned, program.IntegerRegisterCount).ToArray();
            _floats = Enumerable.Repeat(assigned, program.FloatRegisterCount).ToArray();
            _references = Enumerable.Repeat(assigned, program.ReferenceRegisterCount).ToArray();
        }

        private AssignmentState(bool[] integers, bool[] floats, bool[] references)
        {
            _integers = integers;
            _floats = floats;
            _references = references;
        }

        internal AssignmentState Clone() => new(
            (bool[])_integers.Clone(), (bool[])_floats.Clone(), (bool[])_references.Clone());

        internal void Assign(CoflowValueRegister value)
        {
            Assign(_integers, value.IntegerBase, value.Shape.IntegerCount);
            Assign(_floats, value.FloatBase, value.Shape.FloatCount);
            Assign(_references, value.ReferenceBase, value.Shape.ReferenceCount);
        }

        internal void Assign(CoflowRegisterKind kind, int index) => Registers(kind)[index] = true;

        internal void Require(CoflowValueRegister value, string location)
        {
            Require(_integers, value.IntegerBase, value.Shape.IntegerCount, location, "integer");
            Require(_floats, value.FloatBase, value.Shape.FloatCount, location, "float");
            Require(_references, value.ReferenceBase, value.Shape.ReferenceCount, location, "reference");
        }

        internal void Require(CoflowRegisterKind kind, int index, string location)
        {
            if (!Registers(kind)[index])
                throw Invalid($"{location} reads unassigned {kind.ToString().ToLowerInvariant()} register {index}");
        }

        internal void Intersect(AssignmentState other)
        {
            Intersect(_integers, other._integers);
            Intersect(_floats, other._floats);
            Intersect(_references, other._references);
        }

        internal bool SameAs(AssignmentState other) =>
            _integers.SequenceEqual(other._integers) && _floats.SequenceEqual(other._floats) &&
            _references.SequenceEqual(other._references);

        private bool[] Registers(CoflowRegisterKind kind) => kind switch
        {
            CoflowRegisterKind.Integer => _integers,
            CoflowRegisterKind.Float => _floats,
            CoflowRegisterKind.Reference => _references,
            _ => throw Invalid($"unknown register kind `{kind}`"),
        };

        private static void Assign(bool[] lanes, int start, int count)
        {
            for (var index = start; index < start + count; index++) lanes[index] = true;
        }

        private static void Require(bool[] lanes, int start, int count, string location, string kind)
        {
            for (var index = start; index < start + count; index++)
                if (!lanes[index]) throw Invalid($"{location} reads unassigned {kind} register {index}");
        }

        private static void Intersect(bool[] target, bool[] source)
        {
            for (var index = 0; index < target.Length; index++) target[index] &= source[index];
        }
    }

    private static void VerifyValue(CoflowRegisterProgram program, CoflowValueRegister value, string location)
    {
        if (value.Shape is null) throw Invalid($"{location} has no value shape");
        VerifyRange(value.IntegerBase, value.Shape.IntegerCount, program.IntegerRegisterCount, $"{location} integer lanes");
        VerifyRange(value.FloatBase, value.Shape.FloatCount, program.FloatRegisterCount, $"{location} float lanes");
        VerifyRange(value.ReferenceBase, value.Shape.ReferenceCount, program.ReferenceRegisterCount, $"{location} reference lanes");
    }

    private static void VerifyRegister(CoflowRegisterProgram program, CoflowRegisterKind kind, int index, string location)
    {
        var count = kind switch
        {
            CoflowRegisterKind.Integer => program.IntegerRegisterCount,
            CoflowRegisterKind.Float => program.FloatRegisterCount,
            CoflowRegisterKind.Reference => program.ReferenceRegisterCount,
            _ => throw Invalid($"{location} has unknown register kind `{kind}`"),
        };
        VerifyIndex(index, count, location);
    }

    private static void VerifyTransferShape(
        CoflowRegisterOpCode code,
        CoflowRegisterValueTransfer transfer,
        int pc)
    {
        var source = transfer.Source;
        var target = code switch
        {
            CoflowRegisterOpCode.MakeOptionSome or CoflowRegisterOpCode.MakeResultOk => transfer.Target.First,
            CoflowRegisterOpCode.MakeResultErr => transfer.Target.Second,
            _ => transfer.Target,
        };
        var sameLayout = source.Shape.IntegerCount == target.Shape.IntegerCount &&
            source.Shape.FloatCount == target.Shape.FloatCount &&
            source.Shape.ReferenceCount == target.Shape.ReferenceCount;
        if (!sameLayout)
            throw Invalid(
                $"instruction {pc} transfer shapes do not match: " +
                $"`{source.Shape.Type}` ({source.Shape.IntegerCount}/{source.Shape.FloatCount}/{source.Shape.ReferenceCount}) -> " +
                $"`{target.Shape.Type}` ({target.Shape.IntegerCount}/{target.Shape.FloatCount}/{target.Shape.ReferenceCount})");

        if (code == CoflowRegisterOpCode.MoveValue)
        {
            // retype 只改变静态类型；集合句柄不可借布局相同伪装成整数或另一种集合。
            var sourceCollection = source.Shape.Kind == CoflowValueShapeKind.Collection;
            var targetCollection = target.Shape.Kind == CoflowValueShapeKind.Collection;
            if ((sourceCollection || targetCollection) &&
                (!sourceCollection || !targetCollection || source.Shape.Type != target.Shape.Type))
                throw Invalid($"instruction {pc} reinterprets an incompatible collection handle");
            return;
        }

        if (source.Shape.Type != target.Shape.Type)
            throw Invalid($"instruction {pc} transfer types do not match");
    }

    private static T Item<T>(CoflowFrozenArray<T> items, int index, int pc)
    {
        VerifyIndex(index, items.Length, $"instruction {pc} descriptor");
        return items[index] ?? throw Invalid($"instruction {pc} has a null descriptor");
    }

    private static void VerifyJump(int target, int count, int pc)
    {
        if ((uint)target >= (uint)count) throw Invalid($"instruction {pc} has invalid jump target {target}");
    }

    private static void VerifyIndex(int index, int count, string location)
    {
        if ((uint)index >= (uint)count) throw Invalid($"{location} index {index} is out of range");
    }

    private static void VerifyRange(int start, int width, int count, string location)
    {
        if (start < 0 || width < 0 || start > count || width > count - start)
            throw Invalid($"{location} [{start}, {start + (long)width}) is out of range");
    }

    private static InvalidOperationException Invalid(string message) =>
        new($"Invalid Coflow register program: {message}.");
}
}
