namespace Game.Config
{
// 用户扩展与生成文件分开，实际编译验证类、结构体与加载入口的 partial 契约。
public partial class Item
{
    public bool IsFree => price == 0;
}
public partial struct Stats
{
    public bool IsAlive => hp > 0;
}
public sealed partial class Settings
{
    public string StartKey => start.id;
}
public sealed partial class CoflowData
{
    public int ItemCount => Table<Item>().Count;
}
}
