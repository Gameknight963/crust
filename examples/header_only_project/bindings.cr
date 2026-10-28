extern header "native.h";
struct NativeSample;
extern struct NativeSample* sample_create(unsigned int);
extern unsigned int sample_value(const struct NativeSample*);
extern void sample_destroy(struct NativeSample*);
extern unsigned int sample_count;
