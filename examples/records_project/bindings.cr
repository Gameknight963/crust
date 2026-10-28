extern header "native.h";
extern struct Point {
    int x;
    int y;
};
extern struct Sample {
    unsigned char tag;
    int value;
};
extern struct Readings {
    const int id;
    volatile int updates;
    int values[3];
};
extern struct Point translate(struct Point, int);
