#nullable enable
using Coflow;

namespace Game.Config
{
public interface IHostServices
{
    string environment { get; }
    global::Game.Config.Character? favorite { get; }
    global::Game.Config.Mood mood { get; }
    Unit log(string message);
    global::Game.Config.Stats echoStats(global::Game.Config.Stats value);
    global::Game.Config.Profile echoProfile(global::Game.Config.Profile value);
    global::Game.Config.Character echoCharacter(global::Game.Config.Character value);
}
}
