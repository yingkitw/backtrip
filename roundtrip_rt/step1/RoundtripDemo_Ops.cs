using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Threading;

namespace RoundtripDemo;

public static class Ops
{
    public static int Calls;

    [Obsolete("Use Abs2 instead.")]
    public static int Abs(int x)
    {
        if (x < 0) {
            return -x;
        }
        return x;    }

    public static int Abs2(int x)
    {
        return Math.Abs(x);    }

    public static int Sign(int x)
    {
        if (x > 0) {
            return 1;
        }
        if (x < 0) {
            return -1;
        }
        return 0;    }

    public static int Max(int a, int b)
    {
        if (a <= b) {
            return b;
        }
        return a;    }

    public static string DayName(int day)
    {
        switch (day)
        {
            case 0:
                return "Sun";
            case 1:
                return "Mon";
            case 2:
                return "Tue";
            default:
                return "???";
        }    }

    public static int WhileSum(int n)
    {
        int V_0 = default;
        int V_1 = default;

        for (V_1 = 1; V_1 <= n; V_1 = V_1 + 1) {
            V_0 = V_0 + V_1;
        }
        return V_0;    }

    public static int DoSum(int n)
    {
        int V_0 = default;

        do {
            V_0 = V_0 + n;
            n = n - 1;
        } while (n > 0);
        return V_0;    }

    public static int ForSum(int n)
    {
        int V_0 = default;
        int V_1 = default;

        for (V_1 = 0; V_1 < n; V_1 = V_1 + 1) {
            V_0 = V_0 + V_1;
        }
        return V_0;    }

    public static int ListSum(List<int> xs)
    {
        int V_0 = default;

        foreach (var V_2 in xs) {
            V_0 = V_0 + V_2;
        }
        return V_0;    }

    public static int ParseSafe(string s)
    {
        int V_0 = default;

        try {
        V_0 = int.Parse(s);
        goto Label_000E; // leave try
        }
        catch (FormatException) {
        V_0 = -1;
        goto Label_000E; // leave try
        }
        Label_000E:
        return V_0;    }

    public static int ElementAt(int[] xs, int i)
    {
        int V_0 = default;

        V_0 = -1;
        try {
        V_0 = xs[i];
        goto Label_0015; // leave try
        }
        finally {
        RoundtripDemo.Ops.Calls = RoundtripDemo.Ops.Calls + 1;
        // end finally
        }
        Label_0015:
        return V_0;    }

    public static void Swap(ref int a, ref int b)
    {
        int V_0 = default;

        V_0 = a;
        a = b;
        b = V_0;
        return;    }

    public static bool TryGet(int[] xs, int i, out int value)
    {
        value = 0;
        if (i < 0) goto Label_000D;
        if (i < (int)(xs.Length)) goto Label_000F;
        Label_000D:
        return false;
        Label_000F:
        value = xs[i];
        return true;    }

    public static int Total(params int[] xs)
    {
        int V_0 = default;
        int V_1 = default;

        for (V_1 = 0; V_1 < (int)(xs.Length); V_1 = V_1 + 1) {
            V_0 = V_0 + xs[V_1];
        }
        return V_0;    }

    public static double Scale(double v, double factor = 2.0)
    {
        return v * factor;    }

    public static object Box(int x)
    {
        return (object)(x);    }

    public static int Unbox(object o)
    {
        return (int)(o);    }

    public static int[] MakeRange(int n)
    {
        int[] V_0 = default;
        int V_1 = default;

        V_0 = new int[n];
        for (V_1 = 0; V_1 < n; V_1 = V_1 + 1) {
            V_0[V_1] = V_1;
        }
        return V_0;    }

    public static string KindOf(object o)
    {
        if (o as string != null) {
            return "text";
        }
        if (o is int) {
            return "number";
        }
        return "other";    }

    public static string AsText(object o)
    {
        string V_0 = default;

        V_0 = o as string;
        if (V_0 != null) {
            return V_0;
        }
        return "";    }

    public static List<int> FirstThree()
    {
        var V_tmp_0 = new List<int>() { 1, 2, 3 };
        return V_tmp_0;    }

    public static RoundtripDemo.Vec2 Origin()
    {
        RoundtripDemo.Vec2 V_0 = default;

        V_0 = new RoundtripDemo.Vec2 { X = 0, Y = 0 };
        return V_0;    }

    public static string Banner(string name, int n)
    {
        return "== " + name + ":" + n.ToString() + " ==";    }

    public static string Tag(RoundtripDemo.Planet p)
    {
        return "P" + ((RoundtripDemo.Planet)(p)).ToString("D");    }

    public static bool CanRead(RoundtripDemo.Access a)
    {
        return ((RoundtripDemo.Access)(a)).HasFlag(((RoundtripDemo.Access)(1)));    }

}
