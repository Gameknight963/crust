extern header "native.h";
extern struct Point {
    int x;
    int y;
};
extern struct Sample {
    unsigned char tag;
    int value;
};
extern struct Point translate(struct Point, int);
