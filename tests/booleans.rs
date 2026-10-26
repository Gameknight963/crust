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
fn boolean_literals_scalar_conversions_and_conditions_follow_c_rules() {
    run(
        r#"
        bool nonzero(int n) { return n; }
        int number(bool b) { return b; }
        int main() {
            bool yes = true;
            bool no = false;
            print(yes); print(no);
            bool converted = -42;
            print(converted);
            print(nonzero(0)); print(number(3));
            if (yes && !no) { print(42); }
            while (yes) { yes = false; }
            print(yes);
            return 0;
        }
    "#,
        "1\n0\n1\n0\n1\n42\n0\n",
        "",
    );
}

#[test]
fn boolean_struct_fields_typedefs_methods_and_value_copies_work() {
    run(
        r#"
        typedef bool Flag;
        struct Switch {
            Flag enabled;
            static Self create(Flag value) { return (Self){ .enabled = value }; }
            Flag get() const { return this->enabled; }
            void toggle() { this->enabled = !this->enabled; }
        };
        int main() {
            Switch first = Switch::create(true);
            Switch second = first;
            second.toggle();
            print(first.get()); print(second.get());
            return 0;
        }
    "#,
        "1\n0\n",
        "",
    );
}

#[test]
fn shared_and_mutable_boolean_borrows_support_reborrowing_and_mutation() {
    run(
        r#"
        void set(bool^ value, bool next) { *value = next; }
        bool read(const bool^ value) { return *value; }
        void relay(bool^ value) {
            bool next = !read(^const *value);
            set(^*value, next);
        }
        int main() {
            bool value = false;
            set(^value, 42);
            print(read(^const value));
            relay(^value);
            print(read(^const value));
            return 0;
        }
    "#,
        "1\n0\n",
        "",
    );
}

#[test]
fn boolean_control_flow_preserves_owner_cleanup_and_short_circuiting() {
    run(
        r#"
        int mark() { print(999); return 1; }
        int main() {
            BoxInt value = box_int(7);
            bool flag = false;
            if (flag && mark()) { print(0); }
            if (true || mark()) { print(*value); }
            for (bool once = true; once; once = false) {
                BoxInt temporary = box_int(8);
                continue;
            }
            return 0;
        }
    "#,
        "7\n",
        "drop:8\ndrop:7\n",
    );
}

#[test]
fn scalar_conversions_do_not_convert_borrow_types_or_permit_shared_writes() {
    for source in [
        "int read(const int^ n) { return *n; } int main() { bool b = true; return read(^const b); }",
        "bool read(const bool^ n) { return *n; } int main() { int n = 1; return read(^const n); }",
        "void bad(const bool^ b) { *b = true; } int main() { return 0; }",
        "int main() { bool b = true; move(b); return 0; }",
        "int main() { bool b = true; b += 1; return 0; }",
    ] {
        assert!(crust::check(source).is_err(), "should reject {source}");
    }
}
