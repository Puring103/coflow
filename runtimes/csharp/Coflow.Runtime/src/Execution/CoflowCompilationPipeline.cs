using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

/// <summary>编排函数去重、模板缓存、全局链接和原子发布；不参与单个函数的语义分析。</summary>
internal static class CoflowCompilationPipeline
{
    internal static CoflowClosureTemplate[] Compile(
        IReadOnlyList<CoflowFunctionEntry> entries,
        ICoflowSchema schema,
        CoflowRecordCatalog records,
        CfdLoadContext context,
        IReadOnlyDictionary<long, CoflowModule> modules)
    {
        var canonical = new Dictionary<CoflowFunctionEntry, CoflowFunctionEntry>();
        var defaults = new Dictionary<(string DeclaredType, string FieldName, string Source), CoflowFunctionEntry>();
        var definitions = new List<CoflowFunctionEntry>();
        var seenDefinitions = new HashSet<CoflowFunctionEntry>();
        foreach (var entry in entries)
        {
            var definition = entry;
            if (entry.IsDefault && entry.Source is { } defaultSource)
            {
                var key = (entry.Identity.DeclaredType, entry.Identity.FieldName, defaultSource.Source);
                if (!defaults.TryGetValue(key, out definition!))
                {
                    definition = entry;
                    defaults.Add(key, definition);
                }
            }
            if (seenDefinitions.Add(definition)) definitions.Add(definition);
            canonical.Add(entry, definition);
        }
        for (var index = 0; index < definitions.Count; index++)
            definitions[index].AssignProgramIndex(index);
        foreach (var pair in canonical)
            if (!ReferenceEquals(pair.Key, pair.Value)) pair.Key.AssignProgramIndex(pair.Value.ProgramIndex);

        var functions = entries.ToDictionary(entry => entry.Identity);
        var compiled = new List<(CoflowFunctionEntry Entry, CoflowProgramTemplate? Body)>();
        var stagedTemplates = new List<(CoflowModule Module, CoflowFunctionIdentity Identity,
            CoflowProgramTemplate Template)>();
        var diagnostics = new List<CfdDiagnostic>();
        var catalog = new CoflowCompilerCatalog(schema);
        foreach (var entry in definitions)
        {
            if (entry.Source is null)
            {
                if (entry.RequiresCfdBody)
                {
                    diagnostics.Add(new CfdDiagnostic(
                        "COFLOW-FUNCTION-MISSING",
                        $"{entry.Identity.DeclaredType}.{entry.Identity.RecordKey}.{entry.Identity.FieldName}: ordinary functions require a CFD body",
                        entry.SourcePath,
                        entry.SourceSpan));
                    continue;
                }
                compiled.Add((entry, null));
                continue;
            }

            try
            {
                if (modules.TryGetValue(entry.ModuleId, out var owner) &&
                    owner.TryGetTemplate(entry.Identity, out var cached) &&
                    cached.CanReuse(catalog, records))
                {
                    compiled.Add((entry, cached));
                    continue;
                }
                var syntax = CoflowFunctionFrontend.FunctionSyntaxParser.Parse(entry.Source.Source);
                var bound = CoflowFunctionFrontend.FunctionBinder.Bind(entry, syntax, catalog);
                var frontend = new CoflowFunctionFrontend.FunctionParser(
                    bound, catalog, records, context);
                var typed = frontend.ParseAndTypeBody();
                var template = new CoflowFunctionFrontend.TypedCfgLowerer(
                    entry,
                    catalog.Metadata,
                    catalog.Enums).Lower(typed);
                compiled.Add((entry, template));
                if (owner is not null)
                    stagedTemplates.Add((owner, entry.Identity, template));
            }
            catch (CoflowFunctionFrontend.FunctionCompileException error)
            {
                diagnostics.Add(new CfdDiagnostic(
                    error.Code,
                    $"{entry.Identity.DeclaredType}.{entry.Identity.RecordKey}.{entry.Identity.FieldName}: {error.Message}",
                    entry.SourcePath,
                    error.Offset is { } offset
                        ? FunctionSpan(entry.Source, offset)
                        : entry.Source.Span));
            }
        }
        if (diagnostics.Count != 0) throw new CoflowLoadException(diagnostics);

        var linker = new CoflowProgramLinker(functions, records, context);
        var linked = new List<(CoflowFunctionEntry Entry, CoflowProgram? Body)>();
        foreach (var item in compiled)
        {
            try
            {
                linked.Add((item.Entry, item.Body?.Link(linker)));
            }
            catch (CoflowProgramLinkException error)
            {
                diagnostics.Add(new CfdDiagnostic(
                    "COFLOW-FUNCTION-LINK",
                    $"{item.Entry.Identity.DeclaredType}.{item.Entry.Identity.RecordKey}." +
                    $"{item.Entry.Identity.FieldName}: {error.Message}",
                    item.Entry.SourcePath,
                    item.Entry.SourceSpan));
            }
        }
        if (diagnostics.Count != 0) throw new CoflowLoadException(diagnostics);

        // 缓存和 executable 只在全部函数完成语义分析及全局链接后发布。
        foreach (var item in stagedTemplates)
            item.Module.PublishTemplate(item.Identity, item.Template);
        foreach (var item in linked)
            item.Entry.PublishCompiled(item.Body);
        foreach (var pair in canonical)
            if (!ReferenceEquals(pair.Key, pair.Value))
                pair.Key.PublishCompiled(pair.Value.CompiledProgram);
        return linker.Closures.ToArray();
    }

    internal static CfdSpan FunctionSpan(CfdFunctionValue function, int offset)
    {
        var line = function.Span.StartLine;
        var column = function.Span.StartColumn;
        var length = Math.Min(Math.Max(offset, 0), function.Source.Length);
        for (var index = 0; index < length; index++)
        {
            if (function.Source[index] == '\n')
            {
                line++;
                column = 1;
            }
            else
            {
                column++;
            }
        }
        return new CfdSpan(line, column, line, column + (length < function.Source.Length ? 1 : 0));
    }
}
}
