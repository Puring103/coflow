using System.Threading.Tasks;
using System.Threading;
using System.IO;
using System;
using System.Collections.Generic;
using System.Linq;

namespace Coflow.Runtime.CompilerServices
{

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
        if (entry.Owner is null || !CoflowSchemaRuntimeContext.TryGetTypeCodec(entry.Owner.GetType(), out var descriptor))
            throw new CoflowProgramLinkException($"function `{entry.Identity}` has no receiver value");
        var kind = entry.Source is null ? CoflowFunctionKind.Native : CoflowFunctionKind.Program;
        return new CoflowRawFunctionHandle(
            new CoflowFunctionId(_context.SnapshotId, kind, entry.TargetIndex),
            descriptor.GetValueIdObject(entry.Owner));
    }
}

internal sealed class CoflowProgramTemplate
{
    private readonly CoflowVirtualProgram _virtualProgram;
    private CoflowRegisterAllocation? _allocation;
    private CoflowRegisterTemplate? _registerTemplate;
    private readonly CoflowBindingDependency[] _bindingDependencies;

    internal CoflowProgramTemplate(CoflowVirtualProgram program)
    {
        if (program is null) throw new ArgumentNullException(nameof(program));
        _virtualProgram = program;
        Identity = program.Identity;
        SourcePath = program.SourcePath;
        SourceSpan = program.SourceSpan;
        ParameterTypes = program.Parameters.Select(value => value.Type).ToArray();
        ReturnType = program.ReturnType;
        _bindingDependencies = program.BindingDependencies;
    }

    internal CoflowFunctionIdentity Identity { get; }
    internal string SourcePath { get; }
    internal CfdSpan? SourceSpan { get; }
    internal Type[] ParameterTypes { get; }
    internal Type ReturnType { get; }
    internal int ParameterCount => ParameterTypes.Length;
    internal int AllocationCount { get; private set; }
    internal int LoweringCount { get; private set; }
    internal int LinkCount { get; private set; }
    internal IReadOnlyList<CoflowProgramTemplate> NestedClosureTemplates =>
        _virtualProgram.Blocks
            .SelectMany(block => block.Instructions)
            .Select(instruction => instruction.Operation)
            .OfType<CoflowVirtualOperation.MakeClosure>()
            .Select(operation => operation.Template.Program)
            .ToArray();

    internal bool CanReuse(CoflowCompilerCatalog catalog, CoflowRecordCatalog records) =>
        _bindingDependencies.All(dependency => dependency.IsStillValid(catalog, records));

    internal CoflowProgram Link(CoflowProgramLinker linker)
    {
        if (linker is null) throw new ArgumentNullException(nameof(linker));
        if (_registerTemplate is null)
        {
            if (_allocation is null)
            {
                _allocation = CoflowVirtualRegisterAllocator.Allocate(_virtualProgram);
                AllocationCount++;
            }
            _registerTemplate = CoflowVirtualLowering.LowerTemplate(_virtualProgram, linker, _allocation);
            LoweringCount++;
            LinkCount++;
            return new CoflowProgram(this, _registerTemplate.InitialProgram);
        }
        var linked = _registerTemplate.Link(linker, out var relinked);
        if (relinked) LinkCount++;
        return new CoflowProgram(this, linked);
    }
}

internal readonly struct CoflowBindingDependency
{
    public string? DeclaredType { get; init; }
    public string RecordKey { get; init; }
    public string FieldName { get; init; }
    public string ResolvedDeclaredType { get; init; }

    public CoflowBindingDependency(string? DeclaredType, string RecordKey, string FieldName, string ResolvedDeclaredType)
    {
        this.DeclaredType = DeclaredType;
        this.RecordKey = RecordKey;
        this.FieldName = FieldName;
        this.ResolvedDeclaredType = ResolvedDeclaredType;
    }

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
}
