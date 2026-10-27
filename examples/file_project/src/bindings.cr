extern void* crust_file_temporary();
extern int crust_file_close(void*);
extern int crust_file_read_byte(void*);
extern int crust_file_write_byte(void*, unsigned char);
extern int crust_file_flush(void*);
extern int crust_file_rewind(void*);
extern int crust_file_sync_position(void*);
extern int crust_file_eof(void*);
extern int crust_file_error(void*);
