#ifndef CRUST_FILE_H
#define CRUST_FILE_H

void* crust_file_temporary(void);
int crust_file_close(void* handle);
int crust_file_read_byte(void* handle);
int crust_file_write_byte(void* handle, unsigned char value);
int crust_file_flush(void* handle);
int crust_file_rewind(void* handle);
int crust_file_sync_position(void* handle);
int crust_file_eof(void* handle);
int crust_file_error(void* handle);

#endif
