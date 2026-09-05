using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Threading;

namespace RoundtripDemo;

public class Circle : Shape
{
    public double Radius;

    public Circle(double radius) : base()
    {

        this.Radius = radius;
        return;    }

    public override double Area()
    {
        return this.Radius * this.Radius * 3.14;    }

    public override string Describe()
    {
        return "circle";    }

}
