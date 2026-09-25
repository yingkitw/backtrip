package demo;

public class Box<T> {
    private T value;
    private java.util.List<T> items;

    public Box(T value) {
        this.value = value;
    }

    public T getValue() {
        return this.value;
    }

    public void setValue(T value) {
        this.value = value;
    }

    public java.util.Map<String, T> asMap() {
        java.util.Map<String, T> m = new java.util.HashMap<>();
        m.put("value", this.value);
        return m;
    }

    public <U> java.util.List<U> map(java.util.function.Function<? super T, ? extends U> f) {
        java.util.List<U> out = new java.util.ArrayList<>();
        for (T t : this.items) {
            out.add(f.apply(t));
        }
        return out;
    }
}
