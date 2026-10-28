#ifndef CRUST_EXAMPLE_HEADER_ONLY_H
#define CRUST_EXAMPLE_HEADER_ONLY_H

#include <stdint.h>
#include <stdlib.h>

#pragma pack(push, 1)
struct NativeSample {
    uint8_t tag;
    uint32_t value;
    unsigned char flags : 3;
};
#pragma pack(pop)

static unsigned sample_count;

static struct NativeSample *sample_create(uint32_t value) {
    struct NativeSample *sample = malloc(sizeof(*sample));
    if (sample == NULL) abort();
    sample->tag = 7;
    sample->value = value;
    sample->flags = 5;
    sample_count += 1;
    return sample;
}

static inline uint32_t sample_value(const struct NativeSample *sample) {
    return sample->value + sample->tag + sample->flags;
}

static void sample_destroy(struct NativeSample *sample) {
    free(sample);
}

#endif
