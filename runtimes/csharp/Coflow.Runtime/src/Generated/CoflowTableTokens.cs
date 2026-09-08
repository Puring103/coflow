using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

using System.ComponentModel;

[EditorBrowsable(EditorBrowsableState.Never)]
public interface ICoflowTableToken<TTable> where TTable : CoflowTable
{
    Type RecordType { get; }
    TTable Empty { get; }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CoflowStringTableToken<T> : ICoflowTableToken<CoflowStringTable<T>> where T : class
{
    private static readonly CoflowStringTable<T> EmptyTable = new(Array.Empty<T>(), static _ => string.Empty);
    public Type RecordType => typeof(T);
    public CoflowStringTable<T> Empty => EmptyTable;
}

[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CoflowEnumTableToken<T, TKey> : ICoflowTableToken<CoflowEnumTable<T, TKey>>
    where T : class where TKey : struct, Enum
{
    private static readonly CoflowEnumTable<T, TKey> EmptyTable = new(Array.Empty<T>(), static _ => default);
    public Type RecordType => typeof(T);
    public CoflowEnumTable<T, TKey> Empty => EmptyTable;
}
}
