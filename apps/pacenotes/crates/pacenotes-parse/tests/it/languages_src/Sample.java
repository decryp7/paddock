package sample;

import java.util.List;
import static java.lang.Math.max;

public class Sample {
    private int count;
    public static final int LIMIT = 3;

    public Sample(int count) {
        this.count = count;
    }

    public int total(List<Integer> xs) {
        int local = 0;
        for (int x : xs) {
            local += max(x, 1);
        }
        return helper(local);
    }

    private int helper(int v) {
        return v + count;
    }

    interface Visitor {
        void visit(Sample s);
    }

    enum Mode { FAST, SLOW }

    record Pair(int a, int b) {}
}
