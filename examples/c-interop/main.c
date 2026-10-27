// C calls a function written in assembly (dot.s), which calls back into C.
#include <stdio.h>

// Defined in dot.s. C names get a leading underscore: dot is _dot there.
double dot(const double *a, const double *b, long n);

// Called from dot.s for each pair it multiplies.
void report(long i, double product) {
    printf("  pair %ld: %g\n", i, product);
}

int main(void) {
    double a[] = {1.0, 2.0, 3.0, 4.0};
    double b[] = {0.5, 0.25, 2.0, 1.0};
    printf("dot = %g\n", dot(a, b, 4));
    return 0;
}
