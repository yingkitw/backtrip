using System;

namespace Shapes
{
    [System.Obsolete("Use NewCalculator instead.")]
    public class Calculator
    {
        public int Value;
        public const int MaxValue = 100;
        public string Label { get; set; }
        public event Notify OnCalculating;

        public Calculator(int value)
        {
            Value = value;
        }

        public class Settings
        {
            public bool Enabled;
            public string Label;
            public string ReadOnly { get; }
            public int Count { get; set; } = 42;

            public Settings(string label)
            {
                Label = label;
                Enabled = true;
                ReadOnly = "default";
            }
        }

        public int Add(int a, int b)
        {
            return a + b;
        }

        public int Add(int a, int b, int c = 0)
        {
            return a + b + c;
        }

        public int Multiply(int a, int b)
        {
            return a * b;
        }

        public string Greet(string name)
        {
            return "Hello, " + name + "!";
        }

        public int SumUpTo(int n)
        {
            int sum = 0;
            for (int i = 1; i <= n; i++)
            {
                sum = sum + i;
            }
            return sum;
        }

        public static int Square(int x)
        {
            return x * x;
        }

        public string Classify(int day)
        {
            switch (day)
            {
                case 0: return "Sunday";
                case 1: return "Monday";
                case 2: return "Tuesday";
                case 3: return "Wednesday";
                case 4: return "Thursday";
                case 5: return "Friday";
                case 6: return "Saturday";
                default: return "Unknown";
            }
        }

        public int SafeParse(string input)
        {
            try
            {
                return int.Parse(input);
            }
            catch (System.FormatException)
            {
                return -1;
            }
        }

        public int Abs(int x)
        {
            if (x < 0)
            {
                return -x;
            }
            return x;
        }

        public int Sign(int x)
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

        public int CountDown(int start)
        {
            int count = 0;
            do
            {
                count++;
                start--;
            } while (start > 0);
            return count;
        }

        public int SumFor(int n)
        {
            int sum = 0;
            for (int i = 1; i <= n; i++)
            {
                sum += i;
            }
            return sum;
        }

        public void Swap(ref int a, ref int b)
        {
            int tmp = a;
            a = b;
            b = tmp;
        }

        public bool TryParseInt(string s, out int result)
        {
            result = 0;
            return int.TryParse(s, out result);
        }

        private object _sync = new object();
        public int Counter = 0;
        public void IncrementThreadSafe()
        {
            lock (_sync)
            {
                Counter++;
            }
        }

        public string ReadFile(string path)
        {
            using (System.IO.StreamReader reader = new System.IO.StreamReader(path))
            {
                return reader.ReadToEnd();
            }
        }

        public int SumList(System.Collections.Generic.List<int> numbers)
        {
            int sum = 0;
            foreach (int n in numbers)
            {
                sum += n;
            }
            return sum;
        }

        public System.Collections.Generic.List<int> MakeList()
        {
            var list = new System.Collections.Generic.List<int> { 1, 2, 3 };
            return list;
        }

        public int[] MakeArray(int n)
        {
            return new int[n];
        }

        public int FirstElement(int[] xs)
        {
            return xs[0];
        }

        public int ArrayLength(int[] xs)
        {
            return xs.Length;
        }

        public object BoxInt(int x)
        {
            return x;
        }

        public int UnboxInt(object o)
        {
            return (int)o;
        }

        public string CastString(object o)
        {
            return (string)o;
        }

        public string DescribeObject(object obj)
        {
            if (obj is string s)
            {
                return "String: " + s;
            }
            else if (obj is int n)
            {
                return "Int: " + n.ToString();
            }
            return "Unknown";
        }

        public int RunWithClosure(int offset)
        {
            System.Func<int, int> adder = x => x + offset;
            return adder(10);
        }

        public string ClassifyDay(int day) => day switch
        {
            0 => "Sunday",
            1 => "Monday",
            2 => "Tuesday",
            3 => "Wednesday",
            4 => "Thursday",
            5 => "Friday",
            6 => "Saturday",
            _ => "Unknown"
        };
    }

    public struct Point
    {
        public int X;
        public int Y;

        public double Distance(Point other)
        {
            int dx = X - other.X;
            int dy = Y - other.Y;
            return System.Math.Sqrt(dx * dx + dy * dy);
        }
    }

    public enum Color
    {
        Red = 0,
        Green = 1,
        Blue = 2
    }

    public delegate void Notify(string message);

    public interface IResettable
    {
        void Reset();
    }

    public class Counter : IResettable
    {
        [System.Obsolete]
        public int Count;

        [System.Obsolete("Use IncrementBy instead.")]
        public void Increment()
        {
            Count++;
        }

        void IResettable.Reset()
        {
            Count = 0;
        }

        [System.Runtime.InteropServices.DllImport("libc", SetLastError = true)]
        public static extern int getpid();
    }

    public abstract class Shape
    {
        public abstract double Area();
        public virtual string Describe() { return "shape"; }
    }

    public class Circle : Shape
    {
        public double Radius;

        public Circle(double r) { Radius = r; }

        public override double Area() { return Radius * Radius * 3.14159; }

        public override string Describe() { return "circle"; }
    }

    public class Box<T>
    {
        public T Item;

        public Box(T item) { Item = item; }

        public T Get() { return Item; }

        public U Map<U>(System.Func<T, U> f) { return f(Item); }
    }

    public class Logger
    {
        public static int InstanceCount;

        static Logger() { InstanceCount = 1; }

        public void Log(string msg) { }
    }

    [System.Flags]
    public enum Permissions
    {
        None = 0,
        Read = 1,
        Write = 2
    }
}
