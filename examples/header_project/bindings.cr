extern header "native.h";
extern enum NativeStatus : int32_t {
    NativeReady = 1,
    NativeError = -1,
};
struct NativeCounter;
extern struct NativeCounter* native_create(int);
extern int native_value(const struct NativeCounter*);
extern NativeStatus native_status(const struct NativeCounter*);
extern void native_add(struct NativeCounter*, int);
extern void native_destroy(struct NativeCounter*);
extern const int native_limit;
