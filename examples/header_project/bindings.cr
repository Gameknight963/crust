struct NativeCounter;
extern struct NativeCounter* native_create(int);
extern int native_value(const struct NativeCounter*);
extern void native_add(struct NativeCounter*, int);
extern void native_destroy(struct NativeCounter*);
