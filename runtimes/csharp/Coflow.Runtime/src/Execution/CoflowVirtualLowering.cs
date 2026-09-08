using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

/// <summary>将 typed CFG 链接为最终寄存器程序；寄存器复用由后续活性分配阶段负责。</summary>
internal static class CoflowVirtualLowering
{
    internal static CoflowRegisterProgram Lower(
        CoflowVirtualProgram program,
        CoflowProgramLinker? linker = null,
        CoflowRegisterAllocation? cachedAllocation = null) =>
        LowerTemplate(program, linker, cachedAllocation).InitialProgram;

    internal static CoflowRegisterTemplate LowerTemplate(
        CoflowVirtualProgram program,
        CoflowProgramLinker? linker = null,
        CoflowRegisterAllocation? cachedAllocation = null)
    {
        var allocation = cachedAllocation ?? CoflowVirtualRegisterAllocator.Allocate(program);
        var registers = allocation.Registers;
        var integerCount = allocation.IntegerCount;
        var floatCount = allocation.FloatCount;
        var referenceCount = allocation.ReferenceCount;
        var directCalls = program.Blocks.SelectMany(block => block.Instructions)
            .Select(instruction => instruction.Operation)
            .OfType<CoflowVirtualOperation.DirectCall>()
            .Select(operation => operation.Call)
            .Concat(program.Blocks.Select(block => block.Terminator)
                .OfType<CoflowBlockTerminator.DirectTailCall>()
                .Select(terminator => terminator.Call))
            .ToArray();
        var outgoingIntegerBase = integerCount;
        var outgoingFloatBase = floatCount;
        var outgoingReferenceBase = referenceCount;
        if (directCalls.Length != 0)
        {
            integerCount += directCalls.Max(call => call.VmParameterTypes.Sum(type => CoflowValueShape.Of(type).IntegerCount));
            floatCount += directCalls.Max(call => call.VmParameterTypes.Sum(type => CoflowValueShape.Of(type).FloatCount));
            referenceCount += directCalls.Max(call => call.VmParameterTypes.Sum(type => CoflowValueShape.Of(type).ReferenceCount));
        }

        var blockStarts = new int[program.Blocks.Length];
        var instructionCount = 0;
        foreach (var block in program.Blocks)
        {
            blockStarts[block.Index] = instructionCount;
            instructionCount += block.Instructions.Count + TerminatorSize(block.Terminator!);
            for (var index = 0; index < block.Instructions.Count; index++)
            {
                instructionCount += allocation.ReferenceDeaths[block.Index][index]
                    .SelectMany(value => Enumerable.Range(
                        registers[value.Index].ReferenceBase,
                        registers[value.Index].Shape.ReferenceCount))
                    .Distinct().Count();
            }
        }

        var instructions = new List<CoflowRegisterInstruction>(instructionCount);
        var spans = new List<CfdSpan?>(instructionCount);
        var operations = new CoflowRegisterOperations.Builder();
        var relocations = new CoflowRelocationBuilder();
        foreach (var block in program.Blocks)
        {
            for (var instructionIndex = 0; instructionIndex < block.Instructions.Count; instructionIndex++)
            {
                var instruction = block.Instructions[instructionIndex];
                LowerInstruction(
                    instruction, registers, operations, relocations, linker,
                    outgoingIntegerBase, outgoingFloatBase, outgoingReferenceBase,
                    instructions, spans);
                var cleared = new HashSet<int>();
                foreach (var dead in allocation.ReferenceDeaths[block.Index][instructionIndex])
                {
                    var register = registers[dead.Index];
                    for (var lane = 0; lane < register.Shape.ReferenceCount; lane++)
                    {
                        var reference = register.ReferenceBase + lane;
                        if (!cleared.Add(reference)) continue;
                        instructions.Add(new CoflowRegisterInstruction(
                            CoflowRegisterOpCode.ClearReference, reference));
                        spans.Add(instruction.Origin.Span);
                    }
                }
            }
            LowerTerminator(
                block.Terminator!, registers, operations, relocations, linker,
                outgoingIntegerBase, outgoingFloatBase, outgoingReferenceBase,
                blockStarts, instructions, spans);
        }

        try
        {
            var lowered = new CoflowRegisterProgram(
                program.Parameters.Select(value => registers[value.Index]).ToArray(),
                instructions.ToArray(),
                spans.ToArray(),
                Array.Empty<long>(),
                operations.Build(),
                integerCount,
                floatCount,
                referenceCount);
            return new CoflowRegisterTemplate(
                lowered,
                relocations.Constants,
                relocations.NativeCalls,
                relocations.Closures,
                relocations.Calls);
        }
        catch (InvalidOperationException error)
        {
            throw new InvalidOperationException(
                $"Failed to lower Coflow function `{program.Identity}`: {error.Message}", error);
        }
    }

    private static void LowerInstruction(
        CoflowVirtualInstruction instruction,
        CoflowValueRegister[] registers,
        CoflowRegisterOperations.Builder operations,
        CoflowRelocationBuilder relocations,
        CoflowProgramLinker? linker,
        int outgoingIntegerBase,
        int outgoingFloatBase,
        int outgoingReferenceBase,
        List<CoflowRegisterInstruction> output,
        List<CfdSpan?> spans)
    {
        var code = Code(instruction);
        var symbol = Symbol(instruction.Operation);
        if (code is CoflowRegisterOpCode.ConstantInteger or CoflowRegisterOpCode.ConstantFloat or
            CoflowRegisterOpCode.ConstantReference or CoflowRegisterOpCode.ConstantValue)
        {
            var target = Register(instruction.Target, registers);
            var encoded = CoflowEncodedValue.Encode(target.Shape.Type, LinkSymbol(symbol, linker));
            var descriptor = operations.Add(
                CoflowRegisterOpCode.ConstantValue,
                new CoflowRegisterConstantSite(encoded, target));
            if (symbol is CoflowFunctionReferenceTemplate or
                CoflowRecordReferenceTemplate or CoflowConstantReferenceTemplate)
                relocations.Constants.Add(new(descriptor, target.Shape.Type, symbol));
            Add(new CoflowRegisterInstruction(CoflowRegisterOpCode.ConstantValue, C: descriptor));
            return;
        }

        if (code == CoflowRegisterOpCode.MoveValue)
        {
            RequireInputCount(instruction, 1);
            var transferSource = registers[instruction.Inputs[0].Index];
            var descriptor = operations.Add(
                code,
                new CoflowRegisterValueTransfer(
                    transferSource,
                    Register(instruction.Target, registers)));
            Add(new CoflowRegisterInstruction(code, C: descriptor));
            return;
        }

        if (code == CoflowRegisterOpCode.MakeOptionNone)
        {
            var descriptor = operations.Add(
                code,
                new CoflowRegisterTargetSite(Register(instruction.Target, registers)));
            Add(new CoflowRegisterInstruction(code, C: descriptor));
            return;
        }

        if (code is CoflowRegisterOpCode.MakeOptionSome or
            CoflowRegisterOpCode.MakeResultOk or CoflowRegisterOpCode.MakeResultErr or
            CoflowRegisterOpCode.ReadFirstPayload or CoflowRegisterOpCode.ReadSecondPayload)
        {
            RequireInputCount(instruction, 1);
            var transferSource = registers[instruction.Inputs[0].Index];
            if (code == CoflowRegisterOpCode.ReadFirstPayload)
                transferSource = transferSource.First;
            else if (code == CoflowRegisterOpCode.ReadSecondPayload)
                transferSource = transferSource.Second;
            var descriptor = operations.Add(
                code,
                new CoflowRegisterValueTransfer(
                    transferSource,
                    Register(instruction.Target, registers)));
            Add(new CoflowRegisterInstruction(code, C: descriptor));
            return;
        }

        if (code is CoflowRegisterOpCode.MakeArray or CoflowRegisterOpCode.MakeDictionary)
        {
            var target = Register(instruction.Target, registers);
            CoflowValueRegister[] first;
            CoflowValueRegister[]? second = null;
            if (code == CoflowRegisterOpCode.MakeDictionary)
            {
                if ((instruction.Inputs.Length & 1) != 0)
                    throw Invalid(instruction, "requires key/value input pairs");
                first = instruction.Inputs.Where((_, index) => (index & 1) == 0)
                    .Select(value => registers[value.Index]).ToArray();
                second = instruction.Inputs.Where((_, index) => (index & 1) != 0)
                    .Select(value => registers[value.Index]).ToArray();
            }
            else first = instruction.Inputs.Select(value => registers[value.Index]).ToArray();
            var descriptor = operations.Add(
                code,
                new CoflowRegisterCollectionSite(first, second, target));
            Add(new CoflowRegisterInstruction(code, C: descriptor));
            return;
        }

        if (code is CoflowRegisterOpCode.ArrayIndex or CoflowRegisterOpCode.DictionaryIndex)
        {
            RequireInputCount(instruction, 2);
            var descriptor = operations.Add(
                code,
                new CoflowRegisterArrayIndexSite(
                    registers[instruction.Inputs[0].Index],
                    registers[instruction.Inputs[1].Index],
                    Register(instruction.Target, registers)));
            Add(new CoflowRegisterInstruction(code, C: descriptor));
            return;
        }

        if (code is CoflowRegisterOpCode.CollectionCount or CoflowRegisterOpCode.ArrayItem or
            CoflowRegisterOpCode.DictionaryKey or CoflowRegisterOpCode.DictionaryValue)
        {
            if (instruction.Inputs.Length is < 1 or > 2)
                throw Invalid(instruction, "requires a collection and optional index");
            var descriptor = operations.Add(
                code,
                new CoflowRegisterCollectionReadSite(
                    registers[instruction.Inputs[0].Index],
                    instruction.Inputs.Length == 2 ? registers[instruction.Inputs[1].Index] : null,
                    Register(instruction.Target, registers)));
            Add(new CoflowRegisterInstruction(code, C: descriptor));
            return;
        }

        if (code is CoflowRegisterOpCode.DictionaryKeys or CoflowRegisterOpCode.DictionaryValues)
        {
            RequireInputCount(instruction, 1);
            var descriptor = operations.Add(
                code,
                new CoflowRegisterCollectionProjectionSite(
                    registers[instruction.Inputs[0].Index],
                    Register(instruction.Target, registers)));
            Add(new CoflowRegisterInstruction(code, C: descriptor));
            return;
        }

        if (code == CoflowRegisterOpCode.CollectionBuiltin)
        {
            if (instruction.Inputs.Length is < 1 or > 2)
                throw Invalid(instruction, "requires a receiver and optional argument");
            var descriptor = operations.Add(
                code,
                new CoflowRegisterCollectionBuiltinSite(
                    (CoflowBuiltin)symbol!,
                    registers[instruction.Inputs[0].Index],
                    instruction.Inputs.Length == 2 ? registers[instruction.Inputs[1].Index] : null,
                    Register(instruction.Target, registers)));
            Add(new CoflowRegisterInstruction(code, C: descriptor));
            return;
        }

        if (code is CoflowRegisterOpCode.BeginArrayBuilder or CoflowRegisterOpCode.AppendArrayBuilder)
        {
            var expected = code == CoflowRegisterOpCode.BeginArrayBuilder ? 1 : 2;
            RequireInputCount(instruction, expected);
            var descriptor = operations.Add(
                code,
                new CoflowRegisterArrayBuilderSite(
                    registers[instruction.Inputs[0].Index],
                    expected == 2 ? registers[instruction.Inputs[1].Index] : null,
                    Register(instruction.Target, registers)));
            Add(new CoflowRegisterInstruction(code, C: descriptor));
            return;
        }

        if (code is CoflowRegisterOpCode.IsType or CoflowRegisterOpCode.IsArenaType)
        {
            RequireInputCount(instruction, 1);
            var target = Register(instruction.Target, registers).Scalar;
            var source = registers[instruction.Inputs[0].Index].Scalar;
            var descriptor = operations.Add(code, (Type)symbol!);
            Add(new CoflowRegisterInstruction(code, target.Index, source.Index, descriptor));
            return;
        }

        if (code == CoflowRegisterOpCode.ReadValueTag)
        {
            RequireInputCount(instruction, 1);
            var target = Register(instruction.Target, registers).Scalar;
            var source = registers[instruction.Inputs[0].Index];
            Add(new CoflowRegisterInstruction(code, target.Index, source.IntegerBase));
            return;
        }

        if (symbol is CoflowFieldAccess { ReceiverIsStruct: true } structField)
        {
            RequireInputCount(instruction, 1);
            var receiver = registers[instruction.Inputs[0].Index];
            if (receiver.Shape.Kind != CoflowValueShapeKind.Struct)
                throw Invalid(instruction, $"field `{structField.Name}` requires a struct receiver layout");
            var target = Register(instruction.Target, registers);
            if (structField.IsFunction)
            {
                var descriptor = operations.Add(
                    CoflowRegisterOpCode.Native,
                    new CoflowNativeCallSite(structField.Call, new[] { receiver }, target));
                Add(new CoflowRegisterInstruction(CoflowRegisterOpCode.Native, C: descriptor));
                return;
            }

            // struct 字段已经位于 receiver 的连续 lane 中，直接复制对应切片。
            var source = new CoflowValueRegister(
                target.Shape,
                receiver.IntegerBase + structField.IntegerOffset,
                receiver.FloatBase + structField.FloatOffset,
                receiver.ReferenceBase + structField.ReferenceOffset);
            var transfer = operations.Add(
                CoflowRegisterOpCode.MoveValue,
                new CoflowRegisterValueTransfer(source, target));
            Add(new CoflowRegisterInstruction(CoflowRegisterOpCode.MoveValue, C: transfer));
            return;
        }

        if (code is CoflowRegisterOpCode.LoadHostFieldValue or CoflowRegisterOpCode.LoadArenaFieldValue)
        {
            RequireInputCount(instruction, 1);
            var receiverRegister = registers[instruction.Inputs[0].Index];
            // 复合 receiver 本身不是标量；字段读取只需要其宿主引用或 Arena 身份所在的 lane。
            var receiver = code == CoflowRegisterOpCode.LoadHostFieldValue
                ? receiverRegister.ReferenceBase
                : receiverRegister.IntegerBase;
            var descriptor = operations.Add(
                code,
                new CoflowRegisterFieldValueSite(
                    (CoflowFieldAccess)symbol!,
                    Register(instruction.Target, registers)));
            Add(new CoflowRegisterInstruction(code, receiver, C: descriptor));
            return;
        }

        if (code == CoflowRegisterOpCode.Native)
        {
            var linked = LinkSymbol(symbol, linker);
            var descriptor = operations.Add(
                code,
                new CoflowNativeCallSite(
                    (CoflowNativeCall)linked!,
                    instruction.Inputs.Select(value => registers[value.Index]).ToArray(),
                    Register(instruction.Target, registers)));
            if (symbol is CoflowFunctionReferenceTemplate)
                relocations.NativeCalls.Add(new(descriptor, symbol));
            Add(new CoflowRegisterInstruction(code, C: descriptor));
            return;
        }

        if (code == CoflowRegisterOpCode.MakeClosure)
        {
            if (linker is null) throw Invalid(instruction, "requires a program linker");
            var template = ((CoflowClosureProgramTemplate)symbol!).Link(linker);
            var descriptor = operations.Add(
                code,
                new CoflowRegisterClosureSite(
                    template,
                    instruction.Inputs.Select(value => registers[value.Index]).ToArray(),
                    Register(instruction.Target, registers)));
            relocations.Closures.Add(new(descriptor, (CoflowClosureProgramTemplate)symbol!));
            Add(new CoflowRegisterInstruction(code, C: descriptor));
            return;
        }

        if (code == CoflowRegisterOpCode.Call)
        {
            if (linker is null) throw Invalid(instruction, "requires a program linker");
            var call = (CoflowCallSite)symbol!;
            var sourceArguments = instruction.Inputs.Select(value => registers[value.Index]).ToArray();
            var descriptor = operations.Add(
                code,
                BuildDirectCallSite(
                    call, sourceArguments, Register(instruction.Target, registers), linker,
                    outgoingIntegerBase, outgoingFloatBase, outgoingReferenceBase));
            relocations.Calls.Add(new(descriptor, call));
            Add(new CoflowRegisterInstruction(code, C: descriptor));
            return;
        }

        if (code == CoflowRegisterOpCode.CallIndirect)
        {
            if (instruction.Inputs.Length == 0) throw Invalid(instruction, "has no callable input");
            var descriptor = operations.Add(
                code,
                new CoflowRegisterIndirectCallSite(
                    registers[instruction.Inputs[0].Index],
                    instruction.Inputs.Skip(1).Select(value => registers[value.Index]).ToArray(),
                    Register(instruction.Target, registers),
                    Register(instruction.Target, registers).Shape.Type));
            Add(new CoflowRegisterInstruction(code, C: descriptor));
            return;
        }

        var spec = DirectSpec(code);
        var operands = new int[3];
        var inputIndex = 0;
        for (var operand = 0; operand < operands.Length; operand++)
        {
            var access = CoflowRegisterInstructionSpec.OperandAccess(code, operand);
            if (access == CoflowRegisterOperandAccess.None) continue;
            var value = access == CoflowRegisterOperandAccess.Write
                ? Register(instruction.Target, registers)
                : inputIndex < instruction.Inputs.Length
                    ? registers[instruction.Inputs[inputIndex++].Index]
                    : throw Invalid(instruction, "has too few inputs");
            operands[operand] = Scalar(value, spec[operand], instruction);
        }
        if (inputIndex != instruction.Inputs.Length)
            throw Invalid(instruction, "has too many inputs");
        Add(new CoflowRegisterInstruction(code, operands[0], operands[1], operands[2]));

        void Add(CoflowRegisterInstruction lowered)
        {
            output.Add(lowered);
            spans.Add(instruction.Origin.Span);
        }
    }

    private static void LowerTerminator(
        CoflowBlockTerminator terminator,
        CoflowValueRegister[] registers,
        CoflowRegisterOperations.Builder operations,
        CoflowRelocationBuilder relocations,
        CoflowProgramLinker? linker,
        int outgoingIntegerBase,
        int outgoingFloatBase,
        int outgoingReferenceBase,
        int[] blockStarts,
        List<CoflowRegisterInstruction> output,
        List<CfdSpan?> spans)
    {
        switch (terminator)
        {
            case CoflowBlockTerminator.Jump jump:
                Add(new CoflowRegisterInstruction(CoflowRegisterOpCode.Jump, blockStarts[jump.TargetBlock]));
                break;
            case CoflowBlockTerminator.Branch branch:
                var condition = registers[branch.Condition.Index].Scalar;
                if (condition.Kind != CoflowRegisterKind.Integer)
                    throw new InvalidOperationException("A CFG branch condition must use an integer register.");
                Add(new CoflowRegisterInstruction(
                    CoflowRegisterOpCode.JumpIfFalse,
                    condition.Index,
                    blockStarts[branch.FalseBlock]));
                Add(new CoflowRegisterInstruction(CoflowRegisterOpCode.Jump, blockStarts[branch.TrueBlock]));
                break;
            case CoflowBlockTerminator.Return @return:
                var descriptor = operations.Add(
                    CoflowRegisterOpCode.Return,
                    new CoflowRegisterTargetSite(registers[@return.Value.Index]));
                Add(new CoflowRegisterInstruction(CoflowRegisterOpCode.Return, C: descriptor));
                break;
            case CoflowBlockTerminator.DirectTailCall tailCall:
                if (linker is null)
                    throw new InvalidOperationException("A CFG tail call requires a program linker.");
                var sourceArguments = tailCall.Inputs.Select(value => registers[value.Index]).ToArray();
                var callDescriptor = operations.Add(
                    CoflowRegisterOpCode.TailCall,
                    BuildDirectCallSite(
                        tailCall.Call, sourceArguments, registers[tailCall.Result.Index], linker,
                        outgoingIntegerBase, outgoingFloatBase, outgoingReferenceBase));
                relocations.Calls.Add(new(callDescriptor, tailCall.Call));
                Add(new CoflowRegisterInstruction(CoflowRegisterOpCode.TailCall, C: callDescriptor));
                break;
            case CoflowBlockTerminator.IndirectTailCall tailCall:
                if (tailCall.Inputs.Length == 0)
                    throw new InvalidOperationException("A CFG indirect tail call has no callable input.");
                var indirectDescriptor = operations.Add(
                    CoflowRegisterOpCode.TailCallIndirect,
                    new CoflowRegisterIndirectCallSite(
                        registers[tailCall.Inputs[0].Index],
                        tailCall.Inputs.Skip(1).Select(value => registers[value.Index]).ToArray(),
                        registers[tailCall.Result.Index],
                        registers[tailCall.Result.Index].Shape.Type));
                Add(new CoflowRegisterInstruction(CoflowRegisterOpCode.TailCallIndirect, C: indirectDescriptor));
                break;
            case CoflowBlockTerminator.Propagate propagate:
                var propagateDescriptor = operations.Add(
                    CoflowRegisterOpCode.Propagate,
                    new CoflowRegisterPropagateSite(
                        registers[propagate.Source.Index],
                        registers[propagate.Payload.Index],
                        registers[propagate.ReturnValue.Index]));
                Add(new CoflowRegisterInstruction(CoflowRegisterOpCode.Propagate, C: propagateDescriptor));
                Add(new CoflowRegisterInstruction(CoflowRegisterOpCode.Jump, blockStarts[propagate.ContinueBlock]));
                break;
            default:
                throw new InvalidOperationException(
                    $"CFG terminator `{terminator.GetType().Name}` is not lowered yet.");
        }

        void Add(CoflowRegisterInstruction instruction)
        {
            output.Add(instruction);
            spans.Add(terminator.Origin.Span);
        }
    }

    private static CoflowRegisterKind?[] DirectSpec(CoflowRegisterOpCode code)
    {
        if (CoflowRegisterInstructionSpec.Descriptor(code) != CoflowRegisterDescriptorKind.None)
            throw new InvalidOperationException($"CFG descriptor opcode `{code}` is not lowered yet.");
        return new[]
        {
            CoflowRegisterInstructionSpec.OperandKind(code, 0),
            CoflowRegisterInstructionSpec.OperandKind(code, 1),
            CoflowRegisterInstructionSpec.OperandKind(code, 2),
        };
    }

    private static CoflowRegisterOpCode Code(CoflowVirtualInstruction instruction) =>
        instruction.Operation switch
        {
            CoflowVirtualOperation.Constant => ConstantCode(RegisterType(instruction)),
            CoflowVirtualOperation.Move => CoflowRegisterOpCode.MoveValue,
            CoflowVirtualOperation.Unary unary => UnaryCode(unary.Operator, InputType(instruction, 0)),
            CoflowVirtualOperation.Binary binary => BinaryCode(binary.Operator, InputType(instruction, 0)),
            CoflowVirtualOperation.Convert convert => ConversionCode(convert.SourceType, convert.TargetType),
            CoflowVirtualOperation.TypeTest test => test.UsesArenaIdentity
                ? CoflowRegisterOpCode.IsArenaType
                : CoflowRegisterOpCode.IsType,
            CoflowVirtualOperation.MakeOptionNone => CoflowRegisterOpCode.MakeOptionNone,
            CoflowVirtualOperation.MakeOptionSome => CoflowRegisterOpCode.MakeOptionSome,
            CoflowVirtualOperation.MakeResult result => result.IsOk
                ? CoflowRegisterOpCode.MakeResultOk
                : CoflowRegisterOpCode.MakeResultErr,
            CoflowVirtualOperation.ReadValueTag => CoflowRegisterOpCode.ReadValueTag,
            CoflowVirtualOperation.ReadPayload payload => payload.First
                ? CoflowRegisterOpCode.ReadFirstPayload
                : CoflowRegisterOpCode.ReadSecondPayload,
            CoflowVirtualOperation.MakeCollection collection => collection.Dictionary
                ? CoflowRegisterOpCode.MakeDictionary
                : CoflowRegisterOpCode.MakeArray,
            CoflowVirtualOperation.CollectionIndex index => index.Dictionary
                ? CoflowRegisterOpCode.DictionaryIndex
                : CoflowRegisterOpCode.ArrayIndex,
            CoflowVirtualOperation.CollectionRead read => read.Kind switch
            {
                CoflowVirtualCollectionReadKind.Count => CoflowRegisterOpCode.CollectionCount,
                CoflowVirtualCollectionReadKind.ArrayItem => CoflowRegisterOpCode.ArrayItem,
                CoflowVirtualCollectionReadKind.DictionaryKey => CoflowRegisterOpCode.DictionaryKey,
                CoflowVirtualCollectionReadKind.DictionaryValue => CoflowRegisterOpCode.DictionaryValue,
                _ => throw new InvalidOperationException($"Unknown collection read `{read.Kind}`."),
            },
            CoflowVirtualOperation.DictionaryProjection projection => projection.Values
                ? CoflowRegisterOpCode.DictionaryValues
                : CoflowRegisterOpCode.DictionaryKeys,
            CoflowVirtualOperation.CollectionBuiltin => CoflowRegisterOpCode.CollectionBuiltin,
            CoflowVirtualOperation.ArrayBuilder builder => builder.Append
                ? CoflowRegisterOpCode.AppendArrayBuilder
                : CoflowRegisterOpCode.BeginArrayBuilder,
            CoflowVirtualOperation.FieldRead field => field.Field.IsHost
                ? CoflowRegisterOpCode.LoadHostFieldValue
                : CoflowRegisterOpCode.LoadArenaFieldValue,
            CoflowVirtualOperation.Native => CoflowRegisterOpCode.Native,
            CoflowVirtualOperation.BindFunction => CoflowRegisterOpCode.Native,
            CoflowVirtualOperation.MakeClosure => CoflowRegisterOpCode.MakeClosure,
            CoflowVirtualOperation.DirectCall => CoflowRegisterOpCode.Call,
            CoflowVirtualOperation.IndirectCall => CoflowRegisterOpCode.CallIndirect,
            _ => throw new InvalidOperationException(
                $"CFG operation `{instruction.Operation.GetType().Name}` is not lowered yet."),
        };

    private static object? Symbol(CoflowVirtualOperation operation) => operation switch
    {
        CoflowVirtualOperation.Constant constant => constant.Value,
        CoflowVirtualOperation.TypeTest test => test.TargetType,
        CoflowVirtualOperation.CollectionBuiltin builtin => builtin.Builtin,
        CoflowVirtualOperation.FieldRead field => field.Field,
        CoflowVirtualOperation.Native native => native.Call,
        CoflowVirtualOperation.BindFunction function => function.Template,
        CoflowVirtualOperation.MakeClosure closure => closure.Template,
        CoflowVirtualOperation.DirectCall call => call.Call,
        _ => null,
    };

    private static Type RegisterType(CoflowVirtualInstruction instruction) =>
        instruction.Target?.Type ?? throw Invalid(instruction, "has no target");

    private static Type InputType(CoflowVirtualInstruction instruction, int index) =>
        (uint)index < (uint)instruction.Inputs.Length
            ? instruction.Inputs[index].Type
            : throw Invalid(instruction, $"has no input {index}");

    private static CoflowRegisterOpCode ConstantCode(Type type) =>
        type == typeof(double) ? CoflowRegisterOpCode.ConstantFloat :
        type == typeof(string) ? CoflowRegisterOpCode.ConstantReference :
        type == typeof(Unit) ? CoflowRegisterOpCode.ConstantValue :
        CoflowRegisterOpCode.ConstantInteger;

    private static CoflowRegisterOpCode UnaryCode(string operation, Type operandType) =>
        (operation, operandType) switch
        {
            ("!", _) => CoflowRegisterOpCode.Not,
            ("~", _) => CoflowRegisterOpCode.BitNot,
            ("-", var type) when type == typeof(long) => CoflowRegisterOpCode.NegateInt,
            ("-", var type) when type == typeof(double) => CoflowRegisterOpCode.NegateFloat,
            _ => throw new InvalidOperationException(
                $"Typed unary operation `{operation}` has unsupported operand `{operandType}`."),
        };

    private static CoflowRegisterOpCode ConversionCode(Type source, Type target)
    {
        if (source == typeof(long) && target == typeof(double)) return CoflowRegisterOpCode.ConvertIntToFloat;
        if (source == typeof(double) && target == typeof(long)) return CoflowRegisterOpCode.ConvertFloatToInt;
        if (source == typeof(long) && target.IsEnum) return CoflowRegisterOpCode.MoveValue;
        throw new InvalidOperationException($"Typed conversion from `{source}` to `{target}` has no register lowering.");
    }

    private static CoflowRegisterOpCode BinaryCode(string operation, Type operandType)
    {
        if (operandType.IsEnum) operandType = typeof(long);
        if (operation == "==") return CoflowValueShape.Of(operandType).ScalarKind switch
        {
            CoflowRegisterKind.Integer => CoflowRegisterOpCode.EqualInteger,
            CoflowRegisterKind.Float => CoflowRegisterOpCode.EqualFloat,
            _ => CoflowRegisterOpCode.EqualReference,
        };
        if (operandType == typeof(long)) return operation switch
        {
            "+" => CoflowRegisterOpCode.AddInt, "-" => CoflowRegisterOpCode.SubtractInt,
            "*" => CoflowRegisterOpCode.MultiplyInt, "/" => CoflowRegisterOpCode.DivideInt,
            "//" => CoflowRegisterOpCode.IntegerDivide, "%" => CoflowRegisterOpCode.Remainder,
            "**" => CoflowRegisterOpCode.PowerInt, "<<" => CoflowRegisterOpCode.ShiftLeft,
            ">>" => CoflowRegisterOpCode.ShiftRight, "&" => CoflowRegisterOpCode.BitAnd,
            "^" => CoflowRegisterOpCode.BitXor, "|" => CoflowRegisterOpCode.BitOr,
            "<" => CoflowRegisterOpCode.LessInt, "<=" => CoflowRegisterOpCode.LessOrEqualInt,
            ">" => CoflowRegisterOpCode.GreaterInt, ">=" => CoflowRegisterOpCode.GreaterOrEqualInt,
            _ => Invalid(),
        };
        if (operandType == typeof(double)) return operation switch
        {
            "+" => CoflowRegisterOpCode.AddFloat, "-" => CoflowRegisterOpCode.SubtractFloat,
            "*" => CoflowRegisterOpCode.MultiplyFloat, "/" => CoflowRegisterOpCode.DivideFloat,
            "**" => CoflowRegisterOpCode.PowerFloat, "<" => CoflowRegisterOpCode.LessFloat,
            "<=" => CoflowRegisterOpCode.LessOrEqualFloat, ">" => CoflowRegisterOpCode.GreaterFloat,
            ">=" => CoflowRegisterOpCode.GreaterOrEqualFloat, _ => Invalid(),
        };
        if (operandType == typeof(string)) return operation switch
        {
            "+" => CoflowRegisterOpCode.AddString, "<" => CoflowRegisterOpCode.LessString,
            "<=" => CoflowRegisterOpCode.LessOrEqualString, ">" => CoflowRegisterOpCode.GreaterString,
            ">=" => CoflowRegisterOpCode.GreaterOrEqualString, _ => Invalid(),
        };
        return Invalid();

        CoflowRegisterOpCode Invalid() => throw new InvalidOperationException(
            $"Typed binary operation `{operation}` has unsupported operand `{operandType}`.");
    }

    private static CoflowValueRegister Register(
        CoflowVirtualValue? value,
        CoflowValueRegister[] registers) => value is { } target
        ? registers[target.Index]
        : throw new InvalidOperationException("A CFG value instruction has no target.");

    private static int Scalar(
        CoflowValueRegister value,
        CoflowRegisterKind? expected,
        CoflowVirtualInstruction instruction)
    {
        if (expected is null) throw Invalid(instruction, "uses an absent operand");
        var scalar = value.Scalar;
        if (scalar.Kind != expected) throw Invalid(instruction, "operand register kind does not match its type");
        return scalar.Index;
    }

    private static int TerminatorSize(CoflowBlockTerminator terminator) => terminator switch
    {
        CoflowBlockTerminator.Branch or CoflowBlockTerminator.Propagate => 2,
        CoflowBlockTerminator.Jump or CoflowBlockTerminator.Return or
            CoflowBlockTerminator.DirectTailCall or CoflowBlockTerminator.IndirectTailCall => 1,
        _ => throw new InvalidOperationException($"CFG terminator `{terminator.GetType().Name}` is not lowered yet."),
    };

    private static void RequireInputCount(CoflowVirtualInstruction instruction, int expected)
    {
        if (instruction.Inputs.Length != expected)
            throw Invalid(instruction, $"requires {expected} inputs");
    }

    private static InvalidOperationException Invalid(CoflowVirtualInstruction instruction, string message) =>
        new($"CFG operation `{instruction.Operation.GetType().Name}` {message}.");

    private static CoflowValueRegister[] AllocateWindow(
        IReadOnlyList<Type> types,
        int integerBase,
        int floatBase,
        int referenceBase)
    {
        var result = new CoflowValueRegister[types.Count];
        for (var index = 0; index < result.Length; index++)
        {
            var shape = CoflowValueShape.Of(types[index]);
            result[index] = new CoflowValueRegister(shape, integerBase, floatBase, referenceBase);
            integerBase += shape.IntegerCount;
            floatBase += shape.FloatCount;
            referenceBase += shape.ReferenceCount;
        }
        return result;
    }

    private static CoflowRegisterCallSite BuildDirectCallSite(
        CoflowCallSite call,
        CoflowValueRegister[] sourceArguments,
        CoflowValueRegister result,
        CoflowProgramLinker linker,
        int outgoingIntegerBase,
        int outgoingFloatBase,
        int outgoingReferenceBase)
    {
        if (sourceArguments.Length != call.VmParameterTypes.Length)
            throw new InvalidOperationException("A CFG call argument count does not match its signature.");
        var windowArguments = AllocateWindow(
            call.VmParameterTypes,
            outgoingIntegerBase,
            outgoingFloatBase,
            outgoingReferenceBase);
        return new CoflowRegisterCallSite(
            linker.FunctionIndexes.TryGetValue(call.Identity, out var programIndex)
                ? programIndex
                : throw new CoflowProgramLinkException($"unknown function `{call.Identity}`"),
            call.Signature,
            call.VmParameterTypes,
            sourceArguments,
            windowArguments,
            Enumerable.Repeat(true, sourceArguments.Length).ToArray(),
            result,
            windowArguments.Any(value => value.Shape.IntegerCount != 0) ? outgoingIntegerBase : -1,
            windowArguments.Any(value => value.Shape.FloatCount != 0) ? outgoingFloatBase : -1,
            windowArguments.Any(value => value.Shape.ReferenceCount != 0) ? outgoingReferenceBase : -1);
    }

    internal static object? LinkSymbol(object? symbol, CoflowProgramLinker? linker) => symbol switch
    {
        CoflowFunctionReferenceTemplate function when linker is not null => function.Link(linker),
        CoflowRecordReferenceTemplate record when linker is not null => record.Link(linker),
        CoflowConstantReferenceTemplate constant when linker is not null => constant.Link(linker),
        _ => symbol,
    };
}
}
