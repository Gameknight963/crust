/* Numeric helpers embedded in generated C after the basic runtime. */
static inline void crust_rt_shift_count(uint64_t crust_count, unsigned crust_bits, const char *crust_file, size_t crust_line, size_t crust_column)
{
    if (crust_count >= crust_bits) { crust_rt_fail("shift count out of range", crust_file, crust_line, crust_column); }
}

static inline int64_t crust_rt_shift_left_signed(int64_t crust_value, uint64_t crust_count, unsigned crust_bits, const char *crust_file, size_t crust_line, size_t crust_column)
{
    crust_rt_shift_count(crust_count, crust_bits, crust_file, crust_line, crust_column);
    uint64_t crust_maximum = crust_bits == 32 ? INT32_MAX : INT64_MAX;
    uint64_t crust_magnitude = crust_value < 0 ? (uint64_t)(-(crust_value + 1)) + UINT64_C(1) : (uint64_t)crust_value;
    uint64_t crust_limit = crust_maximum + (crust_value < 0 ? UINT64_C(1) : UINT64_C(0));
    if (crust_magnitude > (crust_limit >> crust_count)) { crust_rt_fail("integer overflow", crust_file, crust_line, crust_column); }
    crust_magnitude <<= crust_count;
    return crust_value < 0 ? -(int64_t)(crust_magnitude - 1) - 1 : (int64_t)crust_magnitude;
}

static inline uint64_t crust_rt_shift_left_unsigned(uint64_t crust_value, uint64_t crust_count, unsigned crust_bits, const char *crust_file, size_t crust_line, size_t crust_column)
{
    crust_rt_shift_count(crust_count, crust_bits, crust_file, crust_line, crust_column);
    uint64_t crust_maximum = crust_bits == 32 ? UINT32_MAX : UINT64_MAX;
    return (crust_value << crust_count) & crust_maximum;
}

static inline int64_t crust_rt_shift_right_signed(int64_t crust_value, uint64_t crust_count, unsigned crust_bits, const char *crust_file, size_t crust_line, size_t crust_column)
{
    crust_rt_shift_count(crust_count, crust_bits, crust_file, crust_line, crust_column);
    return crust_value < 0 ? -1 - ((-(crust_value + 1)) >> crust_count) : crust_value >> crust_count;
}

static inline uint64_t crust_rt_shift_right_unsigned(uint64_t crust_value, uint64_t crust_count, unsigned crust_bits, const char *crust_file, size_t crust_line, size_t crust_column)
{
    crust_rt_shift_count(crust_count, crust_bits, crust_file, crust_line, crust_column);
    return crust_value >> crust_count;
}

static inline void crust_rt_print_span(const void *crust_text, size_t crust_length)
{
    if (crust_length != 0) { fwrite(crust_text, 1, crust_length, stdout); }
    fputc('\n', stdout);
}

static inline void crust_rt_print_bytes(const void *crust_text, size_t crust_capacity)
{
    const uint8_t *crust_bytes = (const uint8_t *)crust_text;
    size_t crust_length = 0;
    while (crust_length < crust_capacity && crust_bytes[crust_length] != 0) { ++crust_length; }
    fwrite(crust_bytes, 1, crust_length, stdout);
    fputc('\n', stdout);
}

static inline void crust_rt_print_volatile_bytes(const volatile void *crust_text, size_t crust_capacity)
{
    const volatile uint8_t *crust_bytes = (const volatile uint8_t *)crust_text;
    for (size_t crust_index = 0; crust_index < crust_capacity; ++crust_index) {
        uint8_t crust_value = crust_bytes[crust_index];
        if (crust_value == 0) { break; }
        fputc(crust_value, stdout);
    }
    fputc('\n', stdout);
}

static inline size_t crust_rt_index_unsigned(uint64_t crust_index, size_t crust_length, const char *crust_file, size_t crust_line, size_t crust_column)
{
    if (crust_index >= crust_length) { crust_rt_fail("array index out of bounds", crust_file, crust_line, crust_column); }
    return (size_t)crust_index;
}

static inline size_t crust_rt_index_signed(int64_t crust_index, size_t crust_length, const char *crust_file, size_t crust_line, size_t crust_column)
{
    if (crust_index < 0) { crust_rt_fail("array index out of bounds", crust_file, crust_line, crust_column); }
    return crust_rt_index_unsigned((uint64_t)crust_index, crust_length, crust_file, crust_line, crust_column);
}

static inline void crust_rt_print_i64(int64_t crust_value)
{
    printf("%" PRId64 "\n", crust_value);
}

static inline void crust_rt_print_u64(uint64_t crust_value)
{
    printf("%" PRIu64 "\n", crust_value);
}

static inline int64_t crust_rt_cast_signed(int64_t crust_value, int64_t crust_minimum, int64_t crust_maximum, const char *crust_file, size_t crust_line, size_t crust_column)
{
    if (crust_value < crust_minimum || crust_value > crust_maximum) { crust_rt_fail("integer conversion out of range", crust_file, crust_line, crust_column); }
    return crust_value;
}

static inline uint64_t crust_rt_cast_unsigned_from_signed(int64_t crust_value, uint64_t crust_maximum, const char *crust_file, size_t crust_line, size_t crust_column)
{
    if (crust_value < 0 || (uint64_t)crust_value > crust_maximum) { crust_rt_fail("integer conversion out of range", crust_file, crust_line, crust_column); }
    return (uint64_t)crust_value;
}

static inline int64_t crust_rt_cast_signed_from_unsigned(uint64_t crust_value, int64_t crust_maximum, const char *crust_file, size_t crust_line, size_t crust_column)
{
    if (crust_value > (uint64_t)crust_maximum) { crust_rt_fail("integer conversion out of range", crust_file, crust_line, crust_column); }
    return (int64_t)crust_value;
}

static inline uint64_t crust_rt_cast_unsigned(uint64_t crust_value, uint64_t crust_maximum, const char *crust_file, size_t crust_line, size_t crust_column)
{
    if (crust_value > crust_maximum) { crust_rt_fail("integer conversion out of range", crust_file, crust_line, crust_column); }
    return crust_value;
}

static inline int64_t crust_rt_i64_add(int64_t crust_a, int64_t crust_b, const char *crust_file, size_t crust_line, size_t crust_column)
{
    if ((crust_b > 0 && crust_a > INT64_MAX - crust_b) || (crust_b < 0 && crust_a < INT64_MIN - crust_b)) {
        crust_rt_fail("integer overflow", crust_file, crust_line, crust_column);
    }
    return crust_a + crust_b;
}

static inline int64_t crust_rt_i64_sub(int64_t crust_a, int64_t crust_b, const char *crust_file, size_t crust_line, size_t crust_column)
{
    if ((crust_b > 0 && crust_a < INT64_MIN + crust_b) || (crust_b < 0 && crust_a > INT64_MAX + crust_b)) {
        crust_rt_fail("integer overflow", crust_file, crust_line, crust_column);
    }
    return crust_a - crust_b;
}

static inline int64_t crust_rt_i64_mul(int64_t crust_a, int64_t crust_b, const char *crust_file, size_t crust_line, size_t crust_column)
{
    if (crust_a == 0 || crust_b == 0) { return 0; }
    if ((crust_a == -1 && crust_b == INT64_MIN) || (crust_b == -1 && crust_a == INT64_MIN)) {
        crust_rt_fail("integer overflow", crust_file, crust_line, crust_column);
    }
    if (crust_a > 0) {
        if ((crust_b > 0 && crust_a > INT64_MAX / crust_b) || (crust_b < 0 && crust_b < INT64_MIN / crust_a)) {
            crust_rt_fail("integer overflow", crust_file, crust_line, crust_column);
        }
    } else {
        if ((crust_b > 0 && crust_a < INT64_MIN / crust_b) || (crust_b < 0 && crust_a < INT64_MAX / crust_b)) {
            crust_rt_fail("integer overflow", crust_file, crust_line, crust_column);
        }
    }
    return crust_a * crust_b;
}

static inline int64_t crust_rt_i64_div(int64_t crust_a, int64_t crust_b, const char *crust_file, size_t crust_line, size_t crust_column)
{
    if (crust_b == 0) { crust_rt_fail("division by zero", crust_file, crust_line, crust_column); }
    if (crust_a == INT64_MIN && crust_b == -1) { crust_rt_fail("integer overflow", crust_file, crust_line, crust_column); }
    return crust_a / crust_b;
}

static inline int64_t crust_rt_i64_rem(int64_t crust_a, int64_t crust_b, const char *crust_file, size_t crust_line, size_t crust_column)
{
    if (crust_b == 0) { crust_rt_fail("remainder by zero", crust_file, crust_line, crust_column); }
    if (crust_a == INT64_MIN && crust_b == -1) { crust_rt_fail("integer overflow", crust_file, crust_line, crust_column); }
    return crust_a % crust_b;
}

static inline int64_t crust_rt_i64_neg(int64_t crust_value, const char *crust_file, size_t crust_line, size_t crust_column)
{
    if (crust_value == INT64_MIN) { crust_rt_fail("integer overflow", crust_file, crust_line, crust_column); }
    return -crust_value;
}
