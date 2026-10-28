import "options.cr";
import "stats.cr";

extern int crust_wc_report(const char* path, uint64_t lines, uint64_t bytes, uint64_t characters, bool utf8);
extern void crust_wc_error(const char* subject, int code);

void usage() {
    print("Usage: crustwc [--utf8] [--] FILE...");
    print("Print newline and byte counts. --utf8 also counts Unicode scalar values.");
}

int main(int argc, char** argv) {
    unsafe {
        Result<Options, CommandFailure> parsed = parse_options(argc, argv);
        if (!parsed.is_ok()) {
            CommandFailure failure = parsed.error_value();
            crust_wc_error(failure.argument, 100 + (int)failure.code);
            return 2;
        }
        Options options = move(parsed).take_value();
        if (options.help) { usage(); return 0; }
        int status = 0;
        for (size_t i = 0; i < options.paths.count(); i++) {
            const char* path = options.paths.at(i);
            Result<File, FileError> opened = open_input(path);
            if (!opened.is_ok()) { crust_wc_error(path, (int)ReadError::Open); status = 1; continue; }
            File input = move(opened).take_value();
            Result<Stats, ReadError> counted = read_stats(&input, options.utf8);
            if (!counted.is_ok()) { crust_wc_error(path, (int)counted.error_value()); status = 1; continue; }
            Stats stats = counted.value();
            if (crust_wc_report(path, stats.lines, stats.bytes, stats.characters, options.utf8) != 0) {
                crust_wc_error(0, 5); status = 1; break;
            }
        }
        return status;
    }
}
