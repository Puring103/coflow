#nullable enable
using Coflow;

namespace Game.Config
{
public interface IServices
{
    string environment { get; }
    int adjust(int arg0);
    Unit notify(int arg0);
}
}
