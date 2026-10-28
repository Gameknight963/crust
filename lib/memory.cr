extern void* allocate_aligned(size_t bytes, size_t alignment) = "crust_rt_allocate_aligned";
extern void deallocate_aligned(void* pointer, size_t alignment) = "crust_rt_deallocate_aligned";
extern void* malloc(size_t bytes);
extern void free(void* pointer);
extern void abort();

