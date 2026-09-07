using System;
using System.Collections.Generic;
using System.Linq;
using Coflow.Runtime.CompilerServices;
using Xunit;

namespace Coflow.Runtime.Tests;

public sealed class CoflowVirtualProgramTests
{
    [Fact]
    public void SyntaxParserProducesSchemaFreeFunctionDeclaration()
    {
        var syntax = CoflowFunctionFrontend.FunctionSyntaxParser.Parse(
            "fn(value: Option<int>, items: [string]) -> int { value; 1 }");

        Assert.Equal(new[] { "value", "items" }, syntax.Parameters.Select(value => value.Name));
        Assert.Equal(new[] { "Option<int>", "[string]" }, syntax.Parameters.Select(value => value.TypeName));
        Assert.Equal("int", syntax.ReturnTypeName);
        Assert.Equal(CoflowFunctionFrontend.TokenKind.RightBrace,
            syntax.BodyTokens[syntax.BodyTokens.Count - 2].Kind);
        Assert.Equal(CoflowFunctionFrontend.TokenKind.End,
            syntax.BodyTokens[syntax.BodyTokens.Count - 1].Kind);
    }

    [Fact]
    public void SyntaxParserOwnsNestedTypeNameGrammar()
    {
        var syntax = CoflowFunctionFrontend.FunctionSyntaxParser.Parse(
            "fn(callback: fn([int],{string:Option<float>})->Result<int,string>) -> () { () }");

        Assert.Equal("fn([int],{string:Option<float>})->Result<int,string>",
            Assert.Single(syntax.Parameters).TypeName);
        Assert.Equal("()", syntax.ReturnTypeName);
    }

    [Fact]
    public void BinderResolvesHeaderAgainstDeclaredSignature()
    {
        var syntax = CoflowFunctionFrontend.FunctionSyntaxParser.Parse(
            "fn(value: int, names: [string]) -> bool { true }");
        var entry = new CoflowFunctionEntry(
            new CoflowFunctionIdentity("Validation", "test", "bind"),
            new CoflowFunctionSignature(typeof(bool),
                new[] { typeof(long), typeof(IReadOnlyList<string>) }),
            typeof(object), null, "test.cfd", null);

        var bound = CoflowFunctionFrontend.FunctionBinder.Bind(
            entry, syntax, new CoflowCompilerCatalog(new EmptySchema()));

        Assert.Equal(new[] { typeof(long), typeof(IReadOnlyList<string>) },
            bound.Parameters.Select(value => value.Type));
        Assert.Same(entry, bound.Entry);
    }

    [Fact]
    public void VirtualModelDoesNotStoreFinalOpcodes()
    {
        var virtualTypes = typeof(CoflowVirtualOperation).Assembly.GetTypes()
            .Where(type => type == typeof(CoflowVirtualInstruction) ||
                type == typeof(CoflowBlockTerminator) ||
                type.IsSubclassOf(typeof(CoflowVirtualOperation)) ||
                type.IsSubclassOf(typeof(CoflowBlockTerminator)));

        Assert.DoesNotContain(virtualTypes.SelectMany(type => type.GetFields(
                System.Reflection.BindingFlags.Instance |
                System.Reflection.BindingFlags.Public |
                System.Reflection.BindingFlags.NonPublic)),
            field => field.FieldType == typeof(CoflowRegisterOpCode));
    }

    [Fact]
    public void TypedFunctionAndCfgLowererAreIndependentFromParser()
    {
        var frontend = typeof(CoflowFunctionFrontend);
        var typedFunction = frontend.GetNestedType("TypedFunction",
            System.Reflection.BindingFlags.NonPublic);
        var lowerer = frontend.GetNestedType("TypedCfgLowerer",
            System.Reflection.BindingFlags.NonPublic);

        Assert.NotNull(typedFunction);
        Assert.NotNull(lowerer);
        Assert.Same(frontend, typedFunction!.DeclaringType);
        Assert.Same(frontend, lowerer!.DeclaringType);
        Assert.DoesNotContain(lowerer.GetFields(
                System.Reflection.BindingFlags.Instance |
                System.Reflection.BindingFlags.Public |
                System.Reflection.BindingFlags.NonPublic),
            field => field.FieldType.Name == "FunctionParser");
    }

    [Fact]
    public void TypedExpressionModelDoesNotDependOnParser()
    {
        var frontend = typeof(CoflowFunctionFrontend);
        var expression = frontend.GetNestedType("Expr",
            System.Reflection.BindingFlags.NonPublic)!;
        var expressionTypes = frontend.GetNestedTypes(System.Reflection.BindingFlags.NonPublic)
            .Where(type => type == expression || expression.IsAssignableFrom(type));

        Assert.DoesNotContain(expressionTypes.SelectMany(type => type.GetFields(
                System.Reflection.BindingFlags.Instance |
                System.Reflection.BindingFlags.Public |
                System.Reflection.BindingFlags.NonPublic)),
            field => field.FieldType.Name == "FunctionParser");
        Assert.DoesNotContain(expressionTypes.SelectMany(type => type.GetMethods(
                System.Reflection.BindingFlags.Instance |
                System.Reflection.BindingFlags.Static |
                System.Reflection.BindingFlags.Public |
                System.Reflection.BindingFlags.NonPublic)),
            method => method.GetParameters().Any(parameter =>
                parameter.ParameterType.Name == "FunctionParser"));
    }

    [Fact]
    public void BuilderCreatesTypedBlocksWithExplicitOperands()
    {
        var builder = Builder(new[] { typeof(long) }, typeof(long));
        var origin = new CoflowSourceOrigin("test.cfd", null);
        var one = builder.Constant(typeof(long), 1L, origin);
        var sum = builder.Binary("+", typeof(long), builder.Parameters[0], one, origin);
        builder.Return(sum, origin);

        var program = builder.Build();

        Assert.Equal(typeof(long), sum.Type);
        Assert.Equal(new[] { builder.Parameters[0], one }, program.Blocks[0].Instructions[1].Inputs);
        Assert.IsType<CoflowVirtualOperation.Binary>(program.Blocks[0].Instructions[1].Operation);
        Assert.IsType<CoflowBlockTerminator.Return>(program.Blocks[0].Terminator);
    }

    [Fact]
    public void BuilderRejectsForeignValuesAndInstructionsAfterTermination()
    {
        var first = Builder(Array.Empty<Type>(), typeof(long));
        var second = Builder(new[] { typeof(long) }, typeof(long));
        var origin = new CoflowSourceOrigin("test.cfd", null);

        Assert.Throws<InvalidOperationException>(() => first.Emit(
            new CoflowVirtualOperation.Move(), typeof(long),
            new[] { second.Parameters[0] }, origin));

        var value = first.Constant(typeof(long), 1L, origin);
        first.Return(value, origin);
        Assert.Throws<InvalidOperationException>(() => first.Constant(typeof(long), 2L, origin));
    }

    [Fact]
    public void BuilderRequiresEveryBlockToTerminate()
    {
        var builder = Builder(Array.Empty<Type>(), typeof(Unit));
        builder.CreateBlock();

        var error = Assert.Throws<InvalidOperationException>(builder.Build);

        Assert.Contains("must have a terminator", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void BuilderKeepsAnonymousAndSourceLocalsInSeparateNamespaces()
    {
        var builder = Builder(Array.Empty<Type>(), typeof(long));

        var temporary = builder.CreateLocal(typeof(string));
        var source = builder.Local(0, typeof(long));

        Assert.NotEqual(temporary, source);
        var origin = new CoflowSourceOrigin("test.cfd", null);
        var value = builder.Constant(typeof(long), 1L, origin);
        builder.Return(value, origin);
        Assert.Equal(2, builder.Build().Locals.Length);
    }

    [Fact]
    public void BuilderRemovesUnreachableBlocksAndRelocatesTargets()
    {
        var builder = Builder(new[] { typeof(bool) }, typeof(long));
        var origin = new CoflowSourceOrigin("test.cfd", null);
        var reachable = builder.CreateBlock();
        var unreachable = builder.CreateBlock();
        builder.Jump(reachable, origin);

        builder.Enter(reachable);
        var value = builder.Constant(typeof(long), 1L, origin);
        builder.Return(value, origin);

        builder.Enter(unreachable);
        builder.Jump(unreachable, origin);

        var program = builder.Build();

        Assert.Equal(2, program.Blocks.Length);
        var jump = Assert.IsType<CoflowBlockTerminator.Jump>(program.Blocks[0].Terminator);
        Assert.Equal(1, jump.TargetBlock);
    }

    [Fact]
    public void LoweringMapsExplicitOperandsToFinalRegisters()
    {
        var builder = Builder(new[] { typeof(long) }, typeof(long));
        var origin = new CoflowSourceOrigin("test.cfd", null);
        var one = builder.Constant(typeof(long), 1L, origin);
        var sum = builder.Binary("+", typeof(long), builder.Parameters[0], one, origin);
        builder.Return(sum, origin);

        var lowered = CoflowVirtualLowering.Lower(builder.Build());

        Assert.Equal(CoflowRegisterOpCode.ConstantValue, lowered.Instructions[0].Code);
        Assert.Equal(CoflowRegisterOpCode.AddInt, lowered.Instructions[1].Code);
        Assert.Equal(lowered.Parameters[0].IntegerBase, lowered.Instructions[1].B);
        Assert.Equal(CoflowRegisterOpCode.Return, lowered.Instructions[2].Code);
    }

    [Fact]
    public void LoweringReusesRegistersAccordingToPeakLiveness()
    {
        var builder = Builder(new[] { typeof(long) }, typeof(long));
        var origin = new CoflowSourceOrigin("test.cfd", null);
        var current = builder.Parameters[0];
        for (var index = 0; index < 100; index++)
        {
            var one = builder.Constant(typeof(long), 1L, origin);
            current = builder.Binary("+", typeof(long), current, one, origin);
        }
        builder.Return(current, origin);

        var first = CoflowVirtualLowering.Lower(builder.Build());

        Assert.True(first.IntegerRegisterCount <= 4,
            $"Expected peak-live allocation, got {first.IntegerRegisterCount} integer registers.");
        Assert.Equal(201, first.Instructions.Length);
    }

    [Fact]
    public void LoweringClearsDeadReferenceLanesAfterTheirLastUse()
    {
        var builder = Builder(Array.Empty<Type>(), typeof(string));
        var origin = new CoflowSourceOrigin("test.cfd", null);
        var left = builder.Constant(typeof(string), "left", origin);
        var right = builder.Constant(typeof(string), "right", origin);
        var result = builder.Binary("+", typeof(string), left, right, origin);
        builder.Return(result, origin);

        var program = CoflowVirtualLowering.Lower(builder.Build());

        Assert.Equal(2, program.Instructions.Count(
            instruction => instruction.Code == CoflowRegisterOpCode.ClearReference));
    }

    [Fact]
    public void LoweringRelocatesExplicitBranchTargets()
    {
        var builder = Builder(new[] { typeof(bool) }, typeof(long));
        var origin = new CoflowSourceOrigin("test.cfd", null);
        var whenTrue = builder.CreateBlock();
        var whenFalse = builder.CreateBlock();
        builder.Branch(builder.Parameters[0], whenTrue, whenFalse, origin);

        builder.Enter(whenTrue);
        var one = builder.Constant(typeof(long), 1L, origin);
        builder.Return(one, origin);

        builder.Enter(whenFalse);
        var zero = builder.Constant(typeof(long), 0L, origin);
        builder.Return(zero, origin);

        var lowered = CoflowVirtualLowering.Lower(builder.Build());

        Assert.Equal(CoflowRegisterOpCode.JumpIfFalse, lowered.Instructions[0].Code);
        Assert.Equal(4, lowered.Instructions[0].B);
        Assert.Equal(CoflowRegisterOpCode.Jump, lowered.Instructions[1].Code);
        Assert.Equal(2, lowered.Instructions[1].A);
    }

    [Fact]
    public void FailedExecutionDoesNotRetainReferenceArgumentsInTheSessionPool()
    {
        var reference = ExecuteFailingProgramWithReferenceArgument();

        for (var attempt = 0; attempt < 5 && reference.IsAlive; attempt++)
        {
            GC.Collect();
            GC.WaitForPendingFinalizers();
            GC.Collect();
        }

        Assert.False(reference.IsAlive);
    }

    [Fact]
    public void ProgramTemplateLinksAndExecutesTypedCfg()
    {
        var builder = Builder(new[] { typeof(long) }, typeof(long));
        var origin = new CoflowSourceOrigin("test.cfd", null);
        var one = builder.Constant(typeof(long), 1L, origin);
        var sum = builder.Binary("+", typeof(long), builder.Parameters[0], one, origin);
        builder.Return(sum, origin);
        var template = new CoflowProgramTemplate(builder.Build());
        var program = template.Link(new CoflowProgramLinker(
            new Dictionary<CoflowFunctionIdentity, CoflowFunctionEntry>(),
            new CoflowRecordCatalog(),
            new CfdLoadContext(Array.Empty<CfdDocument>())));

        var result = CoflowVm.ExecuteRaw<long, long>(program, 41L);

        Assert.Equal(42L, result);
    }

    [Fact]
    public void DirectCallResolvesSymbolAndAllocatesOutgoingWindowAtLinkTime()
    {
        var identity = new CoflowFunctionIdentity("Validation", "test", "callee");
        var entry = new CoflowFunctionEntry(
            identity,
            new CoflowFunctionSignature(typeof(long), new[] { typeof(long) }),
            typeof(object),
            null,
            "test.cfd",
            null);
        entry.AssignProgramIndex(3);
        var builder = Builder(new[] { typeof(long) }, typeof(long));
        var origin = new CoflowSourceOrigin("test.cfd", null);
        var result = builder.Emit(
            new CoflowVirtualOperation.DirectCall(CoflowCallSite.From(entry, 1)),
            typeof(long), new[] { builder.Parameters[0] }, origin);
        builder.Return(result, origin);
        var linker = new CoflowProgramLinker(
            new Dictionary<CoflowFunctionIdentity, CoflowFunctionEntry> { [identity] = entry },
            new CoflowRecordCatalog(),
            new CfdLoadContext(Array.Empty<CfdDocument>()));

        var program = new CoflowProgramTemplate(builder.Build()).Link(linker).RegisterProgram;
        var call = Assert.Single(program.Operations.Calls);

        Assert.Equal(3, call.ProgramIndex);
        Assert.NotEqual(call.SourceArguments[0].IntegerBase, call.Arguments[0].IntegerBase);
        Assert.Equal(call.IntegerWindowBase, call.Arguments[0].IntegerBase);
    }

    [Fact]
    public void PropagateContinuesWithSomeAndReturnsNone()
    {
        var runtimeBuilder = new CoflowSchemaRuntimeBuilder();
        runtimeBuilder.RegisterOption<long>();
        using var runtimeScope = CoflowSchemaRuntimeContext.Enter(runtimeBuilder.Build());
        var optionType = typeof(Option<long>);
        var builder = Builder(new[] { optionType }, optionType);
        var origin = new CoflowSourceOrigin("test.cfd", null);
        var continuation = builder.CreateBlock();
        var payload = builder.Propagate(builder.Parameters[0], typeof(long), continuation, origin);
        builder.Enter(continuation);
        var result = builder.Emit(
            new CoflowVirtualOperation.MakeOptionSome(), optionType, new[] { payload }, origin);
        builder.Return(result, origin);
        var program = new CoflowProgramTemplate(builder.Build()).Link(new CoflowProgramLinker(
            new Dictionary<CoflowFunctionIdentity, CoflowFunctionEntry>(),
            new CoflowRecordCatalog(),
            new CfdLoadContext(Array.Empty<CfdDocument>())));

        Assert.Equal(9L, CoflowVm.ExecuteRaw<Option<long>, Option<long>>(
            program, Option<long>.Some(9L)).Value);
        Assert.False(CoflowVm.ExecuteRaw<Option<long>, Option<long>>(
            program, Option<long>.None).HasValue);
        Assert.Contains(program.RegisterProgram.Instructions,
            instruction => instruction.Code == CoflowRegisterOpCode.Propagate);
    }

    private static CoflowVirtualProgramBuilder Builder(IReadOnlyList<Type> parameters, Type result) =>
        new(
            new CoflowFunctionIdentity("Validation", "test", "cfg"),
            "test.cfd",
            null,
            parameters,
            result);

    [System.Runtime.CompilerServices.MethodImpl(
        System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    private static WeakReference ExecuteFailingProgramWithReferenceArgument()
    {
        var builder = Builder(new[] { typeof(string) }, typeof(long));
        var origin = new CoflowSourceOrigin("test.cfd", null);
        var one = builder.Constant(typeof(long), 1L, origin);
        var zero = builder.Constant(typeof(long), 0L, origin);
        var quotient = builder.Binary("/", typeof(long), one, zero, origin);
        builder.Return(quotient, origin);
        var program = new CoflowProgramTemplate(builder.Build()).Link(new CoflowProgramLinker(
            new Dictionary<CoflowFunctionIdentity, CoflowFunctionEntry>(),
            new CoflowRecordCatalog(),
            new CfdLoadContext(Array.Empty<CfdDocument>())));
        var argument = new string('x', 256);
        var reference = new WeakReference(argument);

        var error = Assert.Throws<CoflowFaultException>(() =>
            CoflowVm.ExecuteRaw<string, long>(program, argument));
        Assert.IsType<DivideByZeroException>(error.InnerException);
        return reference;
    }

    private sealed class EmptySchema : ICoflowSchema
    {
        public CoflowSchemaRuntime Runtime { get; } = new CoflowSchemaRuntimeBuilder().Build();
        public IReadOnlyList<ICoflowTypeMetadata> Types { get; } = Array.Empty<ICoflowTypeMetadata>();
        public IReadOnlyList<ICoflowEnumMetadata> Enums { get; } = Array.Empty<ICoflowEnumMetadata>();
        public IReadOnlyList<CoflowConstant> Constants { get; } = Array.Empty<CoflowConstant>();
    }
}
