#ifndef CRUST_TEST_C_WRAPPERS_H
#define CRUST_TEST_C_WRAPPERS_H
#include <stdatomic.h>
#include <stdint.h>
#include <stdlib.h>
#define FEATURE_TWICE(value) ((value) + (value))
#define FEATURE_TEXT "Crust"
#define FEATURE_SCALE 1.5L
static _Atomic(int) feature_counter = 4;
static _Thread_local int feature_thread_counter;
struct NativeBytes { size_t length; unsigned char bytes[]; };
static int feature_atomic_add(int value) {
    return atomic_fetch_add_explicit(&feature_counter, value, memory_order_relaxed);
}
static int feature_thread_next(void) { return ++feature_thread_counter; }
static struct NativeBytes *feature_bytes_create(size_t length) {
    struct NativeBytes *value = malloc(sizeof(*value) + length);
    if (!value) abort();
    value->length = length;
    for (size_t i=0; i<length; ++i) value->bytes[i] = (unsigned char)(i + 1);
    return value;
}
static int feature_bytes_sum(const struct NativeBytes *value) {
    int result = 0;
    for (size_t i=0; i<value->length; ++i) result += value->bytes[i];
    return result;
}
static void feature_bytes_destroy(struct NativeBytes *value) { free(value); }
static int feature_variable_array(int length) {
    int scratch[length];
    int result = 0;
    for (int i=0; i<length; ++i) { scratch[i] = i + 1; result += scratch[i]; }
    return result;
}
static double feature_complex_sum(double real, double imaginary) {
    double _Complex value;
    __real__ value = real; __imag__ value = imaginary;
    return __real__ value + __imag__ value;
}
static uint64_t feature_wide_carry(uint64_t left, uint64_t right) {
    unsigned __int128 value = (unsigned __int128)left + right;
    return (uint64_t)(value >> 64);
}
static int feature_twice(int value) { return FEATURE_TWICE(value); }
static unsigned char feature_text_first(void) { return FEATURE_TEXT[0]; }
static const char *feature_text(void) { return FEATURE_TEXT; }
static long double feature_scale(void) { return FEATURE_SCALE; }


/* Keep array declarators and anonymous member types inside C. */
struct FeatureAnonymous { struct { int value; } member; };
static int feature_anonymous(int value) {
    struct FeatureAnonymous object = { { value } };
    return object.member.value;
}
static int feature_rows_native(const int (*rows)[3], size_t count) {
    int result = 0;
    for (size_t i = 0; i < count; ++i)
        for (size_t j = 0; j < 3; ++j) result += rows[i][j];
    return result;
}
static int feature_rows(const int *elements, size_t rows) {
    return feature_rows_native((const int (*)[3])elements, rows);
}
static const int *feature_row(void) {
    static const int row[3] = {10, 20, 12};
    return row;
}
/* Extensions stay in C and use the selected native compiler. */
#if defined(_MSC_VER)
__declspec(noinline)
#endif
static int feature_extension(int value) {
#if defined(__GNUC__) && !defined(_MSC_VER)
    __asm__ volatile ("" : "+r" (value));
#endif
    return value;
}
#endif
