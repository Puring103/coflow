using System;
using Xunit;

namespace Coflow.Loader.Tests;

public sealed class LoadingPlanTests
{
    [Fact]
    public void RequestedCombinationCyclesFailDuringLoad()
    {
        var fields=new CfdField[]{
            new CfdField<string>("first",CfdValues.String,CfdDefaults.String("first"),dimension:"one"),
            new CfdField<string>("second",CfdValues.String,CfdDefaults.String("second"),dimension:"two")};
        var type=new CfdType<Pair>("Pair",CfdTypeKind.Table,fields,_=>new Pair(),(value,reader)=>{
            value.First=reader.Get<string>(0);value.Second=reader.Get<string>(1);return value;});
        var sources=new[]{new CfdSource("pair.cfd","a: Pair {} one[v] { a: Pair { first: f\"{self.second}\" } } two[v] { a: Pair { second: f\"{self.first}\" } }")};
        Assert.True(CfdLoader.Load(sources,new[]{type},data=>data).Success);
        var result=CfdLoader.Load(sources,new[]{type},data=>data,options:new LoadOptions {
            Combinations=new[]{new DimensionSelection(("one","v"),("two","v"))}});
        Assert.False(result.Success); Assert.Equal("CFD-INTERPOLATION-CYCLE",result.Diagnostics[0].Code);
    }
    private sealed class Pair { internal string First=""; internal string Second=""; }

    [Theory]
    [InlineData("name: f\"{self.label}\"","CFD-INTERPOLATION-CYCLE")]
    [InlineData("note: Some(123)","CFD-TYPE")]
    public void SingleVariantErrorsRetainOverlayLocation(string field,string code)
    {
        var result=CfdLoader.Load(new[]{new CfdSource("base.cfd","a: Item { name: \"A\" }"),
            new CfdSource("overlay.cfd","language[ja] { a: Item { "+field+" } }")},LoadingTests.Types(),data=>data);
        Assert.False(result.Success); Assert.Equal(code,result.Diagnostics[0].Code); Assert.Equal("overlay.cfd",result.Diagnostics[0].Path);
    }

    [Fact]
    public void DynamicDictionaryKeysAreCheckedInEveryBuiltContext()
    {
        var type=new CfdType<Pair>("Pair",CfdTypeKind.Table,new CfdField[]{
            new CfdField<string>("first",CfdValues.String,dimension:"language"),
            new CfdField<System.Collections.Generic.IReadOnlyDictionary<string,int>>("map",CfdValues.Dictionary(CfdValues.String,CfdValues.Int), CfdDefaults.Dictionary(new CfdDefaultEntry(CfdDefaults.Template("f\"{self.first}\""),CfdDefaults.Int(1)), new CfdDefaultEntry(CfdDefaults.String("B"),CfdDefaults.Int(2))))
        },_=>new Pair(),(value,reader)=>{value.First=reader.Get<string>(0);return value;});
        var result=CfdLoader.Load(new[]{new CfdSource("keys.cfd","a: Pair { first: \"A\" } language[ja] { a: Pair { first: \"B\" } }")},new[]{type},data=>data);
        Assert.False(result.Success); Assert.Equal("CFD-DUPLICATE",result.Diagnostics[0].Code);
    }
}
