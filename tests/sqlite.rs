use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn root() -> PathBuf {
    return PathBuf::from(env!("CARGO_MANIFEST_DIR"));
}

fn enabled() -> bool {
    return std::env::var("CRUST_TEST_SQLITE").as_deref() == Ok("1");
}

fn declarations() -> String {
    let header = root().join("examples/sqlite_project/native.h");
    return format!(
        "{}\n{}\n{}",
        include_str!("../examples/sqlite_project/bindings.cr"),
        include_str!("../lib/result.cr"),
        include_str!("../examples/sqlite_project/database.cr")
            .replace("import \"bindings.cr\";", "")
            .replace("import \"../../lib/result.cr\";", "")
            .replace(
                "\"native.h\"",
                &serde_json::to_string(&header.to_string_lossy().replace('\\', "/")).unwrap()
            )
    );
}

fn native_options() -> crust::native::Options {
    let sanitizers = std::env::var("CRUST_TEST_SANITIZERS")
        .unwrap_or_default()
        .split(',')
        .filter(|name| !name.is_empty())
        .map(|name| match name {
            "address" => crust::native::Sanitizer::Address,
            "undefined" => crust::native::Sanitizer::Undefined,
            _ => panic!("unsupported sanitizer {name}"),
        })
        .collect();
    return crust::native::Options {
        defines: vec![
            "SQLITE_THREADSAFE=0".to_owned(),
            "SQLITE_OMIT_LOAD_EXTENSION=1".to_owned(),
            "SQLITE_DQS=0".to_owned(),
        ],
        sanitizers,
        ..Default::default()
    };
}

fn sqlite_archive() -> &'static Path {
    static ARCHIVE: OnceLock<PathBuf> = OnceLock::new();
    return ARCHIVE.get_or_init(|| {
        let directory = root().join(format!("target/sqlite-tests/{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let archive = directory.join(if cfg!(windows) {
            "sqlite.lib"
        } else {
            "libsqlite.a"
        });
        crust::native::compile_static_library(
            &root().join("examples/sqlite_project/target/sqlite/sqlite3.c"),
            &archive,
            &native_options(),
        )
        .unwrap();
        return archive;
    });
}

fn run(source: &str) -> Output {
    let generated = crust::compile(&format!("{}\n{source}", declarations()))
        .unwrap()
        .replace("int main(void)", "int crust_sqlite_test_main(void)")
        .replace("sqlite3_open(", "tracked_sqlite_open(")
        .replace("sqlite3_close(", "tracked_sqlite_close(")
        .replace("sqlite3_prepare_v2(", "tracked_sqlite_prepare(")
        .replace("sqlite3_finalize(", "tracked_sqlite_finalize(");
    let header = root().join("examples/sqlite_project/native.h");
    let generated = format!(
        "#include {}\n#include <stdlib.h>\n{generated}\n{}",
        serde_json::to_string(&header.to_string_lossy().replace('\\', "/")).unwrap(),
        include_str!("native/sqlite_tracker.c")
    );
    let directory = root().join(format!("target/sqlite-tests/{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("case_{}.c", NEXT.fetch_add(1, Ordering::Relaxed)));
    let executable = path.with_extension(if cfg!(windows) { "exe" } else { "out" });
    std::fs::write(&path, generated).unwrap();
    let mut options = native_options();
    options.link_inputs.push(sqlite_archive().to_owned());
    crust::native::compile_file_with_options(&path, &executable, &options).unwrap();
    return Command::new(executable).output().unwrap();
}

fn success(output: Output, expected: &str) {
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\r', ""),
        expected
    );
}

#[test]
fn sqlite_wrapper_checks_without_downloading_the_native_library() {
    let source =
        include_str!("../examples/sqlite_project/main.cr").replace("import \"database.cr\";", "");
    crust::check(&format!("{}\n{source}", declarations())).unwrap();
}

#[test]
fn sqlite_statements_cannot_outlive_or_duplicate_their_owners() {
    let setup = r#"
        uint8_t path[9]=":memory:";uint8_t sql[16]="SELECT 42";
        Result<Database,int> opened=Database::open(&path);
        Database database=move(opened).take_value();
        Result<Statement,int> prepared=database.prepare(&sql);
        Statement statement=move(prepared).take_value();
    "#;
    for operation in [
        "Database moved=move(database);print(statement.step().is_ok());",
        "Statement copied=statement;",
        "Statement moved=move(statement);print(statement.step().is_ok());",
    ] {
        assert!(
            crust::check(&format!(
                "{} int main(){{{setup}{operation}return 0;}}",
                declarations()
            ))
            .is_err(),
            "{operation}"
        );
    }
    let source = format!(
        "{} Statement escape(const Database& outer){{{setup}return move(statement);}}int main(){{return 0;}}",
        declarations()
    );
    assert!(crust::check(&source).is_err());
}

#[test]
fn sqlite_bindings_match_the_pinned_header() {
    if !enabled() {
        return;
    }
    let functions = [
        "sqlite3_open",
        "sqlite3_close",
        "sqlite3_prepare_v2",
        "sqlite3_finalize",
        "sqlite3_step",
        "sqlite3_reset",
        "sqlite3_bind_int64",
        "sqlite3_column_count",
        "sqlite3_column_type",
        "sqlite3_column_int64",
    ];
    let macros = [
        "SQLITE_OK",
        "SQLITE_ERROR",
        "SQLITE_MISUSE",
        "SQLITE_RANGE",
        "SQLITE_MISMATCH",
        "SQLITE_INTEGER",
        "SQLITE_ROW",
        "SQLITE_DONE",
        "SQLITE_CONSTRAINT",
        "SQLITE_CANTOPEN",
    ];
    let bindings = crust::bindings::generate(
        &root().join("examples/sqlite_project/native.h"),
        &crust::bindings::Options {
            functions: functions.into_iter().map(str::to_owned).collect(),
            macros: macros.into_iter().map(str::to_owned).collect(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        bindings.replace('\r', ""),
        include_str!("../examples/sqlite_project/bindings.cr").replace('\r', "")
    );
}

#[test]
fn sqlite_moves_and_early_returns_release_statements_before_connections() {
    if !enabled() {
        return;
    }
    success(
        run(r#"
        Result<Statement,int> prepare_query(const Database& database){
            uint8_t sql[32]="SELECT 42";
            return database.prepare(&sql);
        }
        int query(int branch){
            uint8_t path[9]=":memory:";
            Result<Database,int> opened=Database::open(&path);
            if(!opened.is_ok())return 1;
            Database original=move(opened).take_value();
            Database database=move(original);
            Result<Statement,int> prepared=prepare_query(&const database);
            if(!prepared.is_ok())return 2;
            Statement first=move(prepared).take_value();
            Statement statement=move(first);
            if(branch==0)return 0;
            Result<bool,int> row=statement.step();if(!row.is_ok() || !row.value())return 3;
            if(branch==1)return 0;
            print(statement.integer(0).value());return 0;
        }
        int main(){for(int branch=0;branch<3;branch++){if(query(branch)!=0)return 1;}return 0;}
    "#),
        "42\n",
    );
}

#[test]
fn sqlite_executes_real_queries_and_recovers_from_sql_errors() {
    if !enabled() {
        return;
    }
    let source =
        include_str!("../examples/sqlite_project/main.cr").replace("import \"database.cr\";", "");
    success(run(&source), "1\n1\n3\n33\n");
    let output = Command::new(env!("CARGO_BIN_EXE_crust"))
        .args(["run", "--project"])
        .arg(root().join("examples/sqlite_project"))
        .output()
        .unwrap();
    success(output, "1\n1\n3\n33\n");
}

#[test]
fn sqlite_checks_rows_columns_inputs_and_statement_state() {
    if !enabled() {
        return;
    }
    success(
        run(r#"
        int main(){
            uint8_t bad[2]={65,66};
            if(Database::open(&bad).is_ok())return 1;
            uint8_t path[9]=":memory:";
            Result<Database,int> opened=Database::open(&path);
            if(!opened.is_ok())return 2;
            Database database=move(opened).take_value();
            if(database.prepare(&bad).is_ok())return 3;
            uint8_t empty[1]={0};if(database.prepare(&empty).is_ok())return 4;
            uint8_t sql[32]="SELECT ?, 'text'";
            Result<Statement,int> prepared=database.prepare(&sql);
            if(!prepared.is_ok())return 5;
            Statement query=move(prepared).take_value();
            if(query.integer(0).is_ok() || query.bind_integer(0,1).is_ok())return 6;
            if(!query.bind_integer(1,42).is_ok())return 7;
            Result<bool,int> row=query.step();if(!row.is_ok() || !row.value())return 8;
            if(query.bind_integer(1,7).is_ok())return 9;
            if(query.integer(-1).is_ok() || query.integer(2).is_ok() || query.integer(1).is_ok())return 10;
            print(query.integer(0).value());
            Result<bool,int> done=query.step();if(!done.is_ok() || done.value())return 11;
            done=query.step();if(!done.is_ok() || done.value() || query.integer(0).is_ok())return 12;
            if(!query.reset().is_ok() || !query.bind_integer(1,7).is_ok())return 13;
            row=query.step();if(!row.is_ok() || !row.value())return 14;
            print(query.integer(0).value());
            uint8_t overflow_sql[32]="SELECT abs(?)";
            Result<Statement,int> overflow_prepared=database.prepare(&overflow_sql);
            if(!overflow_prepared.is_ok())return 15;
            Statement overflow=move(overflow_prepared).take_value();
            if(!overflow.bind_integer(1,-9223372036854775808).is_ok())return 16;
            Result<bool,int> error=overflow.step();if(error.is_ok())return 17;
            Result<bool,int> repeated=overflow.step();
            if(repeated.is_ok() || repeated.error_value()!=error.error_value())return 18;
            Result<Unit,int> reset_error=overflow.reset();
            if(reset_error.is_ok() || reset_error.error_value()!=error.error_value())return 19;
            if(!overflow.bind_integer(1,42).is_ok())return 20;
            row=overflow.step();if(!row.is_ok() || !row.value())return 21;
            print(overflow.integer(0).value());return 0;
        }
    "#),
        "42\n7\n42\n",
    );
}

#[test]
fn sqlite_releases_failed_opens_and_allocation_failures() {
    if !enabled() {
        return;
    }
    success(
        run(r#"
        extern void sqlite_fail_after(int budget);
        int main(){
            uint8_t missing[2]="/";
            Result<Database,int> failed=Database::open(&missing);
            if(failed.is_ok())return 1;
            uint8_t path[9]=":memory:";uint8_t sql[32]="SELECT 42";
            int failures=0;int successes=0;int prepare_failures=0;
            for(int budget=0;budget<100;budget++){
                unsafe{sqlite_fail_after(budget);}
                Result<Database,int> opened=Database::open(&path);
                unsafe{sqlite_fail_after(-1);}
                if(!opened.is_ok()){failures++;continue;}
                Database database=move(opened).take_value();successes++;
                for(int limit=0;limit<25;limit++){
                    unsafe{sqlite_fail_after(limit);}
                    Result<Statement,int> prepared=database.prepare(&sql);
                    unsafe{sqlite_fail_after(-1);}
                    if(!prepared.is_ok()){prepare_failures++;continue;}
                    Statement statement=move(prepared).take_value();
                    if(!statement.step().is_ok())return 2;
                }
            }
            print(failures>0);print(successes>0);print(prepare_failures>0);return 0;
        }
    "#),
        "1\n1\n1\n",
    );
}
