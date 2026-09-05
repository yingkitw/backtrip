using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Threading;

namespace RoundtripDemo;

public struct Vec2
{
    public int X;
    public int Y;

    public int Sum()
    {
        return this.X + this.Y;    }

    public override string ToString()
    {
        return this.X.ToString() + "," + this.Y.ToString();    }

}
