use std::path::Path;

fn random(state: &mut u64) -> usize {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    return *state as usize;
}

fn main() {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let iterations: usize = arguments
        .first()
        .map_or(10_000, |value| value.parse().unwrap());
    let seed: u64 = arguments.get(1).map_or(42, |value| value.parse().unwrap());
    assert_ne!(seed, 0, "seed must be nonzero");
    let mut library = [
        include_str!("../lib/memory.cr"),
        include_str!("../lib/result.cr"),
        include_str!("../lib/allocation.cr"),
        include_str!("../lib/box.cr"),
        include_str!("../lib/vector.cr"),
        include_str!("../lib/bytes.cr"),
        include_str!("../lib/string.cr"),
    ]
    .join("\n");
    for dependency in ["memory", "result", "allocation", "vector", "bytes"] {
        library = library.replace(&format!("import \"{dependency}.cr\";"), "");
    }
    library.push_str(r#"
        int main() {
            char text[6] = "hello";
            Result<String, TextError> copied = String::try_from_c_string(&text);
            if (!copied.is_ok()) return 1;
            String value = move(copied).take_value();
            print(value.view());
            Vector<Box<int>> values = Vector<Box<int>>::create();
            Result<Unit, AllocationFailure<Box<int>>> inserted = values.try_push(Box<int>::create(42));
            if (!inserted.is_ok()) return 1;
            print(values.at(0).get());
            return 0;
        }
    "#);
    crust::check(&library).expect("library mutation seed must compile");
    let corpus = [
        include_str!("hello.cr"),
        include_str!("methods.cr"),
        include_str!("destructors.cr"),
        include_str!("generic_box.cr"),
        include_str!("strings.cr"),
        "int sum(const int[]& s) { int n = 0; for (size_t i = 0; i < s.length; i++) n += s[i]; return n; } int main() { int[2] a = {1,2}; return sum(&a); }",
        "static T identity<T>(T value) { return value; } int main() { return identity<int>(1); }",
        "const int[2] values = {1,2}; static int count; int main() { static int calls = 1; unsafe { count += calls; return count; } }",
        "int& choose(int& a, int& b) borrows(a,b) { if (a > b) return a; return b; } int main() { int a=1; int b=2; print(choose(&a,&b)); return 0; }",
        "tagged union Value { double number; int integer; }; int main() { Value value = {.number=1.5}; switch(value) { case number: print(value.number); break; case integer: print(value.integer); break; } return 0; }",
        "int main() { uint8_t[3] bytes={65,0,66}; const uint8_t[]& view=&bytes; print(view); return 0; }",
        library.as_str(),
    ];
    let fragments: &[&[u8]] = &[
        b"{}",
        b"[]&",
        b"<int>",
        b"static ",
        b"unsafe ",
        b"/*",
        b"\"",
        b";",
        b"\0",
        "λ".as_bytes(),
        b"move(",
        b"return ",
    ];
    let directory = Path::new("target/fuzz-frontend");
    std::fs::create_dir_all(directory).unwrap();
    let current = directory.join("current.cr");
    let mut state = seed;
    for iteration in 0..iterations {
        let mut bytes = corpus[random(&mut state) % corpus.len()]
            .as_bytes()
            .to_vec();
        for _ in 0..(1 + random(&mut state) % 8) {
            let position = random(&mut state) % (bytes.len() + 1);
            match random(&mut state) % 3 {
                0 if position < bytes.len() => {
                    bytes.remove(position);
                }
                1 if position < bytes.len() => {
                    bytes[position] = random(&mut state) as u8;
                }
                _ => {
                    bytes.splice(
                        position..position,
                        fragments[random(&mut state) % fragments.len()]
                            .iter()
                            .copied(),
                    );
                }
            }
        }
        let source = String::from_utf8_lossy(&bytes);
        std::fs::write(&current, source.as_bytes()).unwrap();
        let result = std::panic::catch_unwind(|| {
            let _ = crust::compile(&source);
            let _ = crust::analyze(&source);
        });
        if let Err(panic) = result {
            let crash = directory.join(format!("crash_{seed}_{iteration}.cr"));
            std::fs::write(&crash, source.as_bytes()).unwrap();
            eprintln!(
                "compiler panic: {} (seed {seed}, iteration {iteration})",
                crash.display()
            );
            std::panic::resume_unwind(panic);
        }
    }
    println!("Checked {iterations} mutations with seed {seed}");
    return;
}
