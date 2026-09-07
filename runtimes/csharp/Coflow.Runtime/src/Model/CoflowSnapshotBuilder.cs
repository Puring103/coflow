namespace Coflow.Runtime;

using global::Coflow.Runtime.CompilerServices;

internal static class CoflowSnapshotBuilder
{
    internal static CoflowSnapshot Build(
        CfdDocument[] documents,
        ICoflowSchema schema,
        IReadOnlyDictionary<Type, object> hostBindings,
        IReadOnlyDictionary<long, CoflowModule> modules,
        CoflowLayoutRegistry layouts,
        global::Coflow.Runtime.CoflowOptions options,
        uint generation,
        uint snapshotId)
    {
        if (documents is null) throw new ArgumentNullException(nameof(documents));
        if (hostBindings is null) throw new ArgumentNullException(nameof(hostBindings));
        ValidateSchema(schema);
        var schemaIndex = new CoflowSchemaIndex(schema);
        var context = new CfdLoadContext(
            documents, schema.Types, schema.Constants, generation, snapshotId);
        // 维度辅助记录由生成 reader 按需读取，不作为普通表记录发布。
        var allRecords = context.Records.All
            .Where(record => !schema.Runtime.IsDimensionRecord(record.DeclaredType)).ToArray();
        foreach (var record in allRecords)
        {
            if (!schemaIndex.ByName.TryGetValue(record.DeclaredType, out var metadata))
                throw Error("CFD-REF-UNKNOWN-TYPE", $"unknown record type `{record.DeclaredType}`", record.Span);
            if (metadata is ICoflowHostMetadata)
                throw Error("CFD-HOST-RECORD", $"CFD cannot declare @Host `{record.DeclaredType}`", record.Span);
            if (metadata is not ICoflowRecordMetadata)
                throw Error("CFD-REF-UNKNOWN-TYPE", $"type `{record.DeclaredType}` cannot be declared as a record", record.Span);
        }
    
        var records = new CoflowRecordCatalog();
        var singletons = new Dictionary<Type, object>();
        foreach (var record in allRecords)
        {
            var metadata = (ICoflowRecordMetadata)schemaIndex.ByName[record.DeclaredType];
            var shell = context.AttachRecordValue(metadata, record.Key, metadata.CreateRecord(record.Key, context));
            records.Add(record.DeclaredType, record.Key, shell);
            context.RegisterRecord(record.DeclaredType, record.Key, shell);
        }
        foreach (var record in allRecords)
        {
            ((ICoflowRecordMetadata)schemaIndex.ByName[record.DeclaredType]).PopulateRecord(
                records.Get(record.DeclaredType, record.Key), record, context);
            context.CompleteRecordValue(record.DeclaredType, record.Key);
        }
        foreach (var metadata in schema.Types)
        {
            if (metadata is ICoflowHostMetadata hostMetadata)
            {
                hostBindings.TryGetValue(metadata.RuntimeType, out var binding);
                var host = hostMetadata.BindHost(binding, context);
                if (host is not null)
                {
                    host = context.AttachRecordValue(metadata, string.Empty, host);
                    context.CompleteRecordValue(metadata.DeclaredType, string.Empty);
                    singletons.Add(metadata.RuntimeType, host);
                    records.Add(metadata.DeclaredType, string.Empty, host);
                }
                continue;
            }
            var nodes = context.Records.OfType(metadata.DeclaredType);
            if (metadata.IsSingleton && nodes.Count > 1)
                throw Error("CFD-SINGLETON-COUNT", $"singleton `{metadata.DeclaredType}` appears more than once");
            if (metadata.IsSingleton && nodes.Count == 1)
                singletons.Add(metadata.RuntimeType, records.Get(metadata.DeclaredType, nodes[0].Key));
        }
    
        var tables = new Dictionary<Type, CoflowTable>();
        foreach (var metadata in schema.Types.OfType<ICoflowRecordMetadata>()
                     .Where(value => !value.IsSingleton && !value.IsAbstract))
        {
            var values = context.Records.AssignableTo(metadata.DeclaredType)
                .Select(value => records.Get(value.DeclaredType, value.Key)).ToArray();
            if (values.Length != 0) tables.Add(metadata.RuntimeType, metadata.CreateTable(values));
        }
        var closureTargets = CoflowCompilationPipeline.Compile(context.Functions, schema, records, context, modules);
        var functionSets = new List<int[]>();
        var valueEntries = context.Values.Select(value =>
        {
            var metadata = schemaIndex.ById[value.TypeId];
            var slots = Enumerable.Repeat(-1, metadata.Fields.Count).ToArray();
            foreach (var function in value.Functions)
            {
                if (!schemaIndex.FieldSlots.TryGetValue(
                        (metadata.TypeId, function.Identity.FieldName), out var slot))
                    throw new InvalidOperationException($"Function `{function.Identity}` has no schema field slot.");
                slots[slot] = function.ProgramIndex;
            }
            var functionSetIndex = functionSets.Count;
            functionSets.Add(slots);
            return new CoflowSnapshot.ValueEntry(
                value.TypeId, value.RecordKey, value.ApiValue, functionSetIndex);
        }).ToArray();
        var arena = CoflowRecordArena.Build(context.Values,
            schemaIndex.ById, snapshotId);
        return new CoflowSnapshot(schema, tables, singletons, context.Functions,
            valueEntries, arena, functionSets.ToArray(), closureTargets, layouts, schemaIndex,
            options, generation, snapshotId);
    }

    private static void ValidateSchema(ICoflowSchema schema)
    {
        if (schema is null) throw new ArgumentNullException(nameof(schema));
        var invalidTypeId = schema.Types.FirstOrDefault(value => !value.TypeId.IsValid);
        if (invalidTypeId is not null)
            throw Error("COFLOW-METADATA-TYPE-ID", $"schema type `{invalidTypeId.DeclaredType}` has an invalid TypeId");
        var duplicateTypeId = schema.Types.GroupBy(value => value.TypeId)
            .FirstOrDefault(value => value.Count() > 1);
        if (duplicateTypeId is not null)
            throw Error("COFLOW-METADATA-TYPE-ID", $"schema TypeId `{duplicateTypeId.Key.Value}` is duplicated");
        var duplicateName = schema.Types.Select(value => value.DeclaredType)
            .Concat(schema.Enums.Select(value => value.DeclaredType)).GroupBy(value => value, StringComparer.Ordinal)
            .FirstOrDefault(value => value.Count() > 1);
        if (duplicateName is not null) throw Error("COFLOW-METADATA-DUPLICATE-NAME", $"schema name `{duplicateName.Key}` is duplicated");
        var duplicateType = schema.Types.Select(value => value.RuntimeType)
            .Concat(schema.Enums.Select(value => value.RuntimeType)).GroupBy(value => value).FirstOrDefault(value => value.Count() > 1);
        if (duplicateType is not null) throw Error("COFLOW-METADATA-DUPLICATE-TYPE", $"runtime type `{duplicateType.Key}` is duplicated");
    }
    
    private static CoflowLoadException Error(string code, string message, CfdSpan? span = null) =>
        new(new[] { new CfdDiagnostic(code, message, string.Empty, span) });
}
