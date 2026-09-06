using Coflow.Runtime;
using Coflow.Runtime.CompilerServices;
using System;
using System.Collections.Generic;
using System.Linq;
using Xunit;

namespace Coflow.Runtime.Tests;

public sealed class CoflowProgramValidationTests
{
    static CoflowProgramValidationTests()
    {
        CoflowValueLayout.RegisterOption<long>();
        CoflowValueLayout.RegisterOption<string>();
        CoflowValueLayout.RegisterResult<long, string>();
        CoflowValueLayout.RegisterResult<long, double>();
    }

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

        Assert.Equal(7, CoflowVm.ExecuteRaw<Option<long>, Option<long>>(
            option, Option<long>.Some(7)).Value);
        Assert.Equal("failure", CoflowVm.ExecuteRaw<Result<long, string>, Result<long, string>>(
            result, Result<long, string>.Err("failure")).Error);
    }

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public void DirectCallWindowsAllocateMixedAndCompositeArgumentShapes(bool tail)
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
        entry.AssignProgramIndex(0);
        var caller = CallProgram(entry, parameterTypes, tail);
        var site = Assert.Single(caller.RegisterProgram.Operations.Calls);
        Assert.Equal(0, site.ProgramIndex);
        Assert.Equal(parameterTypes, site.Arguments.Select(argument => argument.Shape.Type));
    }

    [Fact]
    public void HostCallWindowUsesTheDeclaredMixedArgumentShapes()
    {
        var parameterTypes = new[]
        {
            typeof(long), typeof(double), typeof(string), typeof(Option<long>),
        };
        var entry = Entry(parameterTypes, typeof(long), implementation: null);
        entry.ConfigureHost(new Func<long, double, string, Option<long>, long>(
            static (integer, floating, text, optional) =>
                integer + (long)floating + text.Length + optional.Value));
        entry.AssignProgramIndex(0);
        var caller = CallProgram(entry, parameterTypes, tail: false);
        var site = Assert.Single(caller.RegisterProgram.Operations.Calls);
        Assert.Null(entry.CompiledProgram);
        Assert.Equal(parameterTypes, site.Arguments.Select(argument => argument.Shape.Type));
    }

    [Fact]
    public void ArrayLiteralLowersDirectlyIntoTheCollectionArena()
    {
        var program = Program(
            new[]
            {
                new CoflowInstruction(CoflowOpCode.Argument, 0),
                new CoflowInstruction(CoflowOpCode.MakeArray, 1,
                    typeof(IReadOnlyList<long>)),
                new CoflowInstruction(CoflowOpCode.Return),
            },
            Array.Empty<object?>(),
            typeof(IReadOnlyList<long>),
            parameterTypes: new[] { typeof(long) });

        Assert.Contains(program.RegisterProgram.Instructions,
            instruction => instruction.Code == CoflowRegisterOpCode.MakeArray);
        Assert.DoesNotContain(program.RegisterProgram.Instructions,
            instruction => instruction.Code == CoflowRegisterOpCode.Native);
        Assert.Equal(new long[] { 7 },
            CoflowVm.ExecuteRaw<long, IReadOnlyList<long>>(program, 7));
    }

    [Fact]
    public void DictionaryLiteralLowersDirectlyIntoTheCollectionArena()
    {
        var program = Program(
            new[]
            {
                Constant(0, typeof(string)),
                Constant(1, typeof(long)),
                new CoflowInstruction(CoflowOpCode.MakeDictionary, 1,
                    typeof(IReadOnlyDictionary<string, long>)),
                new CoflowInstruction(CoflowOpCode.Return),
            },
            new object?[] { "key", 9L },
            typeof(IReadOnlyDictionary<string, long>),
            parameterTypes: new[] { typeof(long) });

        Assert.Contains(program.RegisterProgram.Instructions,
            instruction => instruction.Code == CoflowRegisterOpCode.MakeDictionary);
        Assert.DoesNotContain(program.RegisterProgram.Instructions,
            instruction => instruction.Code == CoflowRegisterOpCode.Native);
        var result = CoflowVm.ExecuteRaw<long, IReadOnlyDictionary<string, long>>(program, 0);
        Assert.Equal(9, result["key"]);
    }

    [Theory]
    [InlineData(1L, true, 20L)]
    [InlineData(-1L, false, 0L)]
    [InlineData(2L, false, 0L)]
    public void ArrayIndexReadsTheArenaWithoutANativeCall(long index, bool hasValue, long expected)
    {
        var program = Program(
            new[]
            {
                Constant(0, typeof(long)),
                Constant(1, typeof(long)),
                new CoflowInstruction(CoflowOpCode.MakeArray, 2,
                    typeof(IReadOnlyList<long>)),
                Constant(2, typeof(long)),
                new CoflowInstruction(CoflowOpCode.ArrayIndex, ValueType: typeof(Option<long>)),
                new CoflowInstruction(CoflowOpCode.Return),
            },
            new object?[] { 10L, 20L, index },
            typeof(Option<long>),
            parameterTypes: new[] { typeof(long) });

        Assert.Contains(program.RegisterProgram.Instructions,
            instruction => instruction.Code == CoflowRegisterOpCode.ArrayIndex);
        Assert.DoesNotContain(program.RegisterProgram.Instructions,
            instruction => instruction.Code == CoflowRegisterOpCode.Native);
        var result = CoflowVm.ExecuteRaw<long, Option<long>>(program, 0);
        Assert.Equal(hasValue, result.HasValue);
        if (hasValue) Assert.Equal(expected, result.Value);
    }

    [Theory]
    [InlineData("second", true, 20L)]
    [InlineData("missing", false, 0L)]
    public void DictionaryIndexReadsTheArenaWithoutANativeCall(
        string key,
        bool hasValue,
        long expected)
    {
        var program = Program(
            new[]
            {
                Constant(0, typeof(string)), Constant(1, typeof(long)),
                Constant(2, typeof(string)), Constant(3, typeof(long)),
                new CoflowInstruction(CoflowOpCode.MakeDictionary, 2,
                    typeof(IReadOnlyDictionary<string, long>)),
                Constant(4, typeof(string)),
                new CoflowInstruction(CoflowOpCode.DictionaryIndex,
                    ValueType: typeof(Option<long>)),
                new CoflowInstruction(CoflowOpCode.Return),
            },
            new object?[] { "first", 10L, "second", 20L, key },
            typeof(Option<long>),
            parameterTypes: new[] { typeof(long) });

        Assert.Contains(program.RegisterProgram.Instructions,
            instruction => instruction.Code == CoflowRegisterOpCode.DictionaryIndex);
        Assert.DoesNotContain(program.RegisterProgram.Instructions,
            instruction => instruction.Code == CoflowRegisterOpCode.Native);
        var result = CoflowVm.ExecuteRaw<long, Option<long>>(program, 0);
        Assert.Equal(hasValue, result.HasValue);
        if (hasValue) Assert.Equal(expected, result.Value);
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
        yield return Case("requires a reference or struct receiver",
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
            new object?[] { Result<long, string>.Ok(1) }, typeof(Result<long, double>));
        yield return Case("creates None with non-Option",
            new[] { new CoflowInstruction(CoflowOpCode.MakeOptionNone, ValueType: typeof(long)) });
        yield return Case("reads a tag",
            new[] { Constant(0, typeof(long)), new CoflowInstruction(CoflowOpCode.ReadValueTag) },
            new object?[] { 1L });
        yield return Case("reinterprets incompatible layouts",
            new[] { Constant(0, typeof(string)), new CoflowInstruction(CoflowOpCode.Reinterpret, ValueType: typeof(long)) },
            new object?[] { "value" });
        yield return Case("reinterprets a collection handle",
            new[]
            {
                Constant(0, typeof(long)),
                new CoflowInstruction(CoflowOpCode.MakeArray, 1,
                    ValueType: typeof(IReadOnlyList<long>)),
                new CoflowInstruction(CoflowOpCode.Reinterpret, ValueType: typeof(long)),
            },
            new object?[] { 1L });
        yield return Case("reinterprets a collection handle",
            new[]
            {
                Constant(0, typeof(long)),
                new CoflowInstruction(CoflowOpCode.Reinterpret, ValueType: typeof(IReadOnlyList<long>)),
            },
            new object?[] { 1L });
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
            new object?[] { CoflowCallSite.From(entry, parameterTypes.Length) },
            typeof(long),
            parameterTypes: parameterTypes,
            functions: new Dictionary<CoflowFunctionIdentity, CoflowFunctionEntry>
            {
                [entry.Identity] = entry,
            });
    }

    private static CoflowFunctionEntry Entry(
        Type[] parameterTypes,
        Type returnType,
        CoflowProgram? implementation)
    {
        var entry = new CoflowFunctionEntry(
            new CoflowFunctionIdentity("Validation", "test", "target"),
            new CoflowFunctionSignature(returnType, parameterTypes),
            typeof(object),
            null,
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
        Type[]? parameterTypes = null,
        IReadOnlyDictionary<CoflowFunctionIdentity, CoflowFunctionEntry>? functions = null) =>
        new CoflowProgramTemplate(
                new CoflowFunctionIdentity("Validation", "test", "program"),
                "validation.cfd",
                null,
                instructions,
                new CfdSpan?[instructions.Length],
                constants,
                parameterTypes ?? Array.Empty<Type>(),
                returnType,
                localCount)
            .Link(new CoflowProgramLinker(
                functions ?? new Dictionary<CoflowFunctionIdentity, CoflowFunctionEntry>(),
                new CoflowRecordCatalog(),
                new CfdLoadContext(Array.Empty<CfdDocument>())));
}
