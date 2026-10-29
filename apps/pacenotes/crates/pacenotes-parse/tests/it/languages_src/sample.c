#include <stdio.h>
#include "local.h"

#define LIMIT 10
#define SQUARE(x) ((x) * (x))

struct point {
    int x;
    int y;
};

enum color { RED, GREEN };

typedef struct point point_t;

static int helper(int v) {
    return v + LIMIT;
}

int main(int argc, char **argv) {
    printf("%d\n", helper(argc));
    return 0;
}

struct point *make_point(int x, int y);
