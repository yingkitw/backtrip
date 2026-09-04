using System;

namespace Shapes
{
    public class Calculator
    {
        public int Value;

        public Calculator(int value)
        {
            Value = value;
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
}
