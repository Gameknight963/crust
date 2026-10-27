#include "file.h"
#include <stdio.h>

void* crust_file_temporary(void) { return tmpfile(); }
int crust_file_close(void* handle) { return fclose((FILE*)handle); }
int crust_file_read_byte(void* handle) {
    int value = fgetc((FILE*)handle);
    return value == EOF ? -1 : value;
}
int crust_file_write_byte(void* handle, unsigned char value) {
    return fputc(value, (FILE*)handle) == EOF ? -1 : 0;
}
int crust_file_flush(void* handle) { return fflush((FILE*)handle); }
int crust_file_rewind(void* handle) {
    int result = fseek((FILE*)handle, 0, SEEK_SET);
    if (result == 0) { clearerr((FILE*)handle); }
    return result;
}
int crust_file_sync_position(void* handle) { return fseek((FILE*)handle, 0, SEEK_CUR); }
int crust_file_eof(void* handle) { return feof((FILE*)handle); }
int crust_file_error(void* handle) { return ferror((FILE*)handle); }
