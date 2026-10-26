mod common;

fn run(source: &str, stdout: &str, drops: &str) {
    let output = common::tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        stdout
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
        drops
    );
}

#[test]
fn separate_fields_can_be_borrowed_together_in_calls_and_locals() {
    run(
        include_str!("../examples/disjoint_fields.cr"),
        "31\n22\n",
        "",
    );
}

#[test]
fn nested_siblings_allow_disjoint_reads_writes_and_reborrows() {
    run(
        r#"
        struct Pair { int left; int right; };
        struct Tree { Pair a; Pair b; };
        void update(Tree^ tree) {
            int^ left = ^tree->a.left;
            int^ right = ^tree->a.right;
            Pair^ other = ^tree->b;
            *left += 1; *right += 2;
            other->left += 3;
            print(other->right);
        }
        int main() {
            Tree tree = { .a = { .left = 1, .right = 2 }, .b = { .left = 3, .right = 4 } };
            update(^tree);
            print(tree.a.left); print(tree.a.right); print(tree.b.left);
            return 0;
        }
    "#,
        "4\n2\n4\n6\n",
        "",
    );
}

#[test]
fn owner_fields_can_be_replaced_while_a_sibling_is_borrowed() {
    run(
        r#"
        struct Pair {
            BoxInt left;
            BoxInt right;
            ~Pair() { print(*this->left + *this->right); }
        };
        int main() {
            Pair pair = { .left = box_int(1), .right = box_int(2) };
            {
                int^ left = ^*pair.left;
                pair.right = box_int(3);
                *left += 1;
                print(*pair.right);
            }
            return 0;
        }
    "#,
        "3\n5\n",
        "drop:2\ndrop:3\ndrop:2\n",
    );
}

#[test]
fn returned_loans_preserve_narrow_input_paths_and_cover_unknown_subfields() {
    run(
        r#"
        struct Pair { int left; int right; };
        int^ identity(int^ value) { return value; }
        int main() {
            Pair pair = { .left = 1, .right = 2 };
            {
                int^ left = identity(^pair.left);
                pair.right += 3;
                print(pair.right);
                *left += 1;
            }
            print(pair.left);
            return 0;
        }
    "#,
        "5\n2\n",
        "",
    );
    let source = "struct Pair { int left; int right; }; int^ left(Pair^ p) { return ^p->left; } int main() { Pair p = { .left = 1, .right = 2 }; int^ r = left(^p); p.right = 3; return 0; }";
    assert!(
        crust::check(source).is_err(),
        "a borrowed return covers its declared whole input"
    );
}

#[test]
fn compound_assignments_capture_one_field_and_can_mutate_a_disjoint_sibling() {
    run(
        r#"
        struct Pair { int left; int right; };
        int change(int^ value) { *value += 1; return *value; }
        int main() {
            Pair pair = { .left = 1, .right = 2 };
            pair.left += change(^pair.right);
            print(pair.left); print(pair.right);
            return 0;
        }
    "#,
        "4\n3\n",
        "",
    );
}

#[test]
fn overlapping_fields_parents_and_array_elements_still_conflict() {
    let prelude = "struct Pair { int left; int right; }; struct Tree { Pair a; Pair b; }; void take(int^ a, int^ b) {}";
    for body in [
        "Pair p = { .left = 1, .right = 2 }; take(^p.left, ^p.left);",
        "Pair p = { .left = 1, .right = 2 }; int^ r = ^p.left; p.left = 3;",
        "Pair p = { .left = 1, .right = 2 }; int^ r = ^p.left; Pair copy = p;",
        "Pair p = { .left = 1, .right = 2 }; int^ r = ^p.left; p = { .left = 3, .right = 4 };",
        "Tree t = { .a = { .left = 1, .right = 2 }, .b = { .left = 3, .right = 4 } }; int^ r = ^t.a.left; Pair^ p = ^t.a;",
        "Tree t = { .a = { .left = 1, .right = 2 }, .b = { .left = 3, .right = 4 } }; Pair^ p = ^t.a; int^ r = ^t.a.right;",
        "int a[2] = {1, 2}; take(^a[0], ^a[1]);",
        "BoxInt a[2] = {box_int(1), box_int(2)}; int^ r = ^*a[0]; a[1] = box_int(3);",
    ] {
        let source = format!("{prelude} int main() {{ {body} return 0; }}");
        assert!(crust::check(&source).is_err(), "should reject {source}");
    }
    for body in [
        "BoxInt^ r = ^p.left; Pair moved = move(p);",
        "int^ r = ^*p.left; p.left = box_int(3);",
    ] {
        let source = format!(
            "struct Pair {{ BoxInt left; BoxInt right; }}; int main() {{ Pair p = {{ .left = box_int(1), .right = box_int(2) }}; {body} return 0; }}"
        );
        assert!(crust::check(&source).is_err(), "should reject {source}");
    }
}
