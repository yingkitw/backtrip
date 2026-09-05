using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Threading;

namespace RoundtripDemo;

    [Flags]
public enum Access : int
{
    None = 0,
    Read = 1,
    Write = 2,
    Execute = 4
}
