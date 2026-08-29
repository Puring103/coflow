using CoflowRuntime;
using CoflowRuntime.Generated;
using System;
using System.Collections.Generic;
using System.Linq;
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

    [Fact]
    public void ExecutesCompositeArgumentsWithoutExplicitIrValueTypes()
    {
        var option = Program(
            new[]
            {
                new CoflowInstruction(CoflowOpCode.Argument),
                new CoflowInstruction(CoflowOpCode.Return),
            },
            Array.Empty<object?>(),
            typeof(Option<long>),
            parameterTypes: new[] { typeof(Option<long>) });
        var result = Program(
            new[]
            {
                new CoflowInstruction(CoflowOpCode.Argument),
                new CoflowInstruction(CoflowOpCode.Return),
            },
            Array.Empty<object?>(),
            typeof(Result<long, string>),
            parameterTypes: new[] { typeof(Result<long, string>) });

        Assert.Equal(7, CoflowVm.Execute<Option<long>, Option<long>>(
            option, Option<long>.Some(7)).Value);
        Assert.Equal("failure", CoflowVm.Execute<Result<long, string>, Result<long, string>>(
            result, Result<long, string>.Err("failure")).Error);
    }

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public void DirectCallWindowsPreserveMixedAndCompositeArguments(bool tail)
    {
        var parameterTypes = new[]
        {
            typeof(long), typeof(double), typeof(string), typeof(Option<long>),
        };
        var target = Program(
            new[]
            {
                new CoflowInstruction(CoflowOpCode.Argument, 0),
                new CoflowInstruction(CoflowOpCode.Argument, 1),
                new CoflowInstruction(CoflowOpCode.Argument, 2),
                new CoflowInstruction(CoflowOpCode.Argument, 3),
                new CoflowInstruction(CoflowOpCode.Native, 0, ValueType: typeof(long)),
                new CoflowInstruction(CoflowOpCode.Return),
            },
            new object?[] { new CoflowNativeCall(new Func<long, double, string, Option<long>, long>(
                static (integer, floating, text, optional) =>
                    integer + (long)floating + text.Length + optional.Value)) },
            typeof(long),
            parameterTypes: parameterTypes);
        var entry = Entry(parameterTypes, typeof(long), target);
        var caller = CallProgram(entry, parameterTypes, tail);

        Assert.Equal(28, CoflowVm.Execute<long, double, string, Option<long>, long>(
            caller, 5, 3.0, "four", Option<long>.Some(16)));
    }

    [Fact]
    public void DirectCallWindowSuppliesMixedArgumentsToHostFunction()
    {
        var parameterTypes = new[]
        {
            typeof(long), typeof(double), typeof(string), typeof(Option<long>),
        };
        var entry = Entry(parameterTypes, typeof(long), implementation: null);
        entry.ConfigureHost(new Func<long, double, string, Option<long>, long>(
            static (integer, floating, text, optional) =>
                integer + (long)floating + text.Length + optional.Value));
        var caller = CallProgram(entry, parameterTypes, tail: false);

        Assert.Equal(28, CoflowVm.Execute<long, double, string, Option<long>, long>(
            caller, 5, 3.0, "four", Option<long>.Some(16)));
    }

    public static IEnumerable<object[]> InvalidPrograms()
    {
        yield return Case("stack underflow",
            new[] { new CoflowInstruction(CoflowOpCode.Pop) });
        yield return Case("invalid argument index",
            new[] { new CoflowInstruction(CoflowOpCode.Argument, 1) });
        yield return Case("read before assignment",
            new[] { new CoflowInstruction(CoflowOpCode.Local), new CoflowInstruction(CoflowOpCode.Return) },
            localCount: 1);
        yield return Case("changes type",
            new[]
            {
                Constant(0, typeof(long)), new CoflowInstruction(CoflowOpCode.StoreLocal),
                Constant(1, typeof(double)), new CoflowInstruction(CoflowOpCode.StoreLocal),
                Constant(0, typeof(long)), new CoflowInstruction(CoflowOpCode.Return),
            },
            new object?[] { 1L, 1.0 }, localCount: 1);
        yield return Case("reads `System.Int64` as Reference",
            new[] { Constant(0, typeof(long)), new CoflowInstruction(CoflowOpCode.LoadField, 1) },
            new object?[] { 1L, new object() });
        yield return Case("cannot construct",
            new[] { Constant(0, typeof(long)), new CoflowInstruction(CoflowOpCode.MakeOptionSome, ValueType: typeof(Option<string>)) },
            new object?[] { 1L });
        yield return Case("payload type",
            new[] { Constant(0, typeof(Option<long>)), new CoflowInstruction(CoflowOpCode.ReadFirstPayload, ValueType: typeof(string)) },
            new object?[] { Option<long>.None });
        yield return Case("incompatible propagation layouts",
            new[] { Constant(0, typeof(Result<long, string>)), new CoflowInstruction(CoflowOpCode.Propagate, ValueType: typeof(long)) },
            new object?[] { Result<long, string>.Ok(1) }, typeof(Result<long, Exception>));
        yield return Case("creates None with non-Option",
            new[] { new CoflowInstruction(CoflowOpCode.MakeOptionNone, ValueType: typeof(long)) });
        yield return Case("reads a tag",
            new[] { Constant(0, typeof(long)), new CoflowInstruction(CoflowOpCode.ReadValueTag) },
            new object?[] { 1L });
        yield return Case("reinterprets incompatible layouts",
            new[] { Constant(0, typeof(string)), new CoflowInstruction(CoflowOpCode.Reinterpret, ValueType: typeof(long)) },
            new object?[] { "value" });
        yield return Case("reads `System.Double` as Integer",
            new[] { Constant(0, typeof(double)), new CoflowInstruction(CoflowOpCode.ConvertIntToFloat) },
            new object?[] { 1.0 });
        yield return Case("invalid Type descriptor",
            new[] { Constant(0, typeof(string)), new CoflowInstruction(CoflowOpCode.IsType, 1) },
            new object?[] { "value", 42L });
        yield return Case("native argument 0",
            new[]
            {
                Constant(0, typeof(double)),
                new CoflowInstruction(CoflowOpCode.Native, 1, ValueType: typeof(long)),
            },
            new object?[] { 1.0, new CoflowNativeCall(new Func<long, long>(value => value)) });
        yield return Case("stack underflow",
            new[] { new CoflowInstruction(CoflowOpCode.JumpIfFalseKeep, 1) });
        yield return Case("incompatible stack layout",
            new[]
            {
                Constant(0, typeof(bool)),
                new CoflowInstruction(CoflowOpCode.JumpIfFalse, 4),
                Constant(1, typeof(long)),
                new CoflowInstruction(CoflowOpCode.Jump, 5),
                Constant(2, typeof(double)),
                new CoflowInstruction(CoflowOpCode.Return),
            },
            new object?[] { true, 1L, 1.0 });
        yield return Case("unknown opcode",
            new[] { new CoflowInstruction((CoflowOpCode)byte.MaxValue) });
    }

    [Theory]
    [MemberData(nameof(InvalidPrograms))]
    public void RejectsInvalidOpcodeStackDescriptorAndLayoutCombinations(object data)
    {
        var invalid = Assert.IsType<InvalidProgram>(data);
        var error = Assert.Throws<InvalidOperationException>(() =>
            Program(invalid.Instructions, invalid.Constants, invalid.ReturnType, invalid.LocalCount));

        Assert.Contains(invalid.Expected, error.Message, StringComparison.Ordinal);
    }

    private static object[] Case(
        string expected,
        CoflowInstruction[] instructions,
        object?[]? constants = null,
        Type? returnType = null,
        int localCount = 0) =>
        new object[] { new InvalidProgram(
            expected, instructions, constants ?? Array.Empty<object?>(), returnType ?? typeof(Unit), localCount) };

    private static CoflowInstruction Constant(int index, Type type) =>
        new(CoflowOpCode.Constant, index, ValueType: type);

    private static CoflowProgram CallProgram(
        CoflowFunctionEntry entry,
        Type[] parameterTypes,
        bool tail)
    {
        var instructions = parameterTypes.Select((_, index) =>
                new CoflowInstruction(CoflowOpCode.Argument, index))
            .Append(new CoflowInstruction(
                tail ? CoflowOpCode.TailCall : CoflowOpCode.Call,
                0,
                ValueType: typeof(long)))
            .ToList();
        if (!tail) instructions.Add(new CoflowInstruction(CoflowOpCode.Return));
        return Program(
            instructions.ToArray(),
            new object?[] { new CoflowCallSite(entry, parameterTypes.Length) },
            typeof(long),
            parameterTypes: parameterTypes);
    }

    private static CoflowFunctionEntry Entry(
        Type[] parameterTypes,
        Type returnType,
        CoflowProgram? implementation)
    {
        var entry = new CoflowFunctionEntry(
            new CoflowFunctionIdentity("Validation", "test", "target"),
            new CoflowFunctionSignature(returnType, parameterTypes),
            null,
            CfdNameResolver.Root,
            "validation.cfd",
            null);
        entry.PublishCompiled(implementation);
        return entry;
    }

    private sealed record InvalidProgram(
        string Expected,
        CoflowInstruction[] Instructions,
        object?[] Constants,
        Type ReturnType,
        int LocalCount);

    private static CoflowProgram Program(
        CoflowInstruction[] instructions,
        object?[] constants,
        Type returnType,
        int localCount = 0,
        Type[]? parameterTypes = null) => new(
            new CoflowFunctionIdentity("Validation", "test", "program"),
            "validation.cfd",
            null,
            instructions,
            new CfdSpan?[instructions.Length],
            constants,
            parameterTypes ?? Array.Empty<Type>(),
            returnType,
            localCount);
}
