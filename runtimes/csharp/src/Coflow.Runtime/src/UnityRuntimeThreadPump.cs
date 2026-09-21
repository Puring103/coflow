#if UNITY_2022_1_OR_NEWER
using UnityEngine;

namespace Coflow
{
    /// <summary>在 Unity 主线程持续处理原生终结请求，并在正常退出时关闭回收域。</summary>
    internal sealed class UnityRuntimeThreadPump : MonoBehaviour
    {
        [RuntimeInitializeOnLoadMethod(RuntimeInitializeLoadType.AfterAssembliesLoaded)]
        private static void Install()
        {
            var owner = new GameObject("Coflow Runtime Thread");
            owner.hideFlags = HideFlags.HideAndDontSave;
            DontDestroyOnLoad(owner);
            owner.AddComponent<UnityRuntimeThreadPump>();
        }

        private void Update() => RuntimeThread.DrainFinalizers();

        private void OnApplicationQuit()
        {
            RuntimeThread.DrainFinalizers();
            RuntimeThread.Shutdown();
        }
    }
}
#endif
