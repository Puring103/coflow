using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Loader
{

/// <summary>Logical project path and UTF-16 source text for one CFD file.</summary>
public readonly struct CfdSource : IEquatable<CfdSource>
{
    public CfdSource(string path, string text)
    {
        Path = path ?? throw new ArgumentNullException(nameof(path));
        Text = text ?? throw new ArgumentNullException(nameof(text));
    }

    public string Path { get; }
    public string Text { get; }

    public bool Equals(CfdSource other) => Path == other.Path && Text == other.Text;
    public override bool Equals(object? obj) => obj is CfdSource other && Equals(other);
    public override int GetHashCode() => HashCode.Combine(Path, Text);
}

}
