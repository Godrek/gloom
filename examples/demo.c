#include <stdio.h>

static void cleanup(void) { puts("cleanup"); }

static int mutually_recursive_b(int n);
static int mutually_recursive_a(int n) {
    return n <= 0 ? 0 : mutually_recursive_b(n - 1);
}
static int mutually_recursive_b(int n) {
    return n <= 0 ? 0 : mutually_recursive_a(n - 1);
}

int main(void) {
    printf("%d\n", mutually_recursive_a(3));
    cleanup();
    return 0;
}
