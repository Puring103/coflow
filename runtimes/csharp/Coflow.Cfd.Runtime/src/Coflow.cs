using CoflowRuntime;
using CoflowRuntime.Generated;
using System.ComponentModel;

public static class Coflow
{
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static CoflowModule LoadData(string cfd, ICoflowGeneratedContract contract) =>
        CoflowModule.Load(new[] { cfd }, contract, false);
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static CoflowModule LoadData(string[] cfdSources, ICoflowGeneratedContract contract) =>
        CoflowModule.Load(cfdSources, contract, false);
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static CoflowModule LoadAndCompile(string cfd, ICoflowGeneratedContract contract) =>
        CoflowModule.Load(new[] { cfd }, contract, true);
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static CoflowModule LoadAndCompile(string[] cfdSources, ICoflowGeneratedContract contract) =>
        CoflowModule.Load(cfdSources, contract, true);
    public static CoflowModuleSet Modules(params CoflowModule[] modules) => new(modules);
}
