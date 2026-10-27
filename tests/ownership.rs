mod common;

use common::tracked_run;

#[test]
fn else_if_chains_preserve_returns_branch_scopes_and_owner_cleanup() {
    let source = r#"
        int select(int kind) {
            BoxInt outer = box_int(9);
            if (kind == 0) {
                BoxInt local = box_int(10);
                return *local;
            } else if (kind == 1) {
                BoxInt local = box_int(11);
                return *local;
            } else if (kind == 2) {
                BoxInt local = box_int(12);
                return *local;
            } else {
                return *outer;
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
        "if (kind == 0) { } else if (kind == 1) { move(owner); } print(*owner); return 0;",
        "if (kind == 0) { } else if (kind == 1) { int local = 1; } else { print(local); } return 0;",
        "if (kind == 0) { return 1; } else if (kind == 1) { return 2; }",
    ] {
        let source = format!(
            "int select(int kind) {{ BoxInt owner = box_int(9); {body} }} int main() {{ return select(0); }}"
        );
        assert!(crust::check(&source).is_err(), "{source}");
    }
}

#[test]
fn rejects_copying_and_using_moved_owners_across_control_flow() {
    let prelude = "int take(BoxInt owner) { return *owner; } ";
    let cases = [
        ("BoxInt a = box_int(1); BoxInt b = a;", "cannot be copied"),
        ("BoxInt a = box_int(1); take(a);", "cannot be copied"),
        (
            "BoxInt a = box_int(1); take(move(a)); print(*a);",
            "has been moved",
        ),
        (
            "BoxInt a = box_int(1); *a = take(move(a));",
            "has been moved",
        ),
        (
            "BoxInt a = box_int(1); take(move(a)); take(move(a));",
            "has been moved",
        ),
        (
            "BoxInt a = box_int(1); if (1) { take(move(a)); } print(*a);",
            "may have been moved",
        ),
        (
            "BoxInt a = box_int(1); 0 && take(move(a)); print(*a);",
            "may have been moved",
        ),
        (
            "BoxInt a = box_int(1); while (1) { take(move(a)); }",
            "restore moved outer owners",
        ),
        (
            "BoxInt a = box_int(1); while (take(move(a))) {}",
            "loop condition cannot consume",
        ),
        ("move(box_int(1));", "requires the name"),
        ("BoxInt a = 1;", "found int"),
        ("int a = box_int(1);", "found BoxInt"),
        ("int a = 1; print(*a);", "dereference"),
        ("print(*box_int(1));", "dereference"),
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
        int take(BoxInt owner) { return *owner; }
        int main(void) {
            BoxInt a = box_int(1);
            if (0) { take(move(a)); return 0; }
            print(*a);
            if (1) { take(move(a)); }
            a = box_int(2);
            int n = 3;
            while (n > 0) {
                take(move(a));
                a = box_int(n);
                n = n - 1;
            }
            print(*a);
            take(move(a));
            (a) = box_int(3);
            print(*a);
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
        BoxInt pass(BoxInt owner) {
            BoxInt local = box_int(20);
            return move(owner);
        }
        int main(void) {
            BoxInt first = box_int(10);
            { BoxInt nested = box_int(30); }
            BoxInt second = pass(move(first));
            second = box_int(40);
            box_int(50);
            BoxInt last = box_int(60);
            print(*second);
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
        int take(BoxInt owner) { return *owner; }
        int main(void) {
            BoxInt owner = box_int(0);
            int n = 1;
            while (n < 10) {
                take(move(owner));
                owner = box_int(n);
                n = n + 1;
            }
            if (n == 10) { take(move(owner)); }
            owner = box_int(42);
            owner = move(owner);
            print(*owner);
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
        void replace(BoxInt^ owner) { *owner = box_int(2); }
        int read(const BoxInt^ owner) { return **owner; }
        int main(void) {
            BoxInt owner = box_int(1);
            replace(^owner);
            print(read(^const owner));
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
