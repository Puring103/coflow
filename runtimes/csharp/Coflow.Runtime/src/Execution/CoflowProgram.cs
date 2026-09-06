using System;
using System.Collections.Generic;
using System.Linq;

namespace Coflow.Runtime.CompilerServices;

internal sealed class CoflowProgramLinker
{
    private readonly IReadOnlyDictionary<CoflowFunctionIdentity, CoflowFunctionEntry> _functions;
    private readonly CoflowRecordCatalog _records;
    private readonly CfdLoadContext _context;

    internal CoflowProgramLinker(
        IReadOnlyDictionary<CoflowFunctionIdentity, CoflowFunctionEntry> functions,
        CoflowRecordCatalog records,
        CfdLoadContext context)
    {
        _functions = functions ?? throw new ArgumentNullException(nameof(functions));
        _records = records ?? throw new ArgumentNullException(nameof(records));
        _context = context ?? throw new ArgumentNullException(nameof(context));
        FunctionIndexes = functions.ToDictionary(pair => pair.Key, pair => pair.Value.ProgramIndex);
    }

    internal IReadOnlyDictionary<CoflowFunctionIdentity, int> FunctionIndexes { get; }
    internal List<CoflowClosureTemplate> Closures { get; } = new();

    internal CoflowClosureTemplate RegisterClosure(CoflowClosureTemplate closure)
    {
        closure.AssignTargetIndex(Closures.Count);
        Closures.Add(closure);
        return closure;
    }

    internal CoflowFunctionEntry Function(CoflowFunctionIdentity identity) =>
        _functions.TryGetValue(identity, out var entry)
            ? entry
            : throw new CoflowProgramLinkException($"unknown function `{identity}`");

    internal object Record(string declaredType, string recordKey) =>
        _records.TryGet(declaredType, recordKey, out var value)
            ? value
            : throw new CoflowProgramLinkException(
                $"unknown record `{declaredType}::{recordKey}`");

    internal object Constant(CoflowConstant constant) => _context.ResolveConstant(constant);

    internal CoflowRawFunctionHandle FunctionHandle(CoflowFunctionEntry entry)
    {
        if (entry.Owner is null || !CoflowTypeCodecs.TryGet(entry.Owner.GetType(), out var descriptor))
            throw new CoflowProgramLinkException($"function `{entry.Identity}` has no receiver value");
        var kind = entry.Source is null ? CoflowFunctionKind.Native : CoflowFunctionKind.Program;
        return new CoflowRawFunctionHandle(
            new CoflowFunctionId(_context.SnapshotId, kind, entry.TargetIndex),
            descriptor.GetValueIdObject(entry.Owner));
    }
}

internal sealed class CoflowProgramTemplate
{
    private readonly CoflowInstruction[] _instructions;
    private readonly CfdSpan?[] _instructionSpans;
    private readonly object?[] _constants;
    private readonly int _localCount;
    private readonly CoflowBindingDependency[] _bindingDependencies;

    internal CoflowProgramTemplate(
        CoflowFunctionIdentity identity,
        string sourcePath,
        CfdSpan? sourceSpan,
        IReadOnlyList<CoflowInstruction> instructions,
        IReadOnlyList<CfdSpan?> instructionSpans,
        IReadOnlyList<object?> constants,
        IReadOnlyList<Type> parameterTypes,
        Type returnType,
        int localCount,
        IReadOnlyList<CoflowBindingDependency>? bindingDependencies = null)
    {
        Identity = identity;
        SourcePath = sourcePath;
        SourceSpan = sourceSpan;
        _instructions = instructions.ToArray();
        _instructionSpans = instructionSpans.ToArray();
        _constants = constants.ToArray();
        ParameterTypes = parameterTypes.ToArray();
        ReturnType = returnType;
        _localCount = localCount;
        _bindingDependencies = bindingDependencies?.ToArray() ?? Array.Empty<CoflowBindingDependency>();

        if (_instructions.Length == 0)
            throw new InvalidOperationException($"Coflow program `{identity}` has no instructions.");
        if (_instructionSpans.Length != _instructions.Length)
            throw new InvalidOperationException($"Coflow program `{identity}` has an invalid source map.");
        if (localCount < 0)
            throw new InvalidOperationException($"Coflow program `{identity}` has a negative local count.");
    }

    internal CoflowFunctionIdentity Identity { get; }
    internal string SourcePath { get; }
    internal CfdSpan? SourceSpan { get; }
    internal Type[] ParameterTypes { get; }
    internal Type ReturnType { get; }
    internal int ParameterCount => ParameterTypes.Length;

    internal bool CanReuse(CoflowCompilerCatalog catalog, CoflowRecordCatalog records) =>
        _bindingDependencies.All(dependency => dependency.IsStillValid(catalog, records));

    internal CoflowProgram Link(CoflowProgramLinker linker)
    {
        if (linker is null) throw new ArgumentNullException(nameof(linker));
        var operations = _constants.Select(value => value switch
        {
            CoflowClosureProgramTemplate closure => closure.Link(linker),
            CoflowFunctionReferenceTemplate function => function.Link(linker),
            CoflowRecordReferenceTemplate record => record.Link(linker),
            CoflowConstantReferenceTemplate constant => constant.Link(linker),
            _ => value,
        }).ToArray();
        var encodedConstants = new CoflowEncodedValue?[_constants.Length];
        for (var index = 0; index < _instructions.Length; index++)
        {
            var instruction = _instructions[index];
            if (instruction.Code != CoflowOpCode.Constant) continue;
            if ((uint)instruction.Operand >= (uint)_constants.Length)
                throw new InvalidOperationException($"Coflow program `{Identity}` has an invalid constant index.");
            encodedConstants[instruction.Operand] ??= CoflowEncodedValue.Encode(
                instruction.ValueType ?? _constants[instruction.Operand]?.GetType() ?? typeof(object),
                operations[instruction.Operand]);
            operations[instruction.Operand] = null;
        }
        var registerProgram = CoflowRegisterLowering.Lower(new CoflowLoweringInput(
            Identity, _instructions, _instructionSpans, operations, encodedConstants,
            ParameterTypes, ReturnType, _localCount, linker.FunctionIndexes));
        return new CoflowProgram(this, registerProgram);
    }
}

internal readonly record struct CoflowBindingDependency(
    string? DeclaredType,
    string RecordKey,
    string FieldName,
    string ResolvedDeclaredType)
{
    internal bool IsStillValid(CoflowCompilerCatalog catalog, CoflowRecordCatalog records)
    {
        var declaredType = DeclaredType;
        var fieldName = FieldName;
        var matches = records.WithKey(RecordKey).Where(candidate =>
            (declaredType is null || candidate.DeclaredType == declaredType) &&
            catalog.Metadata[candidate.DeclaredType].Fields.Any(field =>
                string.Equals(field.Name, fieldName, StringComparison.Ordinal))).ToArray();
        return matches.Length == 1 && matches[0].DeclaredType == ResolvedDeclaredType;
    }
}

internal sealed class CoflowProgram
{
    internal CoflowProgram(CoflowProgramTemplate template, CoflowRegisterProgram registerProgram)
    {
        Identity = template.Identity;
        SourcePath = template.SourcePath;
        SourceSpan = template.SourceSpan;
        ParameterTypes = template.ParameterTypes;
        ReturnType = template.ReturnType;
        RegisterProgram = registerProgram;
    }

    internal CoflowFunctionIdentity Identity { get; }
    internal string SourcePath { get; }
    internal CfdSpan? SourceSpan { get; }
    internal Type[] ParameterTypes { get; }
    internal Type ReturnType { get; }
    internal int ParameterCount => ParameterTypes.Length;
    internal CoflowRegisterProgram RegisterProgram { get; }
}
