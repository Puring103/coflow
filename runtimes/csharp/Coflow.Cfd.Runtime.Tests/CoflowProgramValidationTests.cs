using CoflowRuntime;
using System;
using Xunit;

namespace CoflowRuntime.Tests;

public sealed class CoflowProgramValidationTests
{
    [Fact]
    public void RejectsJumpOutsideInstructionBoundaries()
    {
        var error = Assert.Throws<InvalidOperationException>(() => Program(
            new[] { new CoflowInstruction(CoflowOpCode.Jump, 4) },
            Array.Empty<object?>(),
            typeof(Unit)));
        Assert.Contains("jump target", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void RejectsOperationWithWrongDescriptorType()
    {
        var error = Assert.Throws<InvalidOperationException>(() => Program(
            new[]
            {
                new CoflowInstruction(CoflowOpCode.Native, 0, ValueType: typeof(long)),
                new CoflowInstruction(CoflowOpCode.Return),
            },
            new object?[] { 42L },
            typeof(long)));
        Assert.Contains("CoflowNativeCall descriptor", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void RejectsReturnTypeMismatch()
    {
        var error = Assert.Throws<InvalidOperationException>(() => Program(
            new[]
            {
                new CoflowInstruction(CoflowOpCode.Constant, 0, ValueType: typeof(long)),
                new CoflowInstruction(CoflowOpCode.Return),
            },
            new object?[] { 1L },
            typeof(double)));
        Assert.Contains("return type", error.Message, StringComparison.Ordinal);
    }

    private static CoflowProgram Program(
        CoflowInstruction[] instructions,
        object?[] constants,
        Type returnType) => new(
            new CoflowFunctionIdentity("Validation", "test", "program"),
            "validation.cfd",
            null,
            instructions,
            new CfdSpan?[instructions.Length],
            constants,
            Array.Empty<Type>(),
            returnType,
            0);
}
