/* Numeric helpers embedded in generated C after the basic runtime. */
static inline void crust_rt_shift_count(uint64_t count, unsigned bits, const char *crust_file, size_t line, size_t column)
{
    if (count >= bits) { crust_rt_fail("shift count out of range", crust_file, line, column); }
}

static inline int64_t crust_rt_shift_left_signed(int64_t value, uint64_t count, unsigned bits, const char *crust_file, size_t line, size_t column)
{
    crust_rt_shift_count(count, bits, crust_file, line, column);
    uint64_t maximum = bits == 32 ? INT32_MAX : INT64_MAX;
    uint64_t magnitude = value < 0 ? (uint64_t)(-(value + 1)) + UINT64_C(1) : (uint64_t)value;
    uint64_t limit = maximum + (value < 0 ? UINT64_C(1) : UINT64_C(0));
    if (magnitude > (limit >> count)) { crust_rt_fail("integer overflow", crust_file, line, column); }
    magnitude <<= count;
    return value < 0 ? -(int64_t)(magnitude - 1) - 1 : (int64_t)magnitude;
}

static inline uint64_t crust_rt_shift_left_unsigned(uint64_t value, uint64_t count, unsigned bits, const char *crust_file, size_t line, size_t column)
{
    crust_rt_shift_count(count, bits, crust_file, line, column);
    uint64_t maximum = bits == 32 ? UINT32_MAX : UINT64_MAX;
    return (value << count) & maximum;
}

static inline int64_t crust_rt_shift_right_signed(int64_t value, uint64_t count, unsigned bits, const char *crust_file, size_t line, size_t column)
{
    crust_rt_shift_count(count, bits, crust_file, line, column);
    return value < 0 ? -1 - ((-(value + 1)) >> count) : value >> count;
}

static inline uint64_t crust_rt_shift_right_unsigned(uint64_t value, uint64_t count, unsigned bits, const char *crust_file, size_t line, size_t column)
{
    crust_rt_shift_count(count, bits, crust_file, line, column);
    return value >> count;
}

static inline void crust_rt_print_span(const void *text, size_t length)
{
    if (length != 0) { fwrite(text, 1, length, stdout); }
    fputc('\n', stdout);
}

static inline void crust_rt_print_bytes(const void *text, size_t capacity)
{
    const uint8_t *bytes = (const uint8_t *)text;
    size_t length = 0;
    while (length < capacity && bytes[length] != 0) { ++length; }
    fwrite(bytes, 1, length, stdout);
    fputc('\n', stdout);
}

static inline void crust_rt_print_volatile_bytes(const volatile void *text, size_t capacity)
{
    const volatile uint8_t *bytes = (const volatile uint8_t *)text;
    for (size_t index = 0; index < capacity; ++index) {
        uint8_t value = bytes[index];
        if (value == 0) { break; }
        fputc(value, stdout);
    }
    fputc('\n', stdout);
}

static inline size_t crust_rt_index_unsigned(uint64_t index, size_t length, const char *crust_file, size_t line, size_t column)
{
    if (index >= length) { crust_rt_fail("array index out of bounds", crust_file, line, column); }
    return (size_t)index;
}

static inline size_t crust_rt_index_signed(int64_t index, size_t length, const char *crust_file, size_t line, size_t column)
{
    if (index < 0) { crust_rt_fail("array index out of bounds", crust_file, line, column); }
    return crust_rt_index_unsigned((uint64_t)index, length, crust_file, line, column);
}

static inline void crust_rt_print_i64(int64_t value)
{
    printf("%" PRId64 "\n", value);
}

static inline void crust_rt_print_u64(uint64_t value)
{
    printf("%" PRIu64 "\n", value);
}

static inline int64_t crust_rt_cast_signed(int64_t value, int64_t minimum, int64_t maximum, const char *crust_file, size_t line, size_t column)
{
    if (value < minimum || value > maximum) { crust_rt_fail("integer conversion out of range", crust_file, line, column); }
    return value;
}

static inline uint64_t crust_rt_cast_unsigned_from_signed(int64_t value, uint64_t maximum, const char *crust_file, size_t line, size_t column)
{
    if (value < 0 || (uint64_t)value > maximum) { crust_rt_fail("integer conversion out of range", crust_file, line, column); }
    return (uint64_t)value;
}

static inline int64_t crust_rt_cast_signed_from_unsigned(uint64_t value, int64_t maximum, const char *crust_file, size_t line, size_t column)
{
    if (value > (uint64_t)maximum) { crust_rt_fail("integer conversion out of range", crust_file, line, column); }
    return (int64_t)value;
}

static inline uint64_t crust_rt_cast_unsigned(uint64_t value, uint64_t maximum, const char *crust_file, size_t line, size_t column)
{
    if (value > maximum) { crust_rt_fail("integer conversion out of range", crust_file, line, column); }
    return value;
}

static inline int64_t crust_rt_i64_add(int64_t a, int64_t b, const char *crust_file, size_t line, size_t column)
{
    if ((b > 0 && a > INT64_MAX - b) || (b < 0 && a < INT64_MIN - b)) {
        crust_rt_fail("integer overflow", crust_file, line, column);
    }
    return a + b;
}

static inline int64_t crust_rt_i64_sub(int64_t a, int64_t b, const char *crust_file, size_t line, size_t column)
{
    if ((b > 0 && a < INT64_MIN + b) || (b < 0 && a > INT64_MAX + b)) {
        crust_rt_fail("integer overflow", crust_file, line, column);
    }
    return a - b;
}

static inline int64_t crust_rt_i64_mul(int64_t a, int64_t b, const char *crust_file, size_t line, size_t column)
{
    if (a == 0 || b == 0) { return 0; }
    if ((a == -1 && b == INT64_MIN) || (b == -1 && a == INT64_MIN)) {
        crust_rt_fail("integer overflow", crust_file, line, column);
    }
    if (a > 0) {
        if ((b > 0 && a > INT64_MAX / b) || (b < 0 && b < INT64_MIN / a)) {
            crust_rt_fail("integer overflow", crust_file, line, column);
        }
    } else {
        if ((b > 0 && a < INT64_MIN / b) || (b < 0 && a < INT64_MAX / b)) {
            crust_rt_fail("integer overflow", crust_file, line, column);
        }
    }
    return a * b;
}

static inline int64_t crust_rt_i64_div(int64_t a, int64_t b, const char *crust_file, size_t line, size_t column)
{
    if (b == 0) { crust_rt_fail("division by zero", crust_file, line, column); }
    if (a == INT64_MIN && b == -1) { crust_rt_fail("integer overflow", crust_file, line, column); }
    return a / b;
}

static inline int64_t crust_rt_i64_rem(int64_t a, int64_t b, const char *crust_file, size_t line, size_t column)
{
    if (b == 0) { crust_rt_fail("remainder by zero", crust_file, line, column); }
    if (a == INT64_MIN && b == -1) { crust_rt_fail("integer overflow", crust_file, line, column); }
    return a % b;
}

static inline int64_t crust_rt_i64_neg(int64_t value, const char *crust_file, size_t line, size_t column)
{
    if (value == INT64_MIN) { crust_rt_fail("integer overflow", crust_file, line, column); }
    return -value;
}
