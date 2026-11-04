mod common;
use common::compiler as crustc;

#[test]
fn mutable_arguments_and_receivers_allow_nested_value_computation_in_source_order() {
    let source = r#"
        struct Context {
            int value;
            int next() { this.value+=1; print(this.value); return this.value; }
            int count() const { return this.value; }
            void accept(int number) { this.value+=number; print(this.value); }
        };
        int next(Context& ctx) { return ctx.next(); }
        int read(const Context& ctx) { return ctx.value; }
        void accept(Context& ctx, int number) { ctx.accept(number); }
        void same(int& target, int value) { target=value; }
        int main() {
            Context ctx={.value=1};
            accept(&ctx,next(&ctx));
            accept(&ctx,read(&ctx));
            ctx.accept(ctx.next());
            ctx.accept(ctx.count());
            int number=42; int& reference=&number; same(reference,reference); print(reference);
            return 0;
        }
    "#;
    let output = common::tracked_run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        "2\n4\n8\n9\n18\n36\n42\n"
    );
}

#[test]
fn reservations_keep_simultaneous_aliases_moves_and_unstable_projections_rejected() {
    for body in [
        "both(&owner,&owner);",
        "shared(&owner,&const owner);",
        "use_owner(&owner,take(move(owner)));",
        "use_int(owner.get_mut(),take(move(owner)));",
    ] {
        let source = format!(
            "void both(Box<int>& a,Box<int>& b){{}} void shared(Box<int>& a,const Box<int>& b){{}} void use_owner(Box<int>& a,int value){{}} void use_int(int& a,int value){{}} int take(Box<int> a){{return a.get();}} int main(){{Box<int> owner=Box<int>::create(1); {body} return 0;}}"
        );
        assert!(crustc::check(&source).is_err(), "accepted {body}");
    }
}
