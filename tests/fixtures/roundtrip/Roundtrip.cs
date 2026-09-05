// Round-trip fixture for backtrip: compiled to Roundtrip.dll, decompiled
// back to C#, recompiled, and re-decompiled to validate fidelity.
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

namespace RoundtripDemo
{
    public enum Planet
    {
        Mercury = 1,
        Venus = 2,
        Earth = 3
    }

    [Flags]
    public enum Access
    {
        None = 0,
        Read = 1,
        Write = 2,
        Execute = 4
    }

    public delegate int Compute(int left, int right);

    public interface IWorker
    {
        string Name { get; }
        int DoWork(int units);
    }

    public interface IReset
    {
        void Reset();
    }

    public struct Vec2
    {
        public int X;
        public int Y;

        public int Sum()
        {
            return X + Y;
        }

        public override string ToString()
        {
            return X + "," + Y;
        }
    }

    public abstract class Shape
    {
        public abstract double Area();

        public virtual string Describe()
        {
            return "shape";
        }
    }

    public class Circle : Shape
    {
        public double Radius;

        public Circle(double radius)
        {
            Radius = radius;
        }

        public override double Area()
        {
            return Radius * Radius * 3.14;
        }

        public override string Describe()
        {
            return "circle";
        }
    }

    public class Worker : IWorker, IReset
    {
        public const int MaxUnits = 999;
        public static int Built;
        private static int TotalDone;
        private readonly object _sync = new object();

        public string Name { get; set; }
        public int Done;
        public event Compute Progress;

        public Worker(string name)
        {
            Name = name;
            Done = 0;
            Built = Built + 1;
        }

        static Worker()
        {
            Built = 0;
            TotalDone = 0;
        }

        public int DoWork(int units)
        {
            Done += units;
            TotalDone += units;
            return Done;
        }

        void IReset.Reset()
        {
            Done = 0;
        }

        public void Bump()
        {
            lock (_sync)
            {
                Done++;
            }
        }
    }

    public class Engine<T> : IWorker
    {
        private T _state;

        public string Name { get; set; }
        public T State { get; }

        public Engine(T initial, string name)
        {
            State = initial;
            Name = name;
            _state = initial;
        }

        public U Map<U>(Func<T, U> f)
        {
            return f(_state);
        }

        public int DoWork(int units)
        {
            return units;
        }
    }

    public static class Ops
    {
        [Obsolete("Use Abs2 instead.")]
        public static int Abs(int x)
        {
            if (x < 0)
            {
                return -x;
            }
            return x;
        }

        public static int Abs2(int x)
        {
            return Math.Abs(x);
        }

        public static int Sign(int x)
        {
            if (x > 0)
            {
                return 1;
            }
            else if (x < 0)
            {
                return -1;
            }
            return 0;
        }

        public static int Max(int a, int b)
        {
            return a > b ? a : b;
        }

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
            }
        }

        public static int WhileSum(int n)
        {
            int sum = 0;
            int i = 1;
            while (i <= n)
            {
                sum += i;
                i++;
            }
            return sum;
        }

        public static int DoSum(int n)
        {
            int sum = 0;
            do
            {
                sum += n;
                n--;
            } while (n > 0);
            return sum;
        }

        public static int ForSum(int n)
        {
            int sum = 0;
            for (int i = 0; i < n; i++)
            {
                sum += i;
            }
            return sum;
        }

        public static int ListSum(List<int> xs)
        {
            int sum = 0;
            foreach (int x in xs)
            {
                sum += x;
            }
            return sum;
        }

        public static int ParseSafe(string s)
        {
            try
            {
                return int.Parse(s);
            }
            catch (FormatException)
            {
                return -1;
            }
        }

        public static int Calls;

        public static int ElementAt(int[] xs, int i)
        {
            int v = -1;
            try
            {
                v = xs[i];
            }
            finally
            {
                Calls++;
            }
            return v;
        }

        public static void Swap(ref int a, ref int b)
        {
            int t = a;
            a = b;
            b = t;
        }

        public static bool TryGet(int[] xs, int i, out int value)
        {
            value = 0;
            if (i < 0 || i >= xs.Length)
            {
                return false;
            }
            value = xs[i];
            return true;
        }

        public static int Total(params int[] xs)
        {
            int sum = 0;
            for (int i = 0; i < xs.Length; i++)
            {
                sum += xs[i];
            }
            return sum;
        }

        public static double Scale(double v, double factor = 2.0)
        {
            return v * factor;
        }

        public static object Box(int x)
        {
            return x;
        }

        public static int Unbox(object o)
        {
            return (int)o;
        }

        public static int[] MakeRange(int n)
        {
            int[] a = new int[n];
            for (int i = 0; i < n; i++)
            {
                a[i] = i;
            }
            return a;
        }

        public static string KindOf(object o)
        {
            if (o is string)
            {
                return "text";
            }
            if (o is int)
            {
                return "number";
            }
            return "other";
        }

        public static string AsText(object o)
        {
            string s = o as string;
            if (s != null)
            {
                return s;
            }
            return "";
        }

        public static List<int> FirstThree()
        {
            return new List<int> { 1, 2, 3 };
        }

        public static Vec2 Origin()
        {
            return new Vec2 { X = 0, Y = 0 };
        }

        public static string Banner(string name, int n)
        {
            return "== " + name + ":" + n + " ==";
        }

        // Note: `(int)p` casts are invisible in IL (identity representation),
        // so they cannot round-trip without type inference. ToString("D")
        // yields the same numeric text and survives the round-trip.
        public static string Tag(Planet p)
        {
            return "P" + p.ToString("D");
        }

        // Note: `(a & Access.Read) != 0` compiles to an integer `and` whose
        // operands lose their enum-ness in IL; decompiled output would not
        // compile back. HasFlag survives the round-trip.
        public static bool CanRead(Access a)
        {
            return a.HasFlag(Access.Read);
        }
    }

    public static class Native
    {
        [DllImport("libc")]
        public static extern int getpid();
    }
}
