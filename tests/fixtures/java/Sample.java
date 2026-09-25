package demo;

public class Sample {

    public static final int MAX = 100;
    private int count;
    private String name;

    public Sample(String name) {
        this.name = name;
        this.count = 0;
    }

    public int add(int a, int b) {
        return a + b;
    }

    public int abs(int x) {
        if (x < 0) {
            return -x;
        }
        return x;
    }

    public int max(int a, int b) {
        if (a > b) {
            return a;
        } else {
            return b;
        }
    }

    public int ternary(int x) {
        return x >= 0 ? x : -x;
    }

    public int sumUpTo(int n) {
        int total = 0;
        int i = 1;
        while (i <= n) {
            total = total + i;
            i = i + 1;
        }
        return total;
    }

    public int sumFor(int n) {
        int total = 0;
        for (int i = 0; i < n; i++) {
            total += i;
        }
        return total;
    }

    public int factorial(int n) {
        int result = 1;
        for (int i = 2; i <= n; i++) {
            result *= i;
        }
        return result;
    }

    public String greet(String who) {
        return "Hello, " + who + "!";
    }

    public String dayName(int day) {
        switch (day) {
            case 0:
                return "Sunday";
            case 6:
                return "Saturday";
            default:
                return "Weekday";
        }
    }

    public int sumArray(int[] xs) {
        int total = 0;
        for (int i = 0; i < xs.length; i++) {
            total += xs[i];
        }
        return total;
    }

    public int divide(int a, int b) {
        try {
            return a / b;
        } catch (ArithmeticException e) {
            return -1;
        } finally {
            this.count++;
        }
    }

    public boolean isPositive(int x) {
        return x > 0;
    }

    public static int staticDouble(int x) {
        return x * 2;
    }

    public int getCount() {
        return this.count;
    }

    public String getName() {
        return this.name;
    }

    public void setCount(int count) {
        this.count = count;
    }

    public int clamp(int v, int lo, int hi) {
        if (v < lo) {
            return lo;
        }
        if (v > hi) {
            return hi;
        }
        return v;
    }
}
