namespace CoflowRuntime.Generated;

internal sealed class CfdBoundDocuments
{
    internal CfdBoundDocuments(
        IReadOnlyList<CfdDocument> documents,
        IReadOnlyDictionary<CfdRecordNode, string> recordPaths)
    {
        Documents = documents;
        RecordPaths = recordPaths;
    }

    internal IReadOnlyList<CfdDocument> Documents { get; }
    internal IReadOnlyDictionary<CfdRecordNode, string> RecordPaths { get; }
}

internal static class CfdDocumentBinder
{
    internal static CfdBoundDocuments Bind(
        IReadOnlyList<CfdDocument> documents,
        IReadOnlyDictionary<string, CoflowConstant>? constants = null)
    {
        var boundDocuments = new List<CfdDocument>(documents.Count);
        var recordPaths = new Dictionary<CfdRecordNode, string>();
        foreach (var document in documents)
        {
            var records = document.Records.Select(record => BindRecord(record, constants)).ToArray();
            foreach (var record in records) recordPaths.Add(record, document.Path);
            boundDocuments.Add(new CfdDocument(document.Path, records));
        }
        return new CfdBoundDocuments(boundDocuments, recordPaths);
    }

    private static CfdRecordNode BindRecord(
        CfdRecordNode record,
        IReadOnlyDictionary<string, CoflowConstant>? constants) =>
        new(
            record.Key,
            record.DeclaredType,
            record.Fields.Select(field => new CfdFieldNode(
                field.Name,
                BindValue(field.Value, constants),
                field.Span)).ToArray(),
            record.Span,
            record.GroupType);

    private static CfdValueNode BindValue(
        CfdValueNode value,
        IReadOnlyDictionary<string, CoflowConstant>? constants) => value switch
    {
        CfdSomeValue some => new CfdSomeValue(BindValue(some.Value, constants), some.Span),
        CfdOkValue ok => new CfdOkValue(BindValue(ok.Value, constants), ok.Span),
        CfdErrValue error => new CfdErrValue(BindValue(error.Value, constants), error.Span),
        CfdScalarValue scalar when constants is not null && constants.TryGetValue(scalar.Value, out var constant)
            => new CfdConstantValue(constant, scalar.Span),
        CfdObjectValue objectValue => new CfdObjectValue(
            objectValue.DeclaredType,
            objectValue.Fields.Select(field => new CfdFieldNode(
                field.Name,
                BindValue(field.Value, constants),
                field.Span)).ToArray(),
            objectValue.Span),
        CfdArrayValue array => new CfdArrayValue(
            array.Items.Select(item => BindValue(item, constants)).ToArray(),
            array.Span),
        CfdDictionaryValue dictionary => new CfdDictionaryValue(
            dictionary.Entries.Select(entry => new CfdEntryNode(
                BindValue(entry.Key, constants),
                BindValue(entry.Value, constants),
                entry.Span)).ToArray(),
            dictionary.Span),
        _ => value,
    };
}
