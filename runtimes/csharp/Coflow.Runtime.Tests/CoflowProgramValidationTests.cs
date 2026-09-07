using Coflow.Runtime.CompilerServices;
using System;
using System.Linq;
using Xunit;

namespace Coflow.Runtime.Tests;

public sealed class CoflowProgramValidationTests
{
    [Fact]
    public void FinalInstructionSpecificationCoversEveryOpcode()
    {
        var codes = Enum.GetValues<CoflowRegisterOpCode>();
        Assert.Equal(codes, CoflowRegisterInstructionSpec.All);
        foreach (var code in codes)
        {
            Assert.True(CoflowRegisterInstructionSpec.IsKnown(code));
            _ = CoflowRegisterInstructionSpec.OperandKind(code, 0);
            _ = CoflowRegisterInstructionSpec.OperandKind(code, 1);
            _ = CoflowRegisterInstructionSpec.OperandKind(code, 2);
            _ = CoflowRegisterInstructionSpec.Descriptor(code);
            _ = CoflowRegisterInstructionSpec.ControlFlow(code);
        }
        Assert.False(CoflowRegisterInstructionSpec.IsKnown((CoflowRegisterOpCode)byte.MaxValue));
    }

    [Fact]
    public void FinalVerifierRejectsUnknownOpcodeAndInvalidSourceMap()
    {
        var valid = RegisterProgram(1L, typeof(long));
        var unknown = valid.Instructions.ToArray();
        unknown[0] = unknown[0] with { Code = (CoflowRegisterOpCode)byte.MaxValue };

        var opcodeError = Assert.Throws<InvalidOperationException>(() => Rebuild(valid, instructions: unknown));
        Assert.Contains("unknown opcode", opcodeError.Message, StringComparison.Ordinal);

        var mapError = Assert.Throws<InvalidOperationException>(() =>
            Rebuild(valid, instructionSpans: Array.Empty<CfdSpan?>()));
        Assert.Contains("source map length", mapError.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void FinalVerifierRejectsRegisterImmediateAndDescriptorIndexes()
    {
        var integer = PrimitiveIntegerProgram();
        var invalidRegister = integer.Instructions.ToArray();
        invalidRegister[0] = invalidRegister[0] with { A = integer.IntegerRegisterCount };
        var registerError = Assert.Throws<InvalidOperationException>(() =>
            Rebuild(integer, instructions: invalidRegister));
        Assert.Contains("operand A", registerError.Message, StringComparison.Ordinal);

        var invalidImmediate = integer.Instructions.ToArray();
        invalidImmediate[0] = invalidImmediate[0] with { B = integer.Immediates.Length };
        var immediateError = Assert.Throws<InvalidOperationException>(() =>
            Rebuild(integer, instructions: invalidImmediate));
        Assert.Contains("immediate", immediateError.Message, StringComparison.Ordinal);

        var reference = PrimitiveReferenceProgram();
        var invalidDescriptor = reference.Instructions.ToArray();
        invalidDescriptor[0] = invalidDescriptor[0] with { C = reference.Operations.References.Length };
        var descriptorError = Assert.Throws<InvalidOperationException>(() =>
            Rebuild(reference, instructions: invalidDescriptor));
        Assert.Contains("reference descriptor", descriptorError.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void FinalVerifierRejectsInvalidJumpAndReachableFallthrough()
    {
        var valid = RegisterProgram(1L, typeof(long));
        var invalidJump = new[]
        {
            new CoflowRegisterInstruction(CoflowRegisterOpCode.Jump, valid.Instructions.Length),
        };
        var jumpError = Assert.Throws<InvalidOperationException>(() =>
            Rebuild(valid, instructions: invalidJump, instructionSpans: new CfdSpan?[1]));
        Assert.Contains("jump target", jumpError.Message, StringComparison.Ordinal);

        var fallthrough = new[]
        {
            new CoflowRegisterInstruction(CoflowRegisterOpCode.Nop),
        };
        var fallthroughError = Assert.Throws<InvalidOperationException>(() =>
            Rebuild(valid, instructions: fallthrough, instructionSpans: new CfdSpan?[1]));
        Assert.Contains("falls through", fallthroughError.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void FinalVerifierRejectsReadBeforeAssignment()
    {
        var target = new CoflowValueRegister(CoflowValueShape.Of(typeof(long)), 0, 0, 0);
        var operations = new CoflowRegisterOperations
        {
            Targets = new[] { new CoflowRegisterTargetSite(target) },
        };
        var instructions = new[]
        {
            new CoflowRegisterInstruction(CoflowRegisterOpCode.MoveInteger, 0, 0),
            new CoflowRegisterInstruction(CoflowRegisterOpCode.Return, C: 0),
        };

        var error = Assert.Throws<InvalidOperationException>(() => new CoflowRegisterProgram(
            Array.Empty<CoflowValueRegister>(), instructions, new CfdSpan?[instructions.Length],
            Array.Empty<long>(), operations, 1, 0, 0));

        Assert.Contains("reads unassigned integer register 0", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void FinalVerifierRequiresAssignmentOnEveryIncomingBranch()
    {
        var condition = new CoflowValueRegister(CoflowValueShape.Of(typeof(bool)), 0, 0, 0);
        var result = new CoflowValueRegister(CoflowValueShape.Of(typeof(long)), 1, 0, 0);
        var operations = new CoflowRegisterOperations
        {
            Targets = new[] { new CoflowRegisterTargetSite(result) },
        };
        var instructions = new[]
        {
            new CoflowRegisterInstruction(CoflowRegisterOpCode.JumpIfFalse, 0, 3),
            new CoflowRegisterInstruction(CoflowRegisterOpCode.ConstantInteger, 1, 0),
            new CoflowRegisterInstruction(CoflowRegisterOpCode.Jump, 4),
            new CoflowRegisterInstruction(CoflowRegisterOpCode.Nop),
            new CoflowRegisterInstruction(CoflowRegisterOpCode.Return, C: 0),
        };

        var error = Assert.Throws<InvalidOperationException>(() => new CoflowRegisterProgram(
            new[] { condition }, instructions, new CfdSpan?[instructions.Length],
            new[] { 1L }, operations, 2, 0, 0));

        Assert.Contains("reads unassigned integer register 1", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void VerifiedProgramDoesNotRetainMutableConstructionArrays()
    {
        var target = new CoflowValueRegister(CoflowValueShape.Of(typeof(long)), 0, 0, 0);
        var targets = new[] { new CoflowRegisterTargetSite(target) };
        var instructions = new[]
        {
            new CoflowRegisterInstruction(CoflowRegisterOpCode.ConstantInteger, 0, 0),
            new CoflowRegisterInstruction(CoflowRegisterOpCode.Return, C: 0),
        };
        var spans = new CfdSpan?[] { new(1, 1, 1, 2), new(1, 2, 1, 3) };
        var immediates = new[] { 7L };
        var program = new CoflowRegisterProgram(
            Array.Empty<CoflowValueRegister>(), instructions, spans, immediates,
            new CoflowRegisterOperations { Targets = targets }, 1, 0, 0);

        instructions[0] = new CoflowRegisterInstruction((CoflowRegisterOpCode)byte.MaxValue);
        spans[0] = null;
        immediates[0] = 99;
        targets[0] = new CoflowRegisterTargetSite(
            new CoflowValueRegister(CoflowValueShape.Of(typeof(long)), 99, 0, 0));

        Assert.Equal(CoflowRegisterOpCode.ConstantInteger, program.Instructions[0].Code);
        Assert.NotNull(program.InstructionSpans[0]);
        Assert.Equal(7, program.Immediates[0]);
        Assert.Equal(0, program.Operations.Targets[0].Target.IntegerBase);
    }

    [Fact]
    public void FinalDescriptorsDoNotRetainNestedRegisterArrays()
    {
        var original = new CoflowValueRegister(CoflowValueShape.Of(typeof(long)), 0, 0, 0);
        var registers = new[] { original };
        var collection = new CoflowRegisterCollectionSite(registers, null, original);
        var closure = new CoflowRegisterClosureSite(null!, registers, original);
        var native = new CoflowNativeCallSite(null!, registers, original);

        registers[0] = original with { IntegerBase = 9 };

        Assert.Equal(0, collection.First[0].IntegerBase);
        Assert.Equal(0, closure.Captures[0].IntegerBase);
        Assert.Equal(0, native.Arguments[0].IntegerBase);
    }

    private static CoflowRegisterProgram RegisterProgram(object value, Type type)
    {
        var builder = new CoflowVirtualProgramBuilder(
            new CoflowFunctionIdentity("Validation", "test", "program"),
            "validation.cfd",
            null,
            Array.Empty<Type>(),
            type);
        var origin = new CoflowSourceOrigin("validation.cfd", null);
        var result = builder.Constant(type, value, origin);
        builder.Return(result, origin);
        return CoflowVirtualLowering.Lower(builder.Build());
    }

    private static CoflowRegisterProgram PrimitiveIntegerProgram()
    {
        var target = new CoflowValueRegister(CoflowValueShape.Of(typeof(long)), 0, 0, 0);
        var operations = new CoflowRegisterOperations
        {
            Targets = new[] { new CoflowRegisterTargetSite(target) },
        };
        var instructions = new[]
        {
            new CoflowRegisterInstruction(CoflowRegisterOpCode.ConstantInteger, 0, 0),
            new CoflowRegisterInstruction(CoflowRegisterOpCode.Return, C: 0),
        };
        return new CoflowRegisterProgram(
            Array.Empty<CoflowValueRegister>(), instructions, new CfdSpan?[instructions.Length],
            new[] { 1L }, operations, 1, 0, 0);
    }

    private static CoflowRegisterProgram PrimitiveReferenceProgram()
    {
        var target = new CoflowValueRegister(CoflowValueShape.Of(typeof(string)), 0, 0, 0);
        var operations = new CoflowRegisterOperations
        {
            References = new object?[] { "value" },
            Targets = new[] { new CoflowRegisterTargetSite(target) },
        };
        var instructions = new[]
        {
            new CoflowRegisterInstruction(CoflowRegisterOpCode.ConstantReference, 0, C: 0),
            new CoflowRegisterInstruction(CoflowRegisterOpCode.Return, C: 0),
        };
        return new CoflowRegisterProgram(
            Array.Empty<CoflowValueRegister>(), instructions, new CfdSpan?[instructions.Length],
            Array.Empty<long>(), operations, 0, 0, 1);
    }

    private static CoflowRegisterProgram Rebuild(
        CoflowRegisterProgram source,
        CoflowRegisterInstruction[]? instructions = null,
        CfdSpan?[]? instructionSpans = null) =>
        new(
            source.Parameters.ToArray(),
            instructions ?? source.Instructions.ToArray(),
            instructionSpans ?? source.InstructionSpans.ToArray(),
            source.Immediates.ToArray(),
            source.Operations,
            source.IntegerRegisterCount,
            source.FloatRegisterCount,
            source.ReferenceRegisterCount);
}
