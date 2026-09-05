using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Threading;

namespace RoundtripDemo;

public abstract class Shape
{
    public abstract double Area();

    public virtual string Describe()
    {
        return "shape";    }

    protected Shape()
    {
        return;    }

}
