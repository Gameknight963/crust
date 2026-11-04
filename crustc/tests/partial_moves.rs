mod common;
use common::compiler as crustc;

fn run(source: &str, expected: &str, drops: &str) {
    let output = common::tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        expected
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).replace('\r', ""),
        drops
    );
}

#[test]
fn extracts_fields_reinitializes_and_cleans_only_remaining_owners() {
    run(
        r#"
        struct Pair { Box<int> first; Box<int> second; };
        int main() {
            Pair pair={.first=Box<int>::create(1),.second=Box<int>::create(2)};
            Box<int> first=move(pair.first);
            print(pair.second.get());
            pair.first=Box<int>::create(3);
            print(pair.first.get());
            pair.second=move(first);
            Pair restored=move(pair);
            print(restored.second.get());
            return 0;
        }
    "#,
        "2\n3\n1\n",
        "drop:2\ndrop:1\ndrop:3\n",
    );
}

#[test]
fn nested_fields_branches_parameters_and_early_returns_preserve_cleanup() {
    run(
        r#"
        struct Pair { Box<int> first; Box<int> second; };
        struct Report { Pair pair; Box<int> report; };
        Box<int> extract(Report value, bool choose) {
            if (choose) { return move(value.pair.first); }
            return move(value.pair.second);
        }
        int main() {
            Report value={.pair={.first=Box<int>::create(1),.second=Box<int>::create(2)},.report=Box<int>::create(3)};
            Box<int> result=extract(move(value),true);
            print(result.get());
            return 0;
        }
    "#,
        "1\n",
        "drop:3\ndrop:2\ndrop:1\n",
    );
}

#[test]
fn branch_cleanup_and_loop_restoration_follow_each_field_liveness() {
    run(
        r#"
        struct Pair { Box<int> first; Box<int> second; };
        void choose(bool take) {
            Pair pair={.first=Box<int>::create(1),.second=Box<int>::create(2)};
            if (take) { Box<int> value=move(pair.first); }
            print(pair.second.get());
        }
        int main(){choose(true);choose(false);return 0;}
    "#,
        "2\n2\n",
        "drop:1\ndrop:2\ndrop:2\ndrop:1\n",
    );
    run(
        r#"
        struct Pair { Box<int> first; Box<int> second; };
        int main(){
            Pair pair={.first=Box<int>::create(1),.second=Box<int>::create(2)};
            for(int i=0;i<2;i++){
                Box<int> value=move(pair.first);
                pair.first=Box<int>::create(i+3);
                if(i==0){continue;} break;
            }
            Box<int> value=move(pair.first);
            pair=(Pair){.first=Box<int>::create(5),.second=Box<int>::create(6)};
            print(value.get()); return 0;
        }
    "#,
        "4\n",
        "drop:1\ndrop:3\ndrop:2\ndrop:4\ndrop:6\ndrop:5\n",
    );
}

#[test]
fn partial_moves_reject_invalid_reads_aliases_destructors_and_loop_backedges() {
    let prefix = "struct Pair { Box<int> first; Box<int> second; };";
    let setup = "Pair pair={.first=Box<int>::create(1),.second=Box<int>::create(2)};";
    for body in [
        "Box<int> value=move(pair.first); print(pair.first.get());",
        "Box<int> value=move(pair.first); Pair other=move(pair);",
        "if (true) { Box<int> value=move(pair.first); } print(pair.first.get());",
        "while (true) { Box<int> value=move(pair.first); }",
        "const Box<int>& loan=&pair.first; Box<int> value=move(pair.first); print(loan.get());",
        "Pair& loan=&pair; Box<int> value=move(loan.first);",
    ] {
        assert!(
            crustc::check(&format!("{prefix} int main(){{{setup}{body}return 0;}}")).is_err(),
            "accepted {body}"
        );
    }
    assert!(crustc::check("struct Pair { Box<int> first; Box<int> second; ~Pair() { print(this.second.get()); } }; int main(){Pair pair={.first=Box<int>::create(1),.second=Box<int>::create(2)}; Box<int> value=move(pair.first); return 0;}").is_err());
}
