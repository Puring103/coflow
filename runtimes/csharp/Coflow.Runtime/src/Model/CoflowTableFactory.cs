using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

/// <summary>供生成代码创建强类型表，不向生成程序集公开表的内部存储构造器。</summary>
[System.ComponentModel.EditorBrowsable(System.ComponentModel.EditorBrowsableState.Never)]
public static class CoflowTableFactory
{
    public static CoflowTable String<T>(object[] values, Func<T, string> key) where T : class =>
        new CoflowStringTable<T>(Array.ConvertAll(values, static value => (T)value), key);

    public static CoflowTable Enum<T, TKey>(object[] values, Func<T, TKey> key)
        where T : class where TKey : struct, System.Enum =>
        new CoflowEnumTable<T, TKey>(Array.ConvertAll(values, static value => (T)value), key);
}
}
