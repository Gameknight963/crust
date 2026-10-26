mod common;

#[test]
fn explicit_cdecl_matches_default_function_and_callback_types() {
    let source = r#"
        typedef int (__cdecl *Callback)(int);
        typedef int __cdecl Operation(int);
        int __cdecl increment(int value) { return value + 1; }
        int main() {
            Callback first = increment;
            Operation* second = first;
            unsafe { print(second(41)); }
            return 0;
        }
    "#;
    let output = common::tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
}

#[cfg(all(windows, any(target_arch = "x86", target_arch = "x86_64")))]
#[test]
fn native_calls_callbacks_static_methods_and_generic_functions_preserve_conventions() {
    let source = r#"
        typedef int (__vectorcall *Callback)(int);
        extern int __vectorcall apply_native(Callback callback, int value) = "native_apply";
        T __vectorcall identity<T>(T value) { return value; }
        struct Operations {
            int unused;
            static int __vectorcall twice(int value) { return value * 2; }
        };
        int main() {
            Callback first = identity<int>;
            Callback second = Operations::twice;
            Callback third = &apply_native_result;
            unsafe {
                print(apply_native(first, 40));
                print(apply_native(second, 21));
                print(third(10));
            }
            return 0;
        }
        int __vectorcall apply_native_result(int value) { return value + 1; }
    "#;
    let mut generated = crust::compile(source).unwrap();
    generated.push_str(
        r#"
        typedef int (__vectorcall *NativeCallback)(int);
        int __vectorcall native_apply(NativeCallback callback, int value) {
            return callback(value) + 1;
        }
    "#,
    );
    let output = common::tracked_generated_c(&generated, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "41\n43\n11\n"
    );
}

#[cfg(all(windows, any(target_arch = "x86", target_arch = "x86_64")))]
#[test]
fn incompatible_callback_and_native_symbol_conventions_are_rejected() {
    for source in [
        "typedef int (__vectorcall *Callback)(int); int ordinary(int n) { return n; } int main() { Callback callback = ordinary; return 0; }",
        "extern int __vectorcall first(int) = \"native\"; extern int second(int) = \"native\"; int main() { return 0; }",
        "typedef int __stdcall (__cdecl *Callback)(int); int main() { return 0; }",
        "extern int __vectorcall variadic(int, ...); int main() { return 0; }",
        "typedef int (__vectorcall *Callback)(int, ...); int main() { return 0; }",
        "int __vectorcall main() { return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "{source}");
    }
}

#[cfg(all(windows, any(target_arch = "x86", target_arch = "x86_64")))]
#[test]
fn interface_requirements_check_calling_conventions() {
    let source = r#"
        static interface Operation { int __vectorcall apply(int value); };
        struct Good { int unused; static int __vectorcall apply(int n) { return n + 1; } };
        int invoke<T: Operation>(int n) { return T::apply(n); }
        int main() { return invoke<Good>(1); }
    "#;
    crust::check(source).unwrap();
    let incompatible = source.replace("static int __vectorcall apply", "static int apply");
    assert!(
        crust::check(&incompatible)
            .unwrap_err()
            .message
            .contains("does not satisfy")
    );
}

#[cfg(windows)]
#[test]
fn variadic_stdcall_and_fastcall_use_c_variadic_abi() {
    crust::check(
        r#"
        typedef int (*Callback)(int, ...);
        extern int __stdcall first(int, ...);
        extern int __fastcall second(int, ...);
        int main() { Callback a = first; Callback b = second; return 0; }
    "#,
    )
    .unwrap();
}

#[cfg(all(windows, not(target_arch = "x86")))]
#[test]
fn stdcall_and_fastcall_match_c_when_ignored_by_the_target_abi() {
    crust::check(
        r#"
        typedef int (*Callback)(int);
        extern int __stdcall native(int);
        extern int __fastcall native(int);
        int __stdcall first(int n) { return n; }
        int __fastcall second(int n) { return n; }
        int main() { Callback a = first; Callback b = second; Callback c = native; return 0; }
    "#,
    )
    .unwrap();
}

#[cfg(not(windows))]
#[test]
fn windows_calling_conventions_require_a_windows_target() {
    for convention in ["__stdcall", "__fastcall", "__vectorcall"] {
        let source = format!("extern int {convention} native(int); int main() {{ return 0; }}");
        assert!(
            crust::check(&source)
                .unwrap_err()
                .message
                .contains("Windows target")
        );
    }
}

#[test]
fn calling_conventions_apply_to_functions_and_cannot_repeat() {
    for source in [
        "typedef int __cdecl Count; int main() { return 0; }",
        "struct Item { int __cdecl value; }; int main() { return 0; }",
        "int __cdecl __cdecl main() { return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "{source}");
    }
}
