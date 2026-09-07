namespace Coflow.Runtime.CompilerServices;

internal readonly record struct CoflowConstantRelocation(int Descriptor, Type Type, object Symbol);
internal readonly record struct CoflowNativeRelocation(int Descriptor, object Symbol);
internal readonly record struct CoflowClosureRelocation(int Descriptor, CoflowClosureProgramTemplate Template);
internal readonly record struct CoflowCallRelocation(int Descriptor, CoflowCallSite Call);

/// <summary>保存链接无关的最终编码；新快照只替换包含快照身份或程序索引的 descriptor。</summary>
internal sealed class CoflowRegisterTemplate
{
    private readonly CoflowRegisterProgram _program;
    private readonly CoflowFrozenArray<CoflowConstantRelocation> _constants;
    private readonly CoflowFrozenArray<CoflowNativeRelocation> _nativeCalls;
    private readonly CoflowFrozenArray<CoflowClosureRelocation> _closures;
    private readonly CoflowFrozenArray<CoflowCallRelocation> _calls;
    private CoflowRegisterProgram _latest;

    internal CoflowRegisterTemplate(
        CoflowRegisterProgram program,
        IReadOnlyList<CoflowConstantRelocation> constants,
        IReadOnlyList<CoflowNativeRelocation> nativeCalls,
        IReadOnlyList<CoflowClosureRelocation> closures,
        IReadOnlyList<CoflowCallRelocation> calls)
    {
        _program = program;
        _latest = program;
        _constants = CoflowFrozenArray<CoflowConstantRelocation>.Owned(constants.ToArray());
        _nativeCalls = CoflowFrozenArray<CoflowNativeRelocation>.Owned(nativeCalls.ToArray());
        _closures = CoflowFrozenArray<CoflowClosureRelocation>.Owned(closures.ToArray());
        _calls = CoflowFrozenArray<CoflowCallRelocation>.Owned(calls.ToArray());
    }

    internal CoflowRegisterProgram InitialProgram => _program;

    internal CoflowRegisterProgram Link(CoflowProgramLinker linker, out bool relinked)
    {
        var source = _latest.Operations;
        var requiresRelink = _constants.Count != 0 || _nativeCalls.Count != 0 || _closures.Count != 0;
        foreach (var relocation in _calls)
        {
            if (!linker.FunctionIndexes.TryGetValue(relocation.Call.Identity, out var index))
                throw new CoflowProgramLinkException($"unknown function `{relocation.Call.Identity}`");
            requiresRelink |= source.Calls[relocation.Descriptor].ProgramIndex != index;
        }
        if (!requiresRelink)
        {
            relinked = false;
            return _latest;
        }

        var constants = source.Constants.ToArray();
        foreach (var relocation in _constants)
        {
            var target = constants[relocation.Descriptor].Target;
            constants[relocation.Descriptor] = new CoflowRegisterConstantSite(
                CoflowEncodedValue.Encode(relocation.Type, CoflowVirtualLowering.LinkSymbol(relocation.Symbol, linker)),
                target);
        }

        var nativeCalls = source.NativeCalls.ToArray();
        foreach (var relocation in _nativeCalls)
        {
            var current = nativeCalls[relocation.Descriptor];
            nativeCalls[relocation.Descriptor] = new CoflowNativeCallSite(
                (CoflowNativeCall)CoflowVirtualLowering.LinkSymbol(relocation.Symbol, linker)!,
                current.Arguments.ToArray(), current.Result);
        }

        var closures = source.Closures.ToArray();
        foreach (var relocation in _closures)
        {
            var current = closures[relocation.Descriptor];
            closures[relocation.Descriptor] = new CoflowRegisterClosureSite(
                relocation.Template.Link(linker), current.Captures.ToArray(), current.Target);
        }

        var calls = source.Calls.ToArray();
        foreach (var relocation in _calls)
        {
            var index = linker.FunctionIndexes.TryGetValue(relocation.Call.Identity, out var value)
                ? value
                : throw new CoflowProgramLinkException($"unknown function `{relocation.Call.Identity}`");
            calls[relocation.Descriptor] = calls[relocation.Descriptor].Relink(index);
        }

        var operations = new CoflowRegisterOperations
        {
            References = source.References,
            Constants = CoflowFrozenArray<CoflowRegisterConstantSite>.Owned(constants),
            Transfers = source.Transfers,
            Fields = source.Fields,
            FieldValues = source.FieldValues,
            NativeCalls = CoflowFrozenArray<CoflowNativeCallSite>.Owned(nativeCalls),
            Collections = source.Collections,
            Indexes = source.Indexes,
            CollectionReads = source.CollectionReads,
            Projections = source.Projections,
            CollectionBuiltins = source.CollectionBuiltins,
            ArrayBuilders = source.ArrayBuilders,
            Targets = source.Targets,
            Propagates = source.Propagates,
            Closures = CoflowFrozenArray<CoflowRegisterClosureSite>.Owned(closures),
            Types = source.Types,
            Calls = CoflowFrozenArray<CoflowRegisterCallSite>.Owned(calls),
            IndirectCalls = source.IndirectCalls,
        };
        _latest = _program.Relink(operations);
        relinked = true;
        return _latest;
    }
}

internal sealed class CoflowRelocationBuilder
{
    internal List<CoflowConstantRelocation> Constants { get; } = new();
    internal List<CoflowNativeRelocation> NativeCalls { get; } = new();
    internal List<CoflowClosureRelocation> Closures { get; } = new();
    internal List<CoflowCallRelocation> Calls { get; } = new();
}
