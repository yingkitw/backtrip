package demo;

public class Sync {
    private int count = 0;
    private final Object lock = new Object();

    public synchronized void inc() {
        this.count++;
    }

    public void add(int n) {
        synchronized (this.lock) {
            this.count += n;
        }
    }

    public int get() {
        synchronized (this.lock) {
            return this.count;
        }
    }
}
