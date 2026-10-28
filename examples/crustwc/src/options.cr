import "../../../lib/vector.cr";

enum class CommandError { UnknownOption, NoInput, Allocation };
struct CommandFailure { CommandError code; const char* argument; };
struct Options {
    bool utf8;
    bool help;
    Vector<const char*> paths;
};

extern int strcmp(const char* left, const char* right);
typedef char OptionText[16];

unsafe bool option_is(const char* argument, OptionText expected) {
    return strcmp(argument, &expected[0]) == 0;
}

unsafe Result<Options, CommandFailure> parse_options(int argc, char** argv) {
    Options options = { .utf8 = false, .help = false, .paths = Vector<const char*>::create() };
    bool literal_paths = false;
    for (int i = 1; i < argc; i++) {
        const char* argument = argv[i];
        if (!literal_paths && option_is(argument, "--")) { literal_paths = true; continue; }
        if (!literal_paths && option_is(argument, "--utf8")) { options.utf8 = true; continue; }
        if (!literal_paths && option_is(argument, "--help")) {
            options.help = true;
            return Result<Options, CommandFailure>::success(move(options));
        }
        if (!literal_paths && argument[0] == '-') {
            return Result<Options, CommandFailure>::failure(
                (CommandFailure){ .code = CommandError::UnknownOption, .argument = argument });
        }
        Result<Unit, AllocationFailure<const char*>> added = options.paths.try_push(argument);
        if (!added.is_ok()) {
            return Result<Options, CommandFailure>::failure(
                (CommandFailure){ .code = CommandError::Allocation, .argument = 0 });
        }
    }
    if (options.paths.count() == 0) {
        return Result<Options, CommandFailure>::failure(
            (CommandFailure){ .code = CommandError::NoInput, .argument = 0 });
    }
    return Result<Options, CommandFailure>::success(move(options));
}
