import "bindings.cr";
extern header "native.h";
import "../../lib/result.cr";

bool sqlite_terminated(const uint8_t[]& text) {
    for (size_t i=0; i<text.length; i++) {
        if (text[i]==0) return true;
    }
    return false;
}

struct Database {
    private sqlite3* handle;

    static Result<Self, int> open(const uint8_t[]& path) {
        if (!sqlite_terminated(path)) return Result<Self,int>::failure(SQLITE_MISUSE);
        unsafe {
            sqlite3* handle=0;
            int code=sqlite3_open((const char*)&path[0], &handle);
            if (code!=SQLITE_OK) {
                sqlite3_close(handle);
                return Result<Self,int>::failure(code);
            }
            return Result<Self,int>::success((Self){.handle=handle});
        }
    }

    Result<Statement, int> prepare(const uint8_t[]& sql) const borrows(this) {
        if (!sqlite_terminated(sql)) return Result<Statement,int>::failure(SQLITE_MISUSE);
        unsafe {
            sqlite3_stmt* handle=0;
            int code=sqlite3_prepare_v2(this.handle, (const char*)&sql[0], -1, &handle, 0);
            if (code!=SQLITE_OK || handle==0) {
                sqlite3_finalize(handle);
                if (code==SQLITE_OK) code=SQLITE_MISUSE;
                return Result<Statement,int>::failure(code);
            }
            return Result<Statement,int>::success(Statement::adopt(handle, &const this));
        }
    }

    ~Database() { unsafe { sqlite3_close(this.handle); } }
};

struct Statement {
    private sqlite3_stmt* handle;
    private const Database& database;
    private bool started;
    private bool row;
    private bool finished;
    private int failure;

    // The caller transfers one statement belonging to this live database.
    unsafe static Self adopt(sqlite3_stmt* handle, const Database& database) borrows(database) {
        return (Self){.handle=handle,.database=&database,.started=false,.row=false,.finished=false,.failure=SQLITE_OK};
    }

    Result<Unit,int> bind_integer(int index, int64_t value) {
        if (this.started) return Result<Unit,int>::failure(SQLITE_MISUSE);
        unsafe {
            int code=sqlite3_bind_int64(this.handle,index,value);
            if (code!=SQLITE_OK) return Result<Unit,int>::failure(code);
        }
        return Result<Unit,int>::success(Unit::Value);
    }

    Result<bool,int> step() {
        if (this.finished) {
            if (this.failure!=SQLITE_OK) return Result<bool,int>::failure(this.failure);
            return Result<bool,int>::success(false);
        }
        this.started=true;
        unsafe {
            int code=sqlite3_step(this.handle);
            this.row=code==SQLITE_ROW;
            if (this.row) return Result<bool,int>::success(true);
            this.finished=true;
            if (code!=SQLITE_DONE) {
                this.failure=code;
                return Result<bool,int>::failure(code);
            }
        }
        return Result<bool,int>::success(false);
    }

    Result<int64_t,int> integer(int column) const {
        if (!this.row) return Result<int64_t,int>::failure(SQLITE_MISUSE);
        unsafe {
            if (column<0 || column>=sqlite3_column_count(this.handle)) {
                return Result<int64_t,int>::failure(SQLITE_RANGE);
            }
            if (sqlite3_column_type(this.handle,column)!=SQLITE_INTEGER) {
                return Result<int64_t,int>::failure(SQLITE_MISMATCH);
            }
            return Result<int64_t,int>::success(sqlite3_column_int64(this.handle,column));
        }
    }

    Result<Unit,int> reset() {
        this.started=false;
        this.row=false;
        this.finished=false;
        this.failure=SQLITE_OK;
        unsafe {
            int code=sqlite3_reset(this.handle);
            if (code!=SQLITE_OK) return Result<Unit,int>::failure(code);
        }
        return Result<Unit,int>::success(Unit::Value);
    }

    ~Statement() { unsafe { sqlite3_finalize(this.handle); } }
};
