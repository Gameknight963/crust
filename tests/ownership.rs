mod common;
use common::compiler as crust;

use common::tracked_run;

#[test]
fn else_if_chains_preserve_returns_branch_scopes_and_owner_cleanup() {
    let source = r#"
        int select(int kind) {
            Box<int> outer = Box<int>::create(9);
            if (kind == 0) {
                Box<int> local = Box<int>::create(10);
                return local.get();
            } else if (kind == 1) {
                Box<int> local = Box<int>::create(11);
                return local.get();
            } else if (kind == 2) {
                Box<int> local = Box<int>::create(12);
                return local.get();
            } else {
                return outer.get();
            }
        }
        int main() {
            for (int kind = 0; kind < 4; kind += 1) { print(select(kind)); }
            return 0;
        }
    "#;
    let output = tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "10\n11\n12\n9\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        "drop:10\ndrop:9\ndrop:11\ndrop:9\ndrop:12\ndrop:9\ndrop:9\n"
    );
    for body in [
        "if (kind == 0) { } else if (kind == 1) { move(owner); } print(owner.get()); return 0;",
        "if (kind == 0) { } else if (kind == 1) { int local = 1; } else { print(local); } return 0;",
        "if (kind == 0) { return 1; } else if (kind == 1) { return 2; }",
    ] {
        let source = format!(
            "int select(int kind) {{ Box<int> owner = Box<int>::create(9); {body} }} int main() {{ return select(0); }}"
        );
        assert!(crust::check(&source).is_err(), "{source}");
    }
}

#[test]
fn rejects_copying_and_using_moved_owners_across_control_flow() {
    let prelude = "int take(Box<int> owner) { return owner.get(); } ";
    let cases = [
        (
            "Box<int> a = Box<int>::create(1); Box<int> b = a;",
            "cannot be copied",
        ),
        (
            "Box<int> a = Box<int>::create(1); take(a);",
            "cannot be copied",
        ),
        (
            "Box<int> a = Box<int>::create(1); take(move(a)); print(a.get());",
            "has been moved",
        ),
        (
            "Box<int> a = Box<int>::create(1); a.get_mut() = take(move(a));",
            "has been moved",
        ),
        (
            "Box<int> a = Box<int>::create(1); take(move(a)); take(move(a));",
            "has been moved",
        ),
        (
            "Box<int> a = Box<int>::create(1); if (1) { take(move(a)); } print(a.get());",
            "may have been moved",
        ),
        (
            "Box<int> a = Box<int>::create(1); 0 && take(move(a)); print(a.get());",
            "may have been moved",
        ),
        (
            "Box<int> a = Box<int>::create(1); while (1) { take(move(a)); }",
            "restore moved outer owners",
        ),
        (
            "Box<int> a = Box<int>::create(1); while (take(move(a))) {}",
            "loop condition cannot consume",
        ),
        ("move(Box<int>::create(1));", "requires the name"),
        ("Box<int> a = 1;", "found int"),
        ("int a = Box<int>::create(1);", "found struct Box<int>"),
        ("int a = 1; print(*a);", "dereference"),
        ("print(Box<int>::create(1).get());", "dereference"),
    ];
    for (body, message) in cases {
        let source = format!("{prelude} int main(void) {{ {body} return 0; }}");
        let error = crust::compile(&source).unwrap_err();
        assert!(error.message.contains(message), "{source}: {error}");
    }
}

#[test]
fn accepts_returning_branches_reinitialization_and_restored_loop_owners() {
    crust::check(
        r#"
        int take(Box<int> owner) { return owner.get(); }
        int main(void) {
            Box<int> a = Box<int>::create(1);
            if (0) { take(move(a)); return 0; }
            print(a.get());
            if (1) { take(move(a)); }
            a = Box<int>::create(2);
            int n = 3;
            while (n > 0) {
                take(move(a));
                a = Box<int>::create(n);
                n = n - 1;
            }
            print(a.get());
            take(move(a));
            (a) = Box<int>::create(3);
            print(a.get());
            return 0;
        }
    "#,
    )
    .unwrap();
}

#[test]
fn frees_owners_on_scope_exit_replacement_discard_and_early_return_in_reverse_order() {
    let output = tracked_run(
        r#"
        Box<int> pass(Box<int> owner) {
            Box<int> local = Box<int>::create(20);
            return move(owner);
        }
        int main(void) {
            Box<int> first = Box<int>::create(10);
            { Box<int> nested = Box<int>::create(30); }
            Box<int> second = pass(move(first));
            second = Box<int>::create(40);
            Box<int>::create(50);
            Box<int> last = Box<int>::create(60);
            print(second.get());
            return 0;
        }
    "#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "40\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        "drop:30\ndrop:20\ndrop:10\ndrop:50\ndrop:60\ndrop:40\n"
    );
}

#[test]
fn frees_resources_each_iteration_and_only_on_paths_that_own_them() {
    let output = tracked_run(
        r#"
        int take(Box<int> owner) { return owner.get(); }
        int main(void) {
            Box<int> owner = Box<int>::create(0);
            int n = 1;
            while (n < 10) {
                take(move(owner));
                owner = Box<int>::create(n);
                n = n + 1;
            }
            if (n == 10) { take(move(owner)); }
            owner = Box<int>::create(42);
            owner = move(owner);
            print(owner.get());
            return 0;
        }
    "#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "42\n"
    );
    let drops = String::from_utf8_lossy(&output.stderr);
    assert_eq!(drops.lines().count(), 11);
}

#[test]
fn replacing_through_an_exclusive_borrow_drops_the_old_owner_and_preserves_the_new_one() {
    let output = tracked_run(
        r#"
        void replace(Box<int>& owner) { owner = Box<int>::create(2); }
        int read(const Box<int>& owner) { return owner.get(); }
        int main(void) {
            Box<int> owner = Box<int>::create(1);
            replace(&owner);
            print(read(&const owner));
            return 0;
        }
    "#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        "2\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        "drop:1\ndrop:2\n"
    );
}

#[test]
fn former_owner_spellings_are_ordinary_source_names_and_boxes_need_source_definitions() {
    let source = r#"
struct BoxInt { int value; };
int box_int(int value) { return value + 1; }
typedef int (*Callback)(int);
int main() {
    BoxInt first={.value=41}; BoxInt second=first;
    Callback callback=box_int;
    unsafe { print(callback(second.value)); }
    print(sizeof(BoxInt));
    return 0;
}
"#;
    let output = tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "42\n4\n"
    );
    assert!(
        ::crust::check("int main() { Box<int> value=Box<int>::create(1); return 0; }").is_err()
    );
}
