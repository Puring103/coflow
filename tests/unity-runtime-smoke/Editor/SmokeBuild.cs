using System;
using System.IO;
using UnityEditor;
using UnityEditor.Build.Reporting;
using UnityEditor.SceneManagement;
using UnityEngine;

public static class SmokeBuild
{
    public static void Build()
    {
        string backend = Environment.GetEnvironmentVariable("COFLOW_UNITY_BACKEND");
        string output = Environment.GetEnvironmentVariable("COFLOW_UNITY_OUTPUT");
        PlayerSettings.SetApiCompatibilityLevel(BuildTargetGroup.Standalone, ApiCompatibilityLevel.NET_Standard_2_0);
        PlayerSettings.SetScriptingBackend(BuildTargetGroup.Standalone, backend == "IL2CPP" ? ScriptingImplementation.IL2CPP : ScriptingImplementation.Mono2x);
        PlayerSettings.SetManagedStrippingLevel(BuildTargetGroup.Standalone, ManagedStrippingLevel.Low);
        var importer = (PluginImporter)AssetImporter.GetAtPath("Assets/Plugins/x86_64/coflow_ffi.dll");
        importer.SetCompatibleWithAnyPlatform(false); importer.SetCompatibleWithEditor(true);
        importer.SetCompatibleWithPlatform(BuildTarget.StandaloneWindows64, true);
        importer.SetPlatformData(BuildTarget.StandaloneWindows64, "CPU", "x86_64"); importer.SaveAndReimport();
        var scene = EditorSceneManager.NewScene(NewSceneSetup.EmptyScene, NewSceneMode.Single);
        new GameObject("Coflow smoke").AddComponent<CoflowSmoke>();
        EditorSceneManager.SaveScene(scene, "Assets/Smoke.unity");
        var report = BuildPipeline.BuildPlayer(new[] { "Assets/Smoke.unity" }, output, BuildTarget.StandaloneWindows64, BuildOptions.None);
        if (report.summary.result != BuildResult.Succeeded) throw new Exception("Smoke build failed: " + report.summary.result);
    }
}
