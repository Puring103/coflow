using System;
using System.Collections;
using System.Collections.Generic;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Threading.Tasks;
using Xunit;

namespace Coflow.Loader.Tests;

public sealed class SparseStorageTests
{
    [Fact]
    public void CompletedGraphDoesNotRetainBuildStateAndAllReadsAreAllocationFree()
    {
        var (data, builder, source)=BuildTracked();
        GC.Collect();GC.WaitForPendingFinalizers();GC.Collect();
        Assert.False(builder.IsAlive); Assert.False(source.IsAlive);
        var dimension=data.Table<LoadingTests.Item>()["a"]; var ja=dimension.For("language","ja");
        Assert.Equal("甲",ja.Labels[0]); Assert.Same(ja.Labels[0],ja.Labels[0]); Assert.Same(ja.Map["key"],ja.Map["key"]);
        for(int i=0;i<1000;i++) _=dimension.For("language","ja").Labels[0];
        long start=GC.GetAllocatedBytesForCurrentThread();int checksum=0;
        for(int i=0;i<10000;i++) checksum+=dimension.For("language","ja").Labels[0].Length+ja.Map["key"].Length;
        long allocated=GC.GetAllocatedBytesForCurrentThread()-start;
        Assert.Equal(20000,checksum);Assert.Equal(0,allocated);
        AssertDetached(data,new HashSet<object>(ReferenceEqualityComparer.Instance));
        GC.KeepAlive(data);
    }
    [MethodImpl(MethodImplOptions.NoInlining)]
    private static (CfdData,WeakReference,WeakReference) BuildTracked()
    {
        var documents=CfdParser.ParseAll(new[]{new CfdSource("tracked.cfd",LoadingTests.Source)});
        var types=LoadingTests.Types();var context=new LoadContext(documents,types,null);context.Build();
        return (new CfdData(types,context),new WeakReference(context),new WeakReference(documents));
    }
    private static void AssertDetached(object? value,HashSet<object> seen)
    {
        if(value==null||value is string||value is Type||value.GetType().IsPrimitive||value.GetType().IsEnum||!seen.Add(value))return;
        Assert.False(value is CfdReader or LoadContext or BuildSession or BoundValue or Delegate,$"Unexpected retained {value.GetType()}");
        if(value is IEnumerable items) { foreach(var item in items)AssertDetached(item,seen); return; }
        foreach(var field in value.GetType().GetFields(BindingFlags.Instance|BindingFlags.Public|BindingFlags.NonPublic)) AssertDetached(field.GetValue(value),seen);
    }
    [Fact]
    public void ParallelReadersReuseCompletedObjectsWithoutConstruction()
    {
        int count=0;var result=LoadingTests.Load(LoadingTests.Source,created:()=>count++);Assert.True(result.Success);
        var built=count;var a=result.Data.Table<LoadingTests.Item>()["a"];var expected=a.For("language","ja");
        Parallel.For(0,2000,_=>{
            var row=a.For("language","ja");Assert.Same(expected,row);Assert.Equal("甲",row.Labels[0]);
            Assert.Equal("乙",row.Links[0].Name);Assert.Same(row,row.Next.Value.Next.Value);
        });Assert.Equal(built,count);
    }
}
