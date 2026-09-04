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

            public Settings(string label)
            {
                Label = label;
                Enabled = true;
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
}
