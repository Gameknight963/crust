/* Numeric helpers embedded in generated C after the basic runtime. */
static inline void crust_rt_print_i64(int64_t value)
{
    printf("%" PRId64 "\n", value);
}

static inline void crust_rt_print_u64(uint64_t value)
{
    printf("%" PRIu64 "\n", value);
}

static inline int64_t crust_rt_cast_signed(int64_t value, int64_t minimum, int64_t maximum, size_t line, size_t column)
{
    if (value < minimum || value > maximum) { crust_rt_fail("integer conversion out of range", line, column); }
    return value;
}

static inline uint64_t crust_rt_cast_unsigned_from_signed(int64_t value, uint64_t maximum, size_t line, size_t column)
{
    if (value < 0 || (uint64_t)value > maximum) { crust_rt_fail("integer conversion out of range", line, column); }
    return (uint64_t)value;
}

static inline int64_t crust_rt_cast_signed_from_unsigned(uint64_t value, int64_t maximum, size_t line, size_t column)
{
    if (value > (uint64_t)maximum) { crust_rt_fail("integer conversion out of range", line, column); }
    return (int64_t)value;
}

static inline uint64_t crust_rt_cast_unsigned(uint64_t value, uint64_t maximum, size_t line, size_t column)
{
    if (value > maximum) { crust_rt_fail("integer conversion out of range", line, column); }
    return value;
}

static inline int64_t crust_rt_i64_add(int64_t a, int64_t b, size_t line, size_t column)
{
    if ((b > 0 && a > INT64_MAX - b) || (b < 0 && a < INT64_MIN - b)) {
        crust_rt_fail("integer overflow", line, column);
    }
    return a + b;
}

static inline int64_t crust_rt_i64_sub(int64_t a, int64_t b, size_t line, size_t column)
{
    if ((b > 0 && a < INT64_MIN + b) || (b < 0 && a > INT64_MAX + b)) {
        crust_rt_fail("integer overflow", line, column);
    }
    return a - b;
}

static inline int64_t crust_rt_i64_mul(int64_t a, int64_t b, size_t line, size_t column)
{
    if (a == 0 || b == 0) { return 0; }
    if ((a == -1 && b == INT64_MIN) || (b == -1 && a == INT64_MIN)) {
        crust_rt_fail("integer overflow", line, column);
    }
    if (a > 0) {
        if ((b > 0 && a > INT64_MAX / b) || (b < 0 && b < INT64_MIN / a)) {
            crust_rt_fail("integer overflow", line, column);
        }
    } else {
        if ((b > 0 && a < INT64_MIN / b) || (b < 0 && a < INT64_MAX / b)) {
            crust_rt_fail("integer overflow", line, column);
        }
    }
    return a * b;
}

static inline int64_t crust_rt_i64_div(int64_t a, int64_t b, size_t line, size_t column)
{
    if (b == 0) { crust_rt_fail("division by zero", line, column); }
    if (a == INT64_MIN && b == -1) { crust_rt_fail("integer overflow", line, column); }
    return a / b;
}

static inline int64_t crust_rt_i64_rem(int64_t a, int64_t b, size_t line, size_t column)
{
    if (b == 0) { crust_rt_fail("remainder by zero", line, column); }
    if (a == INT64_MIN && b == -1) { crust_rt_fail("integer overflow", line, column); }
    return a % b;
}

static inline int64_t crust_rt_i64_neg(int64_t value, size_t line, size_t column)
{
    if (value == INT64_MIN) { crust_rt_fail("integer overflow", line, column); }
    return -value;
}
