#include <vector>
#include "local.hpp"

namespace shapes {

class Rect {
public:
    Rect(int w, int h);
    int area() const { return w_ * h_; }
private:
    int w_;
    int h_;
};

struct Pair {
    int a;
    int b;
};

int twice(int v) {
    return v * 2;
}

}  // namespace shapes

int shapes::Rect::perimeter() const {
    return 2 * (w_ + h_);
}

using Numbers = std::vector<int>;
