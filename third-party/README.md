# Third-party runtime

Compiler archives include libclang and, when required, its shared libLLVM
dependency. These libraries come from the LLVM project and use Apache-2.0 with
LLVM exceptions. Their license is included in [llvm/LICENSE.TXT](llvm/LICENSE.TXT).

The runtime is used through libclang's stable C API. Crust's generated programs
are compiled by the selected C compiler and do not link to libclang.

The `CRUST_LIBCLANG` environment variable selects a library when building from
source. Packaged compiler binaries search their own `bin` directory first.
