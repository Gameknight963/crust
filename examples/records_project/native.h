#ifndef CRUST_EXAMPLE_RECORDS_H
#define CRUST_EXAMPLE_RECORDS_H

struct Point { int x; int y; };
struct Readings { const int id; volatile int updates; int values[3]; };
#pragma pack(push, 1)
struct Sample { unsigned char tag; int value; };
#pragma pack(pop)

static inline struct Point translate(struct Point value, int amount) {
    value.x += amount;
    value.y += amount;
    return value;
}

#endif
