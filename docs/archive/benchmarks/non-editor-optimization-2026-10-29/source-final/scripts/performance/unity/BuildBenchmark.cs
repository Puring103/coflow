using System;
using System.IO;
using UnityEditor;
using UnityEditor.Build.Reporting;
using UnityEditor.SceneManagement;
using UnityEngine;

namespace Coflow.Benchmarks.Editor
{
    public static class BuildBenchmark
    {
        public static void Build()
        {
            var root = Path.GetDirectoryName(Application.dataPath);
            if (!File.Exists(Path.Combine(root, ".coflow-benchmark-project")))
                throw new InvalidOperationException("Only isolated benchmark projects can be built.");
            var output = Environment.GetEnvironmentVariable("COFLOW_BENCHMARK_OUTPUT");
            var backend = Environment.GetEnvironmentVariable("COFLOW_BENCHMARK_BACKEND");
            if (string.IsNullOrEmpty(output) || (backend != "Mono" && backend != "IL2CPP"))
                throw new ArgumentException("Benchmark output/backend environment is missing.");

            // 空场景与非开发构建只保留实际运行时宿主；不加载或修改用户工程。
            PlayerSettings.companyName = "Coflow";
            PlayerSettings.productName = "Coflow Benchmark";
            PlayerSettings.runInBackground = true;
            PlayerSettings.gcIncremental = false;
            // Unity 2022 的 NET_Standard 对应 .NET Standard 2.1，使用当前枚举名记录真实配置。
            PlayerSettings.SetApiCompatibilityLevel(BuildTargetGroup.Standalone, ApiCompatibilityLevel.NET_Standard);
            PlayerSettings.SetScriptingBackend(BuildTargetGroup.Standalone,
                backend == "IL2CPP" ? ScriptingImplementation.IL2CPP : ScriptingImplementation.Mono2x);
            PlayerSettings.SetManagedStrippingLevel(BuildTargetGroup.Standalone,
                backend == "IL2CPP" ? ManagedStrippingLevel.Minimal : ManagedStrippingLevel.Disabled);
            PlayerSettings.SetIl2CppCompilerConfiguration(BuildTargetGroup.Standalone, Il2CppCompilerConfiguration.Release);
            var scene = EditorSceneManager.NewScene(NewSceneSetup.EmptyScene, NewSceneMode.Single);
            EditorSceneManager.SaveScene(scene, "Assets/Benchmark.unity");
            var report = BuildPipeline.BuildPlayer(new BuildPlayerOptions
            {
                scenes = new[] { "Assets/Benchmark.unity" },
                locationPathName = Path.Combine(output, "CoflowBenchmark.exe"),
                target = BuildTarget.StandaloneWindows64,
                options = BuildOptions.None
            });
            File.WriteAllText(Path.Combine(output, "build-metadata.json"), JsonUtility.ToJson(new Metadata
            {
                unity_version = Application.unityVersion,
                backend = backend,
                development = false,
                incremental_gc = false,
                api_compatibility = "netstandard2.1",
                managed_stripping = backend == "IL2CPP" ? "Minimal" : "Disabled",
                il2cpp_compiler_configuration = "Release",
                target = "StandaloneWindows64",
                result = report.summary.result.ToString(),
                build_seconds = report.summary.totalTime.TotalSeconds,
                total_size_bytes = report.summary.totalSize.ToString(),
                utc = DateTime.UtcNow.ToString("o")
            }, true));
            if (report.summary.result != BuildResult.Succeeded)
                throw new InvalidOperationException("Benchmark player build failed: " + report.summary.result);
        }

        [Serializable]
        private sealed class Metadata
        {
            public string unity_version, backend, api_compatibility, managed_stripping;
            public string il2cpp_compiler_configuration, target, result, total_size_bytes, utc;
            public bool development, incremental_gc;
            public double build_seconds;
        }
    }
}
