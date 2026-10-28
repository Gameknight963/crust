#include <inttypes.h>
#include <stdbool.h>
#include <stdio.h>

int crust_wc_report(const char* path, uint64_t lines, uint64_t bytes, uint64_t characters, bool utf8) {
    int result;
    if (utf8) {
        result = printf("%" PRIu64 " %" PRIu64 " %" PRIu64 " %s\n", lines, bytes, characters, path);
    } else {
        result = printf("%" PRIu64 " %" PRIu64 " %s\n", lines, bytes, path);
    }
    return result < 0 || fflush(stdout) != 0 ? -1 : 0;
}

void crust_wc_error(const char* subject, int code) {
    const char* message = "operation failed";
    switch (code) {
        case 0: message = "could not open file"; break;
        case 1: message = "could not read file"; break;
        case 2: case 102: message = "allocation failed"; break;
        case 3: message = "invalid UTF-8"; break;
        case 4: message = "could not close file"; break;
        case 5: message = "could not write output"; break;
        case 100: message = "unknown option"; break;
        case 101: message = "no input files"; break;
    }
    if (subject != NULL) { fprintf(stderr, "%s: ", subject); }
    fprintf(stderr, "%s\n", message);
}
