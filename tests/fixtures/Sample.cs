using System;

namespace Shapes
{
    public class Calculator
    {
        public int Value;
        public const int MaxValue = 100;
        public string Label { get; set; }

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
}
